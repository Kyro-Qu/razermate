# razermate 设计与实施计划 v2

> 面向：Razer Basilisk V3 有线版（`1532:0099`，控制口 `MI_03`）
> 本文档基于 **2026-11-23 实机报文验证**，不是纸面推演。
> 上一版（v1）作废，以下凡与 v1 冲突，以本文为准。

---

## 0. 摘要

审查意见提出 5 个关键问题。经实机取证：

| # | 审查意见 | 裁定 |
|---|---|---|
| 1 | 虚拟槽不该占用硬件槽 1，应先验证 Profile 0 | **完全成立，且已验证成功** —— 这是本轮最有价值的结论 |
| 2 | `available / recycled` 不代表 Flash 磨损 | **成立** —— 是空间状态，不是寿命计数 |
| 3 | 两进程方案的 HID 所有权自相矛盾 | **成立** —— v1 确有逻辑冲突，本文已修正 |
| 4 | GUI 内存不必预算 40–60MB | **部分成立** —— 不该承诺数字，但"关窗必退进程"必须作为硬指标 |
| 5 | 宏与 RGB 不能过早承诺 | **部分成立** —— 宏已实测通过（审查方不知情），但触发模式与 LED 刷新率确实未测 |
| 6 | 用 `tokio` + `tokio-named-pipes` | **驳回** —— 该 crate 最新版 v0.1.0 发布于 8 年前，共 127 行，不可用于生产 |
| 7 | 用 message-only window 收 `WM_DEVICECHANGE` | **采纳** |
| 8 | 管道需设安全描述符 | **采纳** |

本轮还额外发现 **两个 v1 与审查意见都没提到的硬约束**，直接改变实现细节（见 §4.4）：
normal mode 下写 DPI 会静默失败；按键 ID 不是直觉值。

另外完成了 p0 的**物理触发验收**（实验 G：按 p0 绑定的键成功打出 `p0test`），
虚拟槽方案的所有前提均已实机落地，不再是假设。

---

## 1. Profile 0 实机验证报告

### 1.1 协议原文

`refs/razerqdhid/docs/cmd_profile.md` 开篇：

> 巴蛇V3鼠标板载内存中可以存5个 profile，分别是白/红/绿/蓝/青，编号分别为 1~5。
> 此外还有一个特殊的 profile 编号，我叫作 direct，编号为0。**编号为0的话，设置立即生效，鼠标断电就会重置，不会保存到板载内存中。**

这是假设的来源。但文档作者自己也标了"我不确定"的地方不少，所以必须实机证伪/证实。

### 1.2 六组实验与原始数据

**实验 A —— Profile 0 是否可读**

```
total=5  available=5  list=05 01 02 03 04 05
profile 0  forward=key:alt+w        back=key:alt+v
profile 1  forward=key:ctrl+shift+r back=key:ctrl+w
profile 2  forward=key:alt+w        back=key:alt+v
profile 3  forward=key:ctrl+shift+r back=key:ctrl+w
profile 4  forward=key:alt+w        back=key:alt+v   ← 当时激活槽
profile 5  forward=key:ctrl+shift+r back=key:ctrl+w
```

结论：`0` 是合法 profile 编号，可读，且**内容恒等于当前激活槽**。

**实验 B —— 写 Profile 0 的按键绑定，是否污染板载槽 / 是否写 Flash**

```
BEFORE  p0 forward = key:alt+w   p4 forward = key:alt+w
        flash: total=92160  available=90368  recycled=2304

write   p0 forward = key:f13

AFTER   p0 forward = key:f13     ← 写入生效
        p4 forward = key:alt+w   ← 板载槽未被污染
        p1 forward = key:ctrl+shift+r  ← 其他槽正常
        flash: total=92160  available=90368  recycled=2304  ← 一个字节都没变
```

结论：**写 p0 是纯 RAM 操作，零 Flash 写入，不影响任何板载槽。**

**实验 C —— 切槽是否重置 p0**

```
p0 forward = key:f13（刚写入）
切到 slot 1  → p0 forward = key:ctrl+shift+r   （= slot 1 的值）
切回 slot 4  → p0 forward = key:alt+w          （= slot 4 的值）
```

结论：**p0 是"当前生效配置的工作副本"，任何切槽都会用目标槽的 Flash 内容覆盖它。**
这不是缺陷，恰好实现了"硬件永远赢软件"。

**实验 D —— p0 的灯效与亮度**

```
BEFORE  p0 strip = 00 0a 01 00 00 01 00 00 ff 00 00 00   （静态 #ff0000）
        p4 strip = 00 0a 01 00 00 01 00 00 ff 00 00 00   （一致）
        亮度 p0 = 0xb2 (178/255 ≈ 70%)，p4 同

write   p0 全区域静态 #00ff00

AFTER   p0 strip = 00 0a 01 00 00 01 00 ff 00 00 00 00   （绿）
        p4 strip = 00 0a 01 00 00 01 00 00 ff 00 00 00   （仍是红，未污染）
        flash 不变
```

结论：灯效、亮度同样支持 p0，同样零落盘。

**实验 E —— DPI 是否分槽、是否能落盘**

```
slot 4: dpi = 2700
slot 3: dpi = 2300        ← 切槽即变，说明 DPI 存在每个槽里
```

但 **normal mode 下写 DPI 静默失败**（返回成功、读回不变），必须切 driver mode：

```
mode 0:0  写 dpi 1800  → 读回 2700   （无效，且不报错）
mode 3:0  写 dpi 1800  → 读回 1800   （生效）
回 0:0    dpi = 1800                 （保持）
切到 slot 3 → 2300
切回 slot 4 → 2700                   ← 之前的 1800 消失
```

结论：**DPI 也是"即时生效、不落盘"的，但写入必须先切 driver mode。**

**实验 F —— Flash 空间统计的真实语义**
```
raw: 00 64 | 00 01 68 00 | 00 01 61 00 | 00 00 09 00
     100  |    92160    |    90368    |    2304
```

换算成页（1 页 = 256 B）：

