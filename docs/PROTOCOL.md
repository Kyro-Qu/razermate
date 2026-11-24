# PROTOCOL.md — Basilisk V3 有线版控制协议规格

> 目标设备：Razer Basilisk V3 有线版，`VID_1532 & PID_0099`，固件 v1.2
> 本文是 `razer-protocol` crate 的实现依据。
> 最后核对：2026-11-23，全部 A 级条目均在本机实机验证。

## 证据等级

| 等级 | 含义 |
|---|---|
| **A** | 本机实机验证（发报文、读回、物理按键） |
| **B** | 逆向文档记载 + 本机验证与之一致 |
| **C** | 逆向文档记载，本机未单独验证 |

---

## 1. 物理与 USB 层

```
VID 0x1532  PID 0x0099  固件 1.2.0
```

设备暴露 4 个接口：

| 接口 | 用途 | 备注 |
|---|---|---|
| MI_00 | 鼠标（endpoint 1） | 标准 HID 鼠标，系统直接驱动 |
| MI_01 | 键盘 / consumer / system（endpoint 2） | 仅在 **driver mode** 下由厂商软件占用 |
| MI_02 | boot 键盘（endpoint 3） | 本协议不用 |
| **MI_03** | **厂商控制口** | usage page `0xFF00`，usage `0x01`，90 字节 feature report |

**本协议只走 MI_03。** Windows 自带 `HID-compliant device` 驱动即可，不需要雷蛇驱动、不需要内核驱动。（A）

打开方式：`CreateFileW` 打开 `\\?\hid#vid_1532&pid_0099&mi_03#...`，读写用 `HidD_SetFeature` / `HidD_GetFeature`。

---

## 2. 报文格式

### 2.1 结构

```c
struct Report {          // 90 字节
    u8  status;          // 发送恒 0x00
    u8  transaction_id;  // Basilisk V3 固定 0x1F
    u16 remaining_packets;// BE，恒 0
    u8  protocol_type;   // 0x00
    u8  data_size;
    u8  command_class;
    u8  command_id;
    u8  arguments[80];
    u8  crc;
    u8  reserved;        // 0x00
};
```

`HidD_SetFeature` / `HidD_GetFeature` 的长度是 **91**：首字节是 report id `0x00`，后接 90 字节结构体。（B，与 openrazer / razerqdhid 一致，本机可通信）

### 2.2 CRC

```rust
fn crc(bytes: &[u8; 90]) -> u8 {
    // bytes[2] ..= bytes[87] 的异或
    bytes[2..88].iter().fold(0u8, |a, b| a ^ b)
}
```
结果写入 `bytes[88]`。（B）

### 2.3 `data_size` 的真实语义（易错）

**`data_size` = 发送的参数区长度，不是响应长度。**

- 发送时 `arguments[0..data_size]` 有效，剩余部分补 `0x00`。
- **响应中，前 `data_size` 个字节是本条命令的应答数据**（通常前缀是请求参数的回显）。

举例（按键读取，`data_size = 10`）：

```
发送  arguments = [profile, button, layer, 0,0,0,0,0,0,0]
响应  arguments[0..10] = [profile, button, layer, fn, fn_size, data0..data4]
                        └────── 回显 3B ──────┘└──── 实际数据 7B ────┘
```

所以读取时解析偏移 = 回显长度。（A）

### 2.4 状态码

| 值 | 名称 | 处理 |
|---|---|---|
| 0 | NEW | — |
| 1 | **BUSY** | 继续重试 |
| 2 | **OK** | 成功 |
| 3 | FAIL | 报错 |
| 4 | TIMEOUT | 报错 |
| 5 | NOT_SUPPORTED | 报错 |

（B）

### 2.5 事务流程

```
1. 组装报文，算 CRC，HidD_SetFeature(0x00 + 90B)
2. 循环 HidD_GetFeature(0x00, 91)，去掉首字节
3. 校验 response.command_class == request.command_class
   且   response.command_id   == request.command_id
       不匹配 → 报错（说明有别的程序在抢设备）
4. status == OK      → 返回
   status == BUSY    → 继续循环
   其他              → 报错
```