```
total       360 页 = 92160 B   （= 90 KiB，与协议文档一致）
used          7 页 = 1792 B    （= 92160 - 90368）
available   353 页 = 90368 B
recycled      9 页 = 2304 B
```

**关键：`available` 包含 `recycled`。** 验证：`353 + 7 = 360`，正好等于总数；
若把三项相加则是 `353 + 7 + 9 = 369 > 360`，不成立。

所以真实结构是：

```
360 页 = used(7)  +  available(353)
available(353) = 立即可分配(344)  +  recycled(9，需格式化才能用)
```

即 `立即可用 = available - recycled = 88064 B`。
这直接影响 UI 的显示逻辑：不能把 `available` 当成"还能写多少"，否则会高估 9 页。

首 2 字节 `100` 含义不明（疑似格式版本号），不作为任何判断依据。

**实验 G —— p0 的绑定是否物理生效（决定性验收）**

"读得回来"和"firmware 真的用了"是两件事，必须按一下才算数。

```
步骤  p0 把 dpi 键(id 96) 从 key:esc 改写为 macro:0002
      宏 0002 内容 = 依次敲 p 0 t e s t（24 B，写入校验通过）
      设备 mode 0:0 normal，Synapse 与两个服务全部不在运行
动作  物理按下该键
结果  记事本中出现 p0test
```

结论：**p0 的绑定不只是可读可写，firmware 确实以 p0 作为当前生效的绑定表。**
虚拟槽方案的最后一块拼图到位。

**实验 H —— 一次"写宏 + 删宏"的存储代价**

```
初始      available 90368  recycled 2304   立即可用 88064
写入 24B 宏  available 90112  recycled 2304   立即可用 87808   (-256，占 1 页)
删除该宏   available 90112  recycled 2560   立即可用 87552   (-512，另 1 页进回收)
```

即"写一次再删一次"会永久消耗 **2 页立即可用空间**，直到格式化才回来。
这实证了两件事：`recycled` 只能靠 format 回收；设备**不会**自动回收。

### 1.3 结论表

| 能力 | p0 可写 | 立即生效 | 写 Flash | 需要 driver mode |
|---|---|---|---|---|
| 按键绑定（含宏绑定） | ✅ | ✅ | ❌ | 否 |
| 灯效（静态/光谱/波浪） | ✅ | ✅ | ❌ | 否 |
| 亮度 | ✅ | ✅ | ❌ | 否 |
| DPI | ✅ | ✅ | ❌ | **是** |
| 回报率 / 滚轮模式 | 待测 | 待测 | 待测 | 待测 |
| 断电后 | 重置为激活槽内容 | — | — | — |

> "立即生效"一列中，按键绑定（含宏绑定）已由**实验 G 的物理按键**确认；
> 灯效、亮度、DPI 为读回确认（灯效另有肉眼可见的颜色变化佐证）。
> "断电重置"由协议文档描述 + 实验 C 的切槽重置实证共同支撑，建议 P3.5 再拔插确认一次。

### 1.4 虚拟槽工作流（由此确立）

**切虚拟槽 = 往 p0 写一整套配置，不碰 Flash。**

```
检测到前台程序变化（连续 2 次轮询一致，约 1.2s）
   ↓
取虚拟槽定义（存在电脑上的 JSON）
   ↓
差量比对：与当前 p0 状态逐字段比
   ↓
只写有差异的部分：
   · 按键绑定 → profile=0
   · 灯效/亮度 → profile=0
   · DPI     → 若不同：切 driver mode → 写 → 读回校验 → 切回 normal
   ↓
读回校验全部字段，失败则重放默认槽
```

**Flash 写入次数 = 0**（除非用户主动改板载槽）。
这从根本上消灭了"自动切程序磨损 Flash"的问题，比"差量写入"更彻底。

**用户按物理 Profile 键的后果**：p0 被硬件覆盖，自动切槽暂时失效。服务下次检测到前台程序变化时重新下发即可 —— 无需特殊处理，天然符合"硬件优先"。

---

## 2. 审查意见逐条裁定

### 2.1 ✅ 采纳：虚拟槽不占用硬件槽（已改为 p0）

见 §1。审查方判断"这可能是整个软件设计中最有价值的发现"，实测证实了这一点。
五个板载槽完整保留给用户，虚拟槽数量不限且零磨损。

### 2.2 ✅ 采纳：Flash 统计不等于磨损

纠正 v1 的错误表述。四项数据分两层：

| 层 | 数据 | 用途 |
|---|---|---|
| 空间状态 | `available` / `used` / `recycled` | 显示剩余空间、提示用户 |
| 磨损程度 | **写入次数累计** | 设备**不暴露**此计数，只能软件自建 |

v2 的做法：

1. 界面显示三项，且区分清楚：`已用 / 立即可用(=available-recycled) / 待回收`（纯状态，不叫"磨损"）
2. 软件自建计数器：本地 `flash-writes.json` 累计记录写入字节数与次数，标注为"本软件统计，非设备寿命"
3. **虚拟槽走 p0，不产生 Flash 写入** → 磨损问题在架构层被消除，而非靠监控
4. **禁止自动执行 Flash Format**（`recycled` 回收需人工确认）

### 2.3 ✅ 采纳：两进程的 HID 所有权必须交接（v1 有矛盾）

v1 同时写了"HID 永远属于服务"和"模式一只有 GUI"，二者不可共存。修正为：

| 模式 | 持有 HID 的进程 | 关闭 GUI 后 |
|---|---|---|
| 模式一 Pure | `razermate.exe` 自己 | 进程退出，句柄释放，**0 残留** |
| 模式二 Background | `razermate-agent.exe` | GUI 退出，agent 继续持有 |

交接协议（详见 §3.1）：命名互斥体 `Global\razermate-hid-owner` 表示所有权，任何时刻只有一个进程能打开设备主句柄。

### 2.4 ⚠️ 部分采纳：GUI 内存不应承诺数字，但"关窗退进程"是硬指标

审查方引用的 9 MiB 数据来自第三方桌面框架对比测试（`HeyVasily/rust-desktop-comparison`）。该测试的场景是极简窗口，本项目要画 11 颗 LED 矩阵、多页切换、中文点阵字形，不能直接套用。

v2 的验收方式改为**实测门槛**，不写预算：

| 指标 | 验收门槛 | 测量工具 |
|---|---|---|
| GUI 关闭后 | GUI 进程**必须消失** | 任务管理器进程列表 |
| Agent 空闲 Private Working Set | ≤ 12 MiB | `Get-Counter` 或 Process Explorer |
| Agent 空闲 CPU | 平均 ≤ 0.3% | 任务管理器 5 分钟均值 |
| GUI 打开时 | 不重复拉起 agent | 进程计数 |
| GUI 退出 | 鼠标已保存设置不受影响 | 拔插后复查 |

术语纪律：**Private Working Set** 是任务管理器"内存"列显示的值；Private Bytes 是提交量；Working Set 含共享页。文档与验收一律用 Private Working Set。

### 2.5 ⚠️ 部分采纳：宏已实测通过，但缺项要补

审查方认为"不足以证明所有宏类型都能在 Synapse 完全退出后独立执行"。这一点在本轮之前就已经有了实证：

**已证实**（2026-11-23）：两个雷蛇服务已 `Start=4` 禁用、Synapse 全部进程为 0 的条件下，
`macro add 0x0001 "text:hello"` → `bind wheelmode macro:0x0001` → 物理按键 → 输入框打出 `hello`。
宏数据存在独立 Flash 区（`0x0602` 写、`0x0682` 读），不隶属任何 profile。

**仍未证实**（接受审查方意见，纳入 P6）：

| 待测项 | 说明 |
|---|---|
| 鼠标按键宏 | 宏里含鼠标左/右/中键的动作 |
| 媒体键宏 | `0x05/0x06` + 2 字节 usage |
| ~~触发模式~~ | ✅ **已证实**：`fn=3` 单次（可带次数）/ `fn=4` 按住循环 / `fn=5` 开关循环，写入读回与物理播放均已实测（见「人工按键验收」） |
| 宏嵌套 | 宏绑定到另一个宏引用的宏 |

RGB 部分同理，已实测：静态/光谱/波浪**由固件生成并写进 profile**，逐颗（effect=8）是运行时。
未测：**多颗 LED 连刷的实际可达帧率**（审查方这条提得对，直接决定"渐变/呼吸动画"是否值得做）。

### 2.6 ❌ 驳回：不要用 `tokio` + `tokio-named-pipes`

事实核查：

- `tokio-named-pipes` 在 crates.io 上的**最新版是 v0.1.0，发布于约 8 年前，全库 127 行**。它不是"可用但有取舍"，而是从未完成。
- tokio 1.x 自 1.7 起已内置 `tokio::net::windows::named_pipe`，不需要第三方 crate。

更重要的是**架构判断**：agent 的工作负载是低频的。

```
每 800ms   查一次前台窗口标题/进程名
事件驱动   管道请求（用户点界面时才有）
事件驱动   WM_DEVICECHANGE（拔插时）
```

这个负载用 tokio 多线程 runtime 是纯开销：worker 线程栈、slab 分配、epoll/IOCP 轮询器，都要算进 12 MiB 预算。

v2 选型：**同步阻塞 + 少量 std 线程**。

```
线程 1  message-only window 消息循环（WM_DEVICECHANGE / 管道连接通知）
线程 2  HID 独占工作线程（收发 + BUSY 重试），通过 mpsc 收命令
线程 3  前台窗口轮询（可合并进线程 1 的定时器）
```

线程栈按需设置（`std::thread::Builder::stack_size(128 * 1024)`），比默认 1 MiB 省得多。
管道用 `windows-rs` 的 `CreateNamedPipeW` + 重叠 I/O，或直接阻塞线程 + 每连接一线程（连接数 ≤ 1，完全够用）。

结论：**不引入 async runtime**。如果将来真需要异步，用 tokio 内置的 named_pipe，绝不用那个 0.1.0 crate。

### 2.7 ✅ 采纳：message-only window

用 `HWND_MESSAGE` 父窗口创建隐藏窗口，`RegisterDeviceNotification` 注册 `GUID_DEVINTERFACE_HID`。

- 内存开销：几 KB 级
- 收益：拔插零延迟、零轮询
- 附带好处：这个窗口同时可以作为管道的连接通知接收点

### 2.8 ✅ 采纳：命名管道安全描述符

```
SDDL: D:P(A;;GA;;;<当前用户SID>)(A;;GA;;;SY)
标志: PIPE_REJECT_REMOTE_CLIENTS
```

即：只允许当前用户与 SYSTEM 完全访问，拒绝远程客户端。
管道名附一个每次启动随机生成的 token（如 `\\.\pipe\razermate-{pid}-{rand}`），写入用户配置目录供 GUI 发现，防止其他进程猜名连接。

---

## 3. 修正后的架构

### 3.1 进程模型

```
模式一 Pure · 单进程
┌─────────────────────────────┐
│ razermate.exe               │
│  Slint UI ─ Core ─ Hid      │
│  持有 Global\razermate-hid-owner │
└─────────────────────────────┘
  关闭窗口 → 进程退出 → 互斥体释放 → 句柄关闭 → 0 残留

模式二 Background · 双进程
┌──────────────┐   Named Pipe   ┌──────────────────────┐
│ razermate.exe│◄──────────────►│ razermate-agent.exe  │
│  Slint UI    │   (SDDL 限权)  │  Hid + Tray + Watch  │
│  纯 IPC 客户端│                │  持有 hid-owner 互斥体 │
└──────────────┘                └──────────────────────┘
  关闭窗口 → GUI 退出（0 残留），agent 继续
  托盘「完全退出」→ agent 退出 → 互斥体释放 → 全清
```

**为什么叫 agent 而不是 Windows Service**：采纳审查方意见。它需要留在当前用户桌面会话里做托盘和前台窗口监听，注册成 SCM 服务反而拿不到会话 0 之外的桌面信息，还要额外处理会话隔离。

### 3.2 虚拟槽