重试策略（B，与 razerqdhid 一致）：最多 `15 × (wait_power + 1)` 次，第 i 次前睡 `10ms × (i+1)`。

**写操作后立刻读，大概率返回 BUSY 或旧值。** 必须等 ≥ 250ms 或走重试。（A，多次踩到）

---

## 3. 命令参考

约定：`cls` = command_class，`cmd` = command_id，`size` = data_size（= 应答数据长度）。

### 3.1 设备信息（cls `0x00`）

| 操作 | cmd | size | 发送参数 | 响应 |
|---|---|---|---|---|
| 读固件版本 | `0x81` | 4 | — | v_major, v_minor, v_patch, 0 |
| 读序列号 | `0x82` | 16 | — | 16 字节 ASCII，补 0 |
| 读设备模式 | `0x84` | 2 | — | mode, param |
| **写设备模式** | `0x04` | 2 | mode, param | 回显 |

| 读回报率 | `0x8E` | 2 | profile | profile, **delay_ms** |
| **写回报率** | `0x0E` | 2 | profile, delay_ms | 回显 |

`mode`：`0` = normal，`1` = bootloader，`2` = test，`3` = **driver**。（A：切到 3 再切回 0 已实测）

**回报率存的是轮询间隔而不是 Hz**（A：`0x00 0x8E 2 4 0` → `[04, 01]`，本机 1000 Hz）。
换算：`Hz = 1000 / delay_ms`。设备只认 `1`（1000 Hz）、`2`（500 Hz）、`8`（125 Hz）三档，
其它值不保证被接受。

**driver mode 的后果**：MI_01 的厂商 report 被激活，板载按键绑定与宏**全部失效**，必须由软件接管按键转发。normal mode 下固件自己按 Flash 里的绑定发键。

> ⚠️ 这是本项目最容易出事的地方。任何一次异常退出若把设备留在 driver mode，用户会觉得"宏坏了"。启动自检必须读一次 `0x84`。

### 3.2 按键与滚轮（cls `0x02`）

**读绑定** `cmd = 0x8C`，`size = 10`

```
发送  [profile, button, layer, 0,0,0,0,0,0,0]
响应  [profile, button, layer, fn, fn_size, data(5)]
```

**写绑定** `cmd = 0x0C`，`size = 10`

```
发送  [profile, button, layer, fn, fn_size, data(5)]
```

`layer`：`0` = normal，`1` = hypershift。
`profile`：`0` = direct（见第 5 节），`1..5` = 板载槽。

**`fn` 表（A，本机逐个对照实读值确证）**

| fn | 名称 | fn_size | data 布局 |
|---|---|---|---|
| `0` | off | 0 | — |
| `1` | 鼠标键 | 1 | `[mouse_code]` |
| **`2`** | **键 / 组合键** | **2** | **`[modifier_bits, usage]`** |
| `3` | 播放宏 | 3 | `[id_hi, id_lo, times]` |
| `4` | 按住播放宏 | 3 | `[id_hi, id_lo, 1]`（按住循环、松开即停；物理已验收） |
| `5` | 切换宏 | 3 | `[id_hi, id_lo, 1]`（点一次开始循环、再点一次停；物理已验收） |
| `6` | DPI 动作 | 1 | `[1 up / 2 down / 6 cycle]` |
| `7` | 下一个 profile | 1 | `[0x04]` |
| `10` | 媒体键 | 2 | `[code_hi, code_lo]` |
| `11` | 双击 | 1 | `[1]` |
| `12` | Hypershift | 1 | `[1]` |
| `13` | 连发键 | — | — |
| **`14`** | **扩展鼠标动作** | **1** | **`[mouse_code]`** |
| `18` | 滚轮模式切换 | 1 | `[1]` |