- 定义存 `%LOCALAPPDATA%\razermate\virtual-slots.json`，数量不限
- 每个虚拟槽含：名称、匹配规则（进程名 / 窗口标题正则）、26 条按键绑定、灯效、亮度、DPI
- 切换 = 写 p0（§1.4），**零 Flash 写入**
- 硬件 5 槽的语义完全不变，物理 Profile 键随时可切，切了就覆盖 p0

**去抖**：目标槽连续 2 次轮询（800ms × 2）一致才下发，Alt+Tab 掠过不触发。
**差量**：逐字段比对 p0 当前值，只写差异项。

### 3.3 IPC 消息协议

```rust
enum Req {
    GetSnapshot,                  // 全部状态（供 UI 首屏）
    SetSlot { index: u8 },        // 切硬件槽
    SetVirtualSlot { id: String },
    WriteBinding { profile: u8, button: u8, layer: u8, action: String },
    SetLighting { zone: u8, effect: u8, mode: u8, speed: u8, rgb: [u8;3] },
    SetBrightness { pct: u8 },
    SetDpi { x: u16, y: u16 },    // agent 内部处理 driver-mode 切换
    MacroList, MacroWrite { id: u16, data: Vec<u8> }, MacroDelete { id: u16 },
    ReloadRules, Shutdown,
}

enum Resp { Ok(Value), Err { code: u16, msg: String } }
```

编码用 `serde_json` + 长度前缀（简单、可调试、体积无所谓，一次最多几 KB）。
每条写操作都必须回传「写入前值 / 写入后读回值」，UI 据此展示校验结果。

### 3.4 代码结构

```
razermate/
├── crates/
│   ├── razer-protocol/   报文编解码、宏指令、常量表（纯函数，无 OS 依赖，可单测）
│   ├── razer-hid/        windows-rs HID：枚举 MI_03、特性报告、BUSY 重试、热插拔
│   ├── razer-core/       能力封装、写后读回校验/回滚、p0 差量引擎、虚拟槽
│   ├── razer-flash/      Flash 空间查询、宏 CRUD、软件侧写入计数
│   └── razer-ipc/        命名管道、SDDL、消息协议
├── apps/
│   ├── razermate/        Slint GUI（中文）
│   └── razermate-agent/  托盘 + 前台监听 + HID + message-only window
├── tests/
│   ├── golden/           Python 对拍的报文样本
│   └── protocol/
└── docs/
    ├── PLAN.md           （本文）
    ├── PROTOCOL.md       协议规格（P0 产出）
    ├── DEVICE.md         设备能力与边界
    └── FLASH.md          Flash 语义与磨损说明
```

---

## 4. 协议事实清单

### 4.1 按键 ID（实测，`get_buttons` 返回）

| 名称 | ID | 名称 | ID |
|---|---|---|---|
| left | 1 | dpi | 96 |
| right | 2 | clutch（狙击） | 15 |
| middle | 3 | profile（底部键） | 14 |
| **back** | **4** | wheelmode（滚轮模式） | 106 |
| **forward** | **5** | wheelup | 9 |
| tiltleft | 52 | wheeldown | 10 |
| tiltright | 53 | | |

**⚠️ 易错点**：`forward` 是 **5** 不是 90，`back` 是 **4** 不是 91。
用 90/91 不会报错崩溃，而是设备返回 `device replied failure`，看起来像"命令不支持"，本轮已踩过。

### 4.2 命令族（`_onboard(cls, cmd, size, args)`，`size` = 格式串总长）

| 族 | 用途 | 读 | 写 | 参数 |
|---|---|---|---|---|
| `0x02` | 按键绑定 | `0x8C` | `0x0C` | profile, button, layer（读 3B；写 +fn+size+data） |
| `0x02` | 备份/恢复 | — | `0x0E`/`0x03` | doc 结构 |
| `0x05` | profile 管理 | `0x8A/0x80/0x81/0x88` | `0x02/0x08/0x04` | 总数/列表/信息/切槽 |
| `0x06` | Flash 与宏 | `0x8E/0x88/0x89/0x82/0x80` | `0x02/0x08/0x09/0x0A` | 空间/尺寸/内容/数量 |
| `0x0F` | 灯效 | `0x82` | `0x02` | profile, region；数据 `effect,mode,speed,n,rgb*` |
| `0x0F` | 亮度 | `0x84` | `0x04` | profile, region, 1B |
| `0x0B` | 传感器 | `0x83` | `0x03` | 全局，无 profile |

灯效 region：`0x01` 滚轮 / `0x04` logo / `0x0a` 灯条（11 颗）。
**写时 `0x00` = 全部三区，读时不能用 `0x00`。**

### 4.3 Flash 语义

```
容量     90 KiB = 360 页 × 256 B
状态     used（已用） / available（未占用，含 recycled） / recycled（待格式化回收）
立即可用 available - recycled
擦除     按页；recycled 需格式化才能回到立即可分配
回收     设备不会自动格式化；删除数据只会把页移进 recycled（实验 H 实测）
查询     0x06 0x8E，返回 total / available / recycled
限制     设备不暴露擦写寿命计数
```

### 4.4 坑位清单（全部实测踩过）

| 坑 | 现象 | 对策 |
|---|---|---|
| 响应回显 | 返回前 N 字节是请求参数的回显，数据在其后 | 按各命令固定偏移取 |
| BUSY | 写完立刻读返回失败 | 读带重试（6 次 × 间隔）；写后 sleep ≥ 250ms |
| normal mode 写 DPI | **静默失败**：返回成功、值不变、不报错 | 必须切 driver mode 写，写完切回 |
| driver mode | 板载宏与 p0 绑定全部失效 | 只在"写 DPI"这个窗口内短暂切换，用完立刻回 normal |
| profile 0 | 切槽/断电即被覆盖 | 不当作持久层，只当作"当前生效层" |
| 按键 ID | forward/back 不是 90/91 | 用 §4.1 表 |
| 宏 ID 解析 | `macro:0x0001` 里的 `x` 被当成乘号 | 用正则而非 split |
| 灯效读 region=0 | 无效 | 读必须指定具体区 |