> ⚠️ 三个极易错的地方：
> 1. **`fn = 2` 才是键**，不是 3。`0x03` 是宏。
> 2. **data 顺序是 `[修饰位, 主键]`**，不是反过来。实读 `forward` = `04 1A` → alt + w。
> 3. **`fn = 0x0E` 不是 openrazer 说的 "turbo button"（长度 3）**，实测是本机滚轮左右倾
>    两个键的 fn，长度 1，data 就是鼠标动作码 `0x68/0x69`。写 `fn=1 [68]` 能原样读回
>    `fn=1 [68]`，说明设备不做归一化，`0x0E` 是独立的一支，按「扩展鼠标动作」处理。
> 4. **`fn = 4/5` 的 data 布局与 `fn = 3` 相同**（3 字节 `[id_hi, id_lo, times]`），
>    区别只在触发方式。`Binding::macro_play_with_mode` 用同一份报文写入；物理行为见 §8.1。

`modifier_bits`（A）：

| 位 | 键 | 位 | 键 |
|---|---|---|---|
| `0x01` | ctrl | `0x10` | 右 ctrl |
| `0x02` | shift | `0x20` | 右 shift |
| `0x04` | alt | `0x40` | 右 alt |
| `0x08` | win | `0x80` | 右 win |

`mouse_code`（作为绑定**动作**，与按钮 **ID** 不同）：`1` left、`2` right、`3` middle、`4` back、`5` forward、`9` wheelup、`10` wheeldown、`0x68` tiltleft、`0x69` tiltright。

> ⚠️ tilt 作动作是 `0x68/0x69`，作按钮 ID 是 `52/53`。两套编码不要混。

**其他滚轮相关**（A，本机逐条实测；命令码来源 razerqdhid `device.py`，行为已实测确认）：

| 操作 | 读 | 写 | size | 参数 |
|---|---|---|---|---|
| 滚轮模式 | `0x94` | `0x14` | 2 | profile, mode（0 段落 / 1 自由） |
| 滚轮加速 | `0x96` | `0x16` | 2 | profile, on |
| 智能卷动 | `0x97` | `0x17` | 2 | profile, on |

读应答 `data_size=2`，内容 `[profile, value]`：**第一字节回显请求的槽号，第二字节才是设定值**。
读命令只带 1 个参数时参数区补零，应答长度仍按 `data_size` 截 2 字节。

本机 5 个板载槽 + p0 的基线全部是 `mode=0 / accel=0 / smart=1`。实测结论：

- 三条写命令在 normal mode 下直接返回 `Ok`，**不需要 driver mode**；写后读回逐字节一致。
- **按槽独立**：只把 4 号槽 `mode` 写成 1，3 号槽仍为 0。
- **跨切槽保持**：`slot use 3` → `slot use 4` 之后，4 号槽仍是改写值，说明设定落在该 profile 上。
- **p0 可独立寻址**：写 p0 不影响 4 号槽，可纳入虚拟槽工作流。
- 回报率相反：`cls 0x00 / 0x8E` 不带 profile 参数，应答 `[00, delay_ms]`，是**设备级**设置。

### 3.3 按键 ID 表（A，`get_buttons` 实测）

| 名称 | ID | 名称 | ID |
|---|---|---|---|
| left | 1 | dpi | **96** |
| right | 2 | clutch（狙击） | **15** |
| middle | 3 | profile（底部） | **14** |
| **back** | **4** | wheelmode（滚轮模式键） | **106** |
| **forward** | **5** | wheelup | 9 |
| tiltleft | 52 | wheeldown | 10 |
| tiltright | 53 | | |

> ⚠️ `forward = 5`、`back = 4`。**不是 90/91**。用错 ID 时设备返回
> `status = FAIL`，看起来像"命令不支持"，极易误判。

### 3.4 Profile 管理（cls `0x05`）

| 操作 | cmd | size | 发送参数 | 响应 |
|---|---|---|---|---|
| 总数 | `0x8A` | 1 | — | `05` |
| 启用数 | `0x80` | 1 | — | `05` |
| 启用列表 | `0x81` | 6 | — | `[总数, id1..id5]` |
| 新建 | `0x02` | 1 | profile | 回显 |
| 删除 | `0x02` | 1 | profile | 回显 |
| **激活槽** | `0x04` | 1 | profile | 回显 |
| 读当前激活 | `0x04` | 1 | `0x00`? | profile |
| profile 描述 | `0x88`/`0x08` | 3 | profile, start | 分块数据 |

（A：总数/列表/激活均实测）

### 3.5 Flash 与宏（cls `0x06`）

**Flash 状态** `cmd = 0x8E`，`size = 14`，无参数

```
响应 [unk(2), total(4), available(4), recycled(4)]   全部大端
实测  00 64 | 00 01 68 00 | 00 01 61 00 | 00 00 09 00
       100 |    92160    |    90368    |    2304
```

**宏数量** `cmd = 0x80`，`size = 2` → `[count_hi, count_lo]`（A）

**读宏大小** `cmd = 0x88`，`size = 6`，发送 `[id_hi, id_lo, 0,0,0,0]`
→ 响应 `[id_hi, id_lo, len(4B BE)]`

**读宏内容** `cmd = 0x89`，`size = 71`，发送 `[id_hi, id_lo, off(4B), len(1B)]`
→ 响应 `[id_hi, id_lo, off(4B), len(1B), data...]`，数据从 **offset 7** 开始
每次最多取 64 字节，循环拼接。

**设置宏大小（分配）** `cmd = 0x08`，`size = 6`，发送 `[id_hi, id_lo, len(4B BE)]`

**写宏内容** `cmd = 0x09`，`size = 7 + chunk`，发送 `[id_hi, id_lo, off(4B), chunk(1B), data...]`
每块 ≤ 64 字节，块间 sleep ≥ 50ms。

**删除宏** `cmd = 0x03`，`size = 2`，发送 `[id_hi, id_lo]`

（A：整套读写删除已实测，含物理按键播放验证）

### 3.6 传感器（cls `0x0B`）

| 操作 | 读 | 写 | size | 参数 |
|---|---|---|---|---|
| 传感器状态 | `0x83` | `0x03` | 2 | `0x0004` |
| 抬升距离 | — | `0x09` | 2 | `0x0004` |
| 校准 | — | `0x09` | 2 | 见文档 |

**全局设置，不带 profile。**（C）

> 注意：DPI 不在这个类里。之前把 DPI 归到 `0x0B` 是错的，扫 `0x0B 0x80..0x8F` 全部无有效载荷。

### 3.7 DPI（cls `0x04`）

**这一类不是 `0x0B`。** `0x04` 是鼠标/传感器参数类，DPI 与 DPI 档位都在这里。

| 操作 | cmd | size | 发送参数 | 响应 |
|---|---|---|---|---|
| 读 DPI | `0x85` | 7 | profile | profile, x_hi, x_lo, y_hi, y_lo, 0, 0 |
| **写 DPI** | `0x05` | 7 | profile, x_hi, x_lo, y_hi, y_lo, 0, 0 | 回显 |
| 读 DPI 档位 | `0x86` | 38 | profile | profile, active_stage, count, 5×(idx, x_hi, x_lo, y_hi, y_lo) |
| 写 DPI 档位 | `0x06` | 38 | profile, active_stage, count, 5×7 字节 | 回显 |

16 位大端。（A：`0x04 0x85 7 4 0 0 0 0 0 0` → `[04, 0A, 8C, 0A, 8C, 00, 00]` = 2700 DPI；
`0x04 0x86 38 4 …` → `[04, 01, 01, 01, 0A, 8C, 0A, 8C, 00, 0]` = 当前第 1 档、共 1 档、2700×2700）