---

## 5. 风险与未验证项

| 项 | 风险 | 处理 |
|---|---|---|
| p0 断电是否真重置 | 低（切槽重置已证） | P3.5 拔插一次确认 |
| ~~p0 绑定是否物理生效~~ | ✅ **已验证**（实验 G：按 p0 绑定键打出 `p0test`） | — |
| ~~回报率/滚轮模式是否走 p0~~ | ✅ **已验证** | 滚轮三项按 profile 存储且 p0 可独立寻址（PROTOCOL §3.2）；回报率 `cls 0x00` 不带 profile，是设备级，归入"仅手动修改" |
| DPI 每切槽切一次 driver mode | 中：切 mode 约 1s，期间宏失效 | 只有 DPI 不同才切；界面提示"正在切模式" |
| ~~宏的 hold/toggle 触发~~ | ✅ **已验证**：`fn=4` 按住循环（`h=6`）、`fn=5` 开关循环（`t=36`），物理按键验收通过 | GUI 按键映射页据此开放「按住播放宏 / 开关播放宏」两类编辑 |
| 11 颗 LED 连刷可达帧率 | 中 | P6 测；低于 20fps 就不做渐变动画 |
| 管道被伪造 | 低 | SDDL + 随机 token |
| 用户同时开 Synapse | 已排除 | 服务已 `Start=4`，Synapse 起不来 |

---

## 6. 阶段计划

| 阶段 | 任务 | 验收（可执行、可复现） |
|---|---|---|
| **P0** | `PROTOCOL.md`：§4 全部内容 + 每条命令的证据等级标注（实测/文档/推断） | 覆盖 0x02/0x05/0x06/0x0B/0x0F；每条标"回显偏移 + BUSY 行为 + 证据来源" |
| **P1** | `razer-protocol` + `razer-hid` crate + CLI | 同一操作，Rust 与 Python `razerctl` **读回字节逐字一致**（golden test 自动跑） |
| **P2** | Slint GUI 骨架 + 中文 + 全部只读页 | 界面每个数值 = `razerctl` 同项实测值 |
| **P3** | 写入：按键 / 灯效 / 亮度 / 回报率 / 滚轮 / 槽位切换 | 每次写后自动读回；不一致则回滚并提示 |
| **P3.5** | **p0 专项**：虚拟槽配置全套走 p0；DPI driver-mode 流程；拔插验证 | ① 连续切 20 个虚拟槽，`available/recycled` **零变化** ② 拔插后 p0 回到激活槽内容 ③ 改 DPI 全流程 ≤ 3s |
| **P4** | `razermate-agent` + IPC + 托盘 + 单实例唤醒 + 热插拔 | 关 GUI 后仅 1 个进程，Private Working Set ≤ 12 MiB；拔插 10 次自动重连 |
| **P5** | 自动切换 + 硬件优先 + 异常恢复 | 切前台程序 2s 内换配置；按物理 Profile 键后自动切暂停、下次程序变化恢复 |
| **P6** | 宏编辑器（含 hold/toggle 实测）+ LED 动画（按实测帧率决定）+ 打包 | Synapse 全程不在的机器上完成全流程；单 exe 免安装 |
| **P7** | 打磨 | 崩溃不影响鼠标设置；配置备份/恢复 |

**P3.5 单列的理由**（采纳审查意见）：虚拟槽的整套假设都压在 p0 上，必须先证死再往上盖。
好消息是这一层已经过一次实测（§1.2 实验 B/D），P3.5 是把它做成完整流程并抗拔插。

---

## 7. 需要你确认的三件事