> 本机验证的硬事实：
> 1. **DPI 按 profile 存储**：slot 4 = 2700，slot 3 = 2300，切槽即变。（A）
> 2. **normal mode 下写入静默失败**：返回 OK、不报错、值不变。必须在 driver mode 写。（A）
> 3. **写入不落盘**：写 1800 → 切槽 → 回 2700。（A）
>
> 结论：写 DPI 必须「切 3 → 写 → 读回校验 → 切回原模式」，失败要回滚。

### 3.8 灯效（cls `0x0F`）

**读灯效** `cmd = 0x82`，`size = 2`，发送 `[profile, region]`

```
响应 [profile, region, effect, mode, speed, n, rgb0, rgb1, rgb2, ...]
```
`n` = 颜色数量（单色为 1，逐颗为 11）。

**写灯效** `cmd = 0x02`，`size = 2 + 5 + 3n`，发送 `[profile, region, effect, mode, speed, n, rgb*]`

**读亮度** `cmd = 0x84`，`size = 2`，发送 `[profile, region]` → `[profile, region, pct]`

**写亮度** `cmd = 0x04`，`size = 3`，发送 `[profile, region, pct]`（pct 0..255）

region：

| 值 | 区域 |
|---|---|
| `0x01` | 滚轮 |
| `0x04` | logo |
| `0x0A` | 灯条（11 颗） |
| `0x00` | **写 = 全部三区；读 = 无效** |

effect：

| 值 | 效果 | mode | speed |
|---|---|---|---|
| `1` | 静态色 | — | — |
| `3` | RGB 循环（spectrum） | 1 | — |
| `4` | 波浪（wave） | 1 顺 / 2 逆 | 0 最快 ～ 255 最慢 |
| `8` | 逐颗自定义 | — | — |

（A：1/3/4 已实测写入并断电保留；8 为运行时，不落盘）

---

## 4. 宏指令编码

宏体是一串字节对序列，`0x01/0x02` 表示按下/松开。

| 字节 | 含义 | 示例 |
|---|---|---|
| `0x01, usage` | 按下键 | `01 06` = 按下 c |
| `0x02, usage` | 松开键 | `02 06` = 松开 c |
| `0x08, mask` | 鼠标键按下/松开 | `08 01` 左键按下，`08 00` 全松 |
| `0x0A, delta` | 滚轮 | `0A 01` 上滚一格 |
| `0x11, ms` | 延时（1 字节，≤255ms） | `11 78` = 120ms |
| `0x12, ms_hi, ms_lo` | 延时（2 字节 BE） | `12 01 F4` = 500ms |
| `0x05, hi, lo` | 媒体键按下 | `05 00 CD` = 播放/暂停 |
| `0x06, 00, 00` | 媒体键松开 | |

组合键顺序：先按修饰键 → 按主键 → 松主键 → **逆序**松修饰键。

例：`ctrl+shift+r`
```
01 E0  01 E1  01 15  02 15  02 E1  02 E0
lctrl  lshift  r      r      lshift lctrl
```

（A：`text:p0test` 与 `text:hello` 两次物理播放验证）

**文本只能发 HID usage，即 ASCII。中文无法直接输入。**

---

## 5. Profile 0（Direct Profile）语义

这是本项目虚拟槽方案的基础。协议文档称其为 direct profile：

> 编号为 0 的话，设置立即生效，鼠标断电就会重置，不会保存到板载内存中。

实机六组实验确认（A）：

| 属性 | 实测结果 |
|---|---|
| 读 | 返回内容**恒等于当前激活槽** |
| 写按键 / 灯效 / 亮度 / DPI | 立即生效 |
| 写 Flash | **零写入**（available / recycled 前后完全一致） |
| 对板载槽的影响 | **无**。写 p0 后 slot 1..5 内容不变 |
| 切槽 | p0 被目标槽的 Flash 内容**覆盖** |
| 物理按键触发生效 | **是**（按 p0 绑定的键成功播放宏） |
| 断电 | 重置为激活槽内容 |

**推论**：p0 是"当前生效配置的工作副本"，是纯 RAM 层。
写 p0 = 立即生效 + 零磨损 + 不影响板载槽；但任何切槽操作都会丢弃它。

这正是虚拟槽需要的：切换虚拟槽 = 往 p0 写一套配置，不产生 Flash 写入，用户按物理 Profile 键时硬件自然接管。

---

## 6. Flash 存储模型

```
总容量 90 KiB = 360 页 × 256 B
页状态 available ──写入──> used ──擦除──> recycled ──格式化──> available
```

- 设备**不会自动格式化**。删除数据只把页推进 `recycled`。（A：实验 H）
- `available` **包含** `recycled`。立即可用 = `available - recycled`。（A）
- 实测：写 24B 宏再删除，`available` 少 256（1 页），`recycled` 多 256，
  **立即可用净少 512（2 页）**。
- 设备不提供擦写寿命计数。磨损只能软件自建统计。
- 新建宏只在 `available` 里分配；满了会写失败。此时需格式化 + 重写全部数据（垃圾回收）。

**本项目策略**：不做自动格式化。虚拟槽走 p0 从根上避免写入。

---

## 7. 实现检查清单

写 Rust 实现时逐条对照：

- [ ] `HidD_SetFeature` 前置 `0x00` report id，总长 91
- [ ] CRC = `bytes[2..88]` 异或，写入 `bytes[88]`
- [ ] `transaction_id = 0x1F`
- [ ] `data_size` = 应答数据长度；发送参数区补零到该长度
- [ ] BUSY 重试，写后至少等 250ms 再读
- [ ] 校验响应的 class/id 与请求一致
- [ ] 按键 ID 用第 3.3 节表，不用 90/91
- [ ] 写 DPI 前切 driver mode，写完立刻切回 normal
- [ ] 切换 profile 后 p0 会被覆盖，不要缓存 p0 状态
- [ ] 读灯效必须指定具体 region，不能用 `0x00`
- [ ] 宏 ID 解析用正则，`0x0001` 里的 `x` 不是乘号
- [ ] Flash 立即可用 = `available - recycled`

---

## 8. 待补验证（C 级条目）

已被实测消掉的条目不再列出，剩下这些仍未确认：

| 项 | 说明 |
|---|---|
| Flash 格式化命令 | 已知存在，未记录命令码；本项目不使用（避免在用户设备上执行破坏性操作） |

> 已消项：滚轮模式 / 加速 / 智能卷动（见 §3.2、§8.1），以及宏 hold / toggle 的
> **物理触发**（见 §8.1「`fn=4 / fn=5` 物理触发」行，人工按键验收）。
> §8 至此只剩一条**不应自动化**的破坏性操作。

### 8.1 已消项（留档，避免重复怀疑）