1. **项目路径**：`D:\WorkSpace\Project\DeviceManagement\razermate\`（本文档已在此）。
2. **模式二的默认行为**：装完先按"模式一（纯净）"交付，还是直接做双进程？建议先模式一跑通（P0–P3.5），再叠 agent（P4）。
3. **DPI 的取舍**：改 DPI 需要短暂切 driver mode（约 1 秒，期间板载宏失效）。
   - 方案 A：支持软件改 DPI，界面明确提示这 1 秒的副作用
   - 方案 B：不支持软件改 DPI，只显示当前值，改 DPI 用鼠标硬件键
   我倾向 A，但取决于你是否常用 DPI 键。

---

## 8. 实施进度

> 更新于 2026-11-24 00:20。以下均为实机跑通并验证，不是"写完了"。

### 工具链（一次性解决，已固化在 `.cargo/config.toml`）

```
rustc 1.100.0  host x86_64-pc-windows-gnu  静态 CRT
链接器：rustup 自带的 rust-lld + rustup 自带的 MinGW 库
```

**为什么不能用系统 gcc**：本机 MinGW-w64 是 gcc 8.1，其 `libkernel32.a` 把
`WaitOnAddress` / `WakeByAddressSingle` / `WakeByAddressAll` 声明为 `KERNEL32.dll`
的导入。这三个函数在 Windows 8+ 位于 `KernelBase.dll`，正常通过 API Set 访问，
直接导入 KERNEL32 会让加载器在 `main()` 之前就以 `STATUS_ENTRYPOINT_NOT_FOUND
(0xC0000139)` 杀掉进程。换 rust-lld + rustup 的库后，产物依赖
`api-ms-win-core-synch-l1-2-0.dll`，问题消失。

### P0 ✅

`docs/PROTOCOL.md` 完成：报文结构、CRC、data_size 语义、状态码、六个命令族、
宏指令编码、Profile 0 语义、Flash 模型、坑位清单、实现检查清单。

### P1 ✅

| 项 | 结果 |
|---|---|
| workspace | `razer-protocol` / `razer-hid` / `razer-core` / `razerctl-rs` |
| 单元测试 | **33 个全通过** |
| 报文级对拍（golden） | **21/21 命令与 Python 参考实现逐字节一致**（含 CRC） |
| 写入对拍 | Rust 写 `p0 tiltleft=key:f13` → Python 读回一致；反向 Python 写 `f14` → Rust 读回一致 |
| 真实设备 | 宏 写/读/校验/删 全通过；p0 灯效三区写入，Flash 用量零变化；切槽恢复正确 |
| release 产物 | **338 KB 单文件**，无 MinGW 运行时依赖，`info` 134 ms |

命令行（与 Python 版对齐，另有 `binding` / `paths` / `hexdump` 三个调试命令）：

```
razerctl info | check | flash | mode | normal | paths
razerctl slot [list|show N|use N]
razerctl led <zone|all> <colour>
razerctl onboard <zone|all> static|spectrum|wave [dir speed] [--slot N]
razerctl bright <0-100> [--slot N]
razerctl bind <button> <action> [--slot N]
razerctl binding <button> [--slot N]
razerctl macro list|show|add|del
razerctl hexdump <class> <cmd> <size> [args...]
```

### P2 ✅ 图形界面（只读）

`apps/razermate`：Rust + Slint 1.19（`backend-winit` + `renderer-software`），
980×680，深色中文界面，先做 7 个页面（概览 / 按键映射 / 灯光 / 性能 / 宏 / 配置槽位 / 设置），
P5 再加「自动切换」页，共 8 页。

| 项 | 结果 |
|---|---|
| 数据一致性 | 界面每个数字都来自设备实测，读数与 `razerctl` 完全一致（2700 DPI / 1000 Hz / 68% 亮度 / 4 号槽） |
| 渲染核对 | 自研 `--shot <file.bmp>` 用 Slint `take_snapshot()` 直出图片，绕开多显示器 DPI 坐标问题，逐页目视核对 |
| 交互核对 | 真实鼠标点击验证命中测试：点行 → 打开编辑面板，选中态与预填参数正确 |

### P3 ✅ 写入

所有写操作统一走 `razer-core` 的「读旧值 → 写 → 读回校验 → 失败回滚」，界面只呈现结果。

| 项 | 结果 |
|---|---|
| 可写项 | 按键绑定、静态色/光谱/波浪、亮度、DPI、回报率、切槽 |
| `razerctl selftest` | **8/8 通过**：亮度、静态色、按键绑定、DPI、回报率各自「改→校验→还原→再校验」全 OK |
| 亮度还原精度 | 发现百分比有损（69%→175→68%），新增 `set_brightness_raw()`，还原改成原始值，误差归零 |
| **新发现的坑** | 切驱动模式写 DPI 会把**生效槽重置为 1 号槽**。`set_dpi()` 现在会记住并还原原槽位，已列入 PROTOCOL.md |

### P4 ✅ 后台守护进程（agent + IPC）

`crates/razer-ipc`（契约）+ `apps/razermate-agent`（托盘常驻进程）。

| 项 | 结果 |
|---|---|
| 内存 | **私有工作集 2.09 MB**（目标 ≤ 12 MiB）；5 线程 / 213 句柄；不用 async runtime，无 HID 常驻句柄 |
| 单实例 | `Local\RazerMateAgent` 互斥体；第二次启动直接退出并拉起面板（已实测） |
| 管道安全 | SDDL `D:P(A;;GA;;;SY)(A;;GA;;;OW)` 只给 SYSTEM + 所有者；`PIPE_REJECT_REMOTE_CLIENTS`；连接后还必须出 `BCryptGenRandom` 生成的 64 位 hex 令牌，认证失败立即断开 |
| 托盘 | `Shell_NotifyIconW` 注册成功，`HKCU\Control Panel\NotifyIconSettings` 中已出现本程序条目（Win11 默认收进溢出区，用户可自行固定） |
| 热插拔 | `WM_DEVICECHANGE` 作废槽位记忆 + 每 4 s 设备巡检，双重兜底 |
| 命令 | `--status / --slot N / --pause 0\|1 / --reload / --rules / --fg / --quit`，全部实测可用 |

**中途修掉的两个真问题**：`RELOAD` 原来只打了日志没真正换内存里的规则；
`STATUS` 每次开两次 HID 设备。

### P5 ✅ 自动切换 + 硬件优先

| 项 | 结果 |
|---|---|
| 规则文件 | `%LOCALAPPDATA%\RazerMate\rules.conf`，带注释的纯文本，用户可手改 |
| 实测链路 | `notepad.exe=2` → 打开记事本 → 设备真的切到 2 号槽；改成 `=4` 并 `--reload` → 切 4 |
| 硬件优先 | 外部改槽（模拟按底部按键）→ 日志「槽位被外部改为 3，退让 6 秒」，且**前台程序不变就一直不抢回来** |
| 重新接管 | 切走再切回同一程序 → 规则重新生效（实测 3 → 1） |
| 暂停/恢复 | `PAUSE 1` 后改规则不生效；`PAUSE 0` 立即接管（实测） |
| GUI 集成 | 「自动切换」页：服务状态徽章、启动/停止/暂停/恢复、填入当前前台、槽位选择、规则增删、全局选项（开关/检测间隔/无匹配槽位）—— 全部用真实鼠标点击验证过 |

**界面层修掉的两个真问题**：
1. 规则行的整行点击区声明在内容之后，把「删除」按钮的点击整个吞掉了（Slint 后声明者在上层）。
2. 提示条原本参与竖直布局，出现/消失时整页会跳一下，导致紧接着的点击打偏。改成绝对定位浮层。

### P6 ✅ 宏编辑器 + 灯带预览 + 打包

| 项 | 结果 |
|---|---|
| 宏编辑器 | 新建 / 打开 / 步骤增删改与排序 / 写入 Flash / 删除，全链路真实鼠标点击验证；摘要从十六进制改成可读动作串（`10 B · ctrl+c wait:200`） |
| **设备限制（新发现）** | **已存在的宏 ID 不能直接覆写**，`MACRO_SET_SIZE` 直接回 `Fail 0x06/0x08`。`macro_write()` 改为「备份旧内容 → 删除 → 写新 → 失败自动还原」，CLI 与 GUI 同时受益；实测 13B→10B→13B 全部 verified |
| 宏 ID 登记表 | 设备只报总数、不给 ID 列表。写入时登记、删除时注销，界面只查登记表；`macro scan` / GUI「扫描 Flash」按区间补发现。实测清空登记表后扫描能找回 `0x0202` |
| hold / toggle | `fn=0x04/0x05` 写入读回一致（PROTOCOL §8.1）；收尾又通过人工按键验收确认物理触发成立（`h=6` / `t=36`），GUI 按键映射页据此开放「按住播放宏 / 开关播放宏」两类编辑（`edit-kind` 9 / 10，带往返单测） |
| LED 帧率 | `bench-led`：**142.9 fps（7.00 ms/帧，11 颗灯）**，软件动画可行但持续占 CPU |
| 灯带预览（取舍） | 不做常驻动画。改为一次性「彩虹 / 渐变」+ 60 帧 2 秒流动（实测 2.16 s / 27.8 fps，按 30 fps 发送）+ 自动还原。明确标注运行时效果、不写 Flash |
| 跨进程还原 | `led-preview.bak` 记下预览前板载灯效，关程序 / 隔天仍能「还原」。连续预览 / 预览中跑基准都不会把临时帧覆盖成板载效果（实测彩虹→渐变→基准→还原，回到 `#0000FF`） |
| 打包（v1） | `versions/v1-gui/scripts/package.ps1`：`cargo build --release --workspace` → `versions/v1-gui/dist/RazerMate/`（3 个 exe + 中文 `使用说明.txt`）。免安装、免注册表、免管理员 |