| 项 | 结论 | 证据 |
|---|---|---|
| DPI 命令原始码 | `cls 0x04 / cmd 0x85 读、0x05 写`，`>BHHxx` | A：raw 实测 2700 |
| 回报率命令码 | `cls 0x00 / cmd 0x8E 读、0x0E 写`，存 delay_ms | A：raw 实测 `[04,01]` |
| DPI 档位 | `cls 0x04 / cmd 0x86`，38 字节 | A：raw 实测 1 档 2700 |
| `fn=0x0E` 是什么 | 扩展鼠标动作，`data=[mouse_action]`，长度 1。本机滚轮左右倾即此 | A：扫描 + 写回验证 |
| 按键名反查 | A–Z、0–9、小键盘、F1–F24 已全表可读 | A：`usage::label` |
| **逐颗 LED 可达帧率** | **142.9 fps（7.00 ms/帧，11 颗灯，最慢 7.01 ms 极稳）** | A：`razerctl bench-led 60` |
| **`fn=0x04/0x05` 能否存住** | 能。写 `macrohold` → 读回 `fn=4 data=[01,01,01]`；写 `macrotoggle` → `fn=5` 同样一致 | A：`bind` + `binding` 往返 |
| **`fn=4 / fn=5` 物理触发** | **按设计生效**。`fn=4` 按住 DPI 键期间循环播放（`h=6`），松开即停（后续整段 toggle 输出 0 个 `h`）；`fn=5` 点一次前进键后自行循环（`t=36`），脚本 `bind off` 强制掐断；另一次人工观测 `t=40` 后第二次点击停下。GUI 已按此开放 `edit-kind` 9 / 10 两类编辑，真实点击写入与读回一致 | A：`tests/holdtest.py --watch` 两轮实机按键，p0 绑定 `fn=4/5/3` 读回一致，采集串 `'chhhhhhttt…'`，对照组 `c=1`；收尾删宏 + 切槽还原逐项校验；GUI 端到端写入 4 号槽后还原 |
| **删除宏后的 Flash 页** | 宏删除后 `macros 0`，但 `recycled` +768 B（3 页）、`available` 86272→85504 B；固件自行回收，不做格式化 | A：验收前后 `razerctl info` 对比 |
| **覆写已存在的宏 ID** | **不行**。`MACRO_SET_SIZE` 直接回 `Fail 0x06/0x08`，必须先 `MACRO_DELETE(0x03)` 再写。`core::macro_write` 已封装「备份 → 删 → 写 → 失败还原」 | A：`macro add` 覆写失败 / 先删后写成功，实测 13B→10B→13B |
| **CUSTOM 灯效怎么读** | `GET_EFFECT` 应答 `data_size=0x0c`（12 字节），字段 `n` 写 11 但只给得出 2 颗颜色。`Lighting::parse` 按实际字节数取，不能要求 `6+n×3` | A：`raw 0x0f 0x82 2 4 0x0a` → `[00 0a 08 00 00 0b ff 88 00 ff e9 00]` |
| **解析失败 ≠ HID 故障** | 旧代码把 `ok_or(Error::Transfer{code:0})` 当兜底，报 `HID transfer failed (win32 error 0)`，误导排查方向。现分 `Error::Parse` / `Error::Io` | A：修复后 CUSTOM 状态可读回「逐灯预览 · 设备回报 2 颗颜色」 |
| **软件动画实测** | 60 帧按 30 fps 发送，2.16 s 跑完并自动还原；不做常驻动画（持续占 CPU 与低占用目标冲突） | A：`led preview animate 60` |
| **预览备份不能被覆盖** | 连续点预览、或在 CUSTOM 状态下跑 `bench-led`，若把当前帧当成「板载效果」存下来，还原时会把一颗彩虹色写成永久静态色（实测踩到 `#FF8800`）。只在 `effect != CUSTOM` 时写备份，还原目标始终取备份里的板载值 | A：rainbow → gradient → bench-led → restore 全链路回到 `#0000FF`，`selftest` 静态色项同步为 `#0000FF` |
| **滚轮三项读写与落盘** | `0x94/0x14`、`0x96/0x16`、`0x97/0x17` 全部可写可读回，应答 `[profile, value]`；normal mode 即可写，无需 driver mode；按槽独立、跨切槽保持、p0 可独立寻址。回报率是设备级、不带 profile | A：5 槽 + p0 逐条探测；写相反值读回一致；`slot use 3 → 4` 后保持；写 p0 不影响 4 号槽；全部还原后 5 槽仍是 `0/0/1` |

**关于 142.9 fps 的解读**：单帧耗时几乎恒等于 7 ms，说明瓶颈不是协议解析而是
HID 传输本身的节奏，不会因为灯珠更多而线性变差。结论：软件动画技术上完全可行，
但每帧要阻塞 7 ms——30 fps 就是单核约 21% 的占用。所以本项目把「软件动画」做成
可选且只在界面里预览，长期效果一律用板载光谱/波浪（固件生成，零占用）。