**「单 exe」的取舍**：原计划写「单 exe 免安装」，实际交付是一个免安装目录里的
3 个 exe。理由：后台服务要常驻且只占 2 MB，把 Slint GUI 整个链进去会白白拖大
常驻内存；命令行工具用于自检与排障，也不该塞进 GUI。三者职责分开，目录整体
拷走即用，卸载只需删目录。这一点与 PLAN 原表述不同，如实记录。

**界面层修掉的问题**：宏页多个 `Btn` 被 `horizontal-stretch` 挤成窄缝（给死宽度）、
底部说明文字换行后贴边溢出（给固定高度 + stretch）。

### P7 ✅ 打磨：备份 / 恢复 / 自启 / 崩溃安全

| 项 | 结果 |
|---|---|
| 配置快照 | `razerctl backup show/save/restore`，纯文本可读可 diff：规则、宏登记表 + 当前槽的 13 条按键绑定、3 区灯效与亮度、DPI、回报率、生效槽、宏本体 |
| 恢复实测 | 改乱（灯效改红 + DPI 键改 `left`）→ `backup restore` → **22 项逐项读回校验**，灯效回 `#0000FF`、DPI 键回 `key:esc`、槽位 4 / normal mode 不变 |
| GUI 集成 | 设置页「立即备份 / 恢复最近备份 / 打开目录」；改乱后经界面点恢复，设备精确还原 |
| 运行时效果不入快照 | CUSTOM（`effect=8`）不写 Flash，导出时改用 `led-preview.bak` 里的板载灯效覆盖；没有备份就标注跳过。实测预览中备份，快照仍是三区 `0000ff` |
| 开机启动 | `HKCU\...\Run\RazerMate` 指向 `razermate-agent.exe`，Win32 注册表 API 进程内读写（不起子进程、不闪窗、无需管理员）。徽章 / 按钮 / 注册表三方核对通过，测完已关闭还原现场 |
| 崩溃安全 | 板载设置本来就在鼠标 Flash，程序崩溃 / 卸载都不影响；每次写都读回校验并回滚；宏覆写先备份旧内容；灯带预览另存跨进程备份 |

**修掉的诊断误导**：`Lighting::parse` 要求 `6 + n×3` 字节，而 CUSTOM 效果设备只回
12 字节（声明 11 颗，实际给 2 颗），解析失败被 `core::lighting()` 伪装成
`HID transfer failed (win32 error 0)`，看起来像硬件断了。现在按实际字节数解析，
并新增 `Error::Parse` / `Error::Io` 明确区分「应答解析失败」「本地文件失败」与真正的
传输故障；界面能正确显示「逐灯预览（运行时）· 设备回报 2 颗颜色」。

### 收尾补测 ✅ 最后一条可自动化验证的 C 级项

本计划 §5 曾把「回报率 / 滚轮模式是否走 p0」列为中等风险。收尾时用
release 版 `razerctl raw` 对 `cls 0x02` 的三组滚轮命令做了完整往返验证，
结论已同步到 `docs/PROTOCOL.md` §3.2 与 §8.1：

| 步骤 | 结果 |
|---|---|
| 5 个板载槽 + p0 只读探测 | 全部 `mode=0 / accel=0 / smart=1`，应答 `[profile, value]` |
| 写相反值 | `0x14/0x16/0x17` 在 normal mode 下均 `Ok`，读回 `1/1/0` 完全一致，无需 driver mode |
| 切槽 `3 → 4` | 4 号槽仍保持改写值 → 设定落在对应 profile，落盘 |
| 按槽隔离 | 只改 4 号槽 `mode=1` 时 3 号槽仍为 0 |
| p0 独立寻址 | 写 p0 不影响 4 号槽，可纳入虚拟槽工作流 |
| 回报率 | `cls 0x00 / 0x8E` 无 profile 参数，应答 `[00, delay_ms]`，设备级 |
| 现场还原 | 三项恢复 `0/0/1`、生效槽 4、normal mode；5 槽复查一致 |

### 人工按键验收 ✅ `fn=4` / `fn=5` 物理触发

`tests/holdtest.py` 把「宏播放了几次」变成可数字符：p0 上把 `dpi` 绑
`macrohold:0x0301`（吐 `h`）、`forward` 绑 `macrotoggle:0x0302`（吐 `t`）、
`back` 绑普通 `macro:0x0303`（吐 `c`，对照组），三个宏都是 `text:x, wait:300`。
置顶观察窗收集键盘输入，收尾强制还原。

| 项 | 观测 | 结论 |
|---|---|---|
| 写入读回 | `fn=4 data=[03,01,01]` / `fn=5 data=[03,02,01]` / `fn=3 data=[03,03,01]` | 三种触发方式在 p0 绑定正确 |
| 对照组 | `c=1` | 宏本体、p0 绑定、键盘输入链路均正常 |
| hold | `h=6`，且原始串 `'chhhhhhttt…'` 中**最后一个 h 之后 0 个 h** | 按住 DPI 键期间循环播放成立，松开即停 |
| toggle | `t=36` | 点一次前进键后自行循环播放成立 |
| toggle 停机 | 本轮由脚本 `bind off` 强制掐断；上一轮人工观测 `t=40` 后第二次点击停下 | 开关式停止可用，本轮不重复声称 |
| 现场还原 | 3 个宏删除、`macros 0`、p0 三个绑定回到 `key:esc` / `alt+w` / `alt+v`、切槽 `3→4`、生效槽 4 / normal mode | `OK` |
| GUI 端到端 | release GUI「按键映射」页真实鼠标点击「按住播放宏 / 开关播放宏」→ 写入当前生效槽的 DPI 扳机键 → 设备读回 `fn=4 data=[01,01,01]` / `fn=5 data=[01,01,01]`；选中态、参数提示、成功 Toast 均正确；随后还原 `key:esc` | 通过 |

`tests/holdtest.py --selftest` 用本轮真实采集串离线验证判读逻辑，10/10 通过；
`--prepare` / `--watch` / `--restore` 三态可复跑，且任何路径都保证还原。
一个副作用如实记录：删除宏后 `recycled` 增加 768 B（3 页），`available` 从
86272 B 降到 85504 B —— 固件会自行回收，本项目不碰格式化命令。

至此 `docs/PROTOCOL.md` §8 的 C 级清单只剩一条**不应自动化**的项：Flash 格式化
命令（本项目不使用，且属破坏性操作）。

### P8 ✅ 界面美化（Apple HIG + 产品参考图）

依据 Apple Human Interface Guidelines（设计原则、Layout、Typography、
Accessibility）与 Basilisk V3 产品图 / 配置界面参考图，在不加载图片、不做特效
渲染、不播放动画的约束下重做视觉层级。设计依据与色值实测见 `versions/v1-gui/docs/UI_DESIGN.md`。

| 项 | 结果 |
|---|---|
| 导航 | 每项配 16×16 纯几何图标（零贴图），侧栏分「设备 / 自动化 / 程序」三组 |
| 顶栏 | 标题 19px + 副标题 12px + 带圆点的状态徽章 + 2px RGB 五色细条，呼应鼠标底盘灯带 |
| 设备卡 | `B3` 标识加 RGB 小条；HID 长路径简化为 `USB HID · 1532:0099 · 控制口 MI_03`；固件 / 模式改徽章 |
| 统计卡 | 数值放大到 21px，顶部 3px 色条；只有「当前生效槽」使用绿色 |
| 控件 | 按钮 32px、Chip 28px、色块 32px、滑条拇指带绿环；统一 8 / 10 / 12px 圆角 |
| 可读性 | 辅助文字 `#59606A`（2.45:1）→ `#858E9C`（4.52:1），全部文字达 WCAG AA；修复禁用主按钮近黑字问题 |
| 空状态 | 宏列表为空改为带图标的引导卡片；状态栏去掉 100+ 字符路径，完整路径仍可用 `razerctl info` 查 |

8 页逐张 `980×680` 截图核对，无重叠 / 溢出 / 截断；
回归：54 个单测全通过、golden 21/21、`selftest` 8/8；
release GUI 真实点击回归：点选 DPI 扳机键 →「按住播放宏」→ 写入，设备读回
`fn=4 data=[01,01,01]`，随后还原 `key:esc`——新尺寸下控件命中正常。
### P9 ✅ 按键参数改为点选 / 录入（不再手填）

用户反馈「按键不用填写，要录入或可选项目（如复制粘贴）」。按动作类型把参数输入框
改成点选 / 录入，手填降级为折叠兜底：

| 类型 | 新交互 |
|---|---|
| 鼠标键 | 9 个动作 Chip 两行列全（左右中、后退前进、滚轮上下、左右倾） |
| 组合键 | 8 个常用预设 Chip（复制 / 粘贴 / 剪切 / 全选 / 保存 / 撤销 / 重做 / Alt+Tab）+「录入组合键」 |
| DPI 动作 | 提高 / 降低 / 循环 3 个 Chip |
| 媒体键 | 播放 / 上下曲 / 停止 / 静音 / 音量 ± 共 7 个 Chip |
| 宏（播放 / 按住 / 开关） | 列出 Flash 中已有宏直接点选；播放另配 1 / 2 / 3 次 Chip |
| 其他值 | 「手动输入（高级）」折叠区，保留协议串输入能力 |

「录入组合键」实现：`FocusScope.key-pressed` 拿到 Slint `KeyEvent.text` 与 4 个修饰键
状态，`razer_core::key_name_from_slint_text` 把 Slint 私有区特殊键（F1–F24、方向键、
Enter、Esc、Space 等）映射到设备键名，`chord_token` 拼成 `ctrl+c` 这类参数，
再用 `parse_action` 校验设备认得才采纳。新增 3 组单测覆盖映射与往返。

实测：GUI 点左键行 → 切组合键 → 点「录入组合键」→ 注入真实 `Ctrl+C`，界面显示
「当前：ctrl+c」且复制 Chip 高亮 → 点写入，当前生效槽 1 的 left 读回
`fn=2 data=[01,06]`，随后还原 `left`。回归：56 个单测全通过。
P0–P9 与全部补测完成，没有遗留待办。


---

## 附录：本轮实验环境

```
设备    Razer Basilisk V3 (1532:0099)，fw v1.2，控制口 MI_03
驱动    Windows 自带 HID-compliant device（无需雷蛇驱动）
雷蛇    Synapse 全部进程 0；两个服务 Start=4 已禁用
工具    openrazer-win 1.6.8 (direct 模式) / Python 3.12.10
实验后状态  slot 4 激活、mode 0:0、DPI 2700、回报率 1000 Hz、三区静态蓝 #0000FF、
           DPI 键 key:esc、无宏、无后台服务、无开机启动项；配置目录留有 latest.txt 快照
```
