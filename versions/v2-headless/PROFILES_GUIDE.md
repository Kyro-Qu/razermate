# RazerMate v2 配置说明（给 AI / Agent 读）

> 阅读对象：任何要修改 `profiles.json` 的 AI、Agent 或人。
> `profiles.json` 是唯一事实源；本文件描述用户偏好与当前配置状态，两者冲突时以 JSON 为准。
> 契约版本 `schema: 3`，机器可校验的约束见同目录 `profiles.schema.json`；
> 人用手册与排障见 `README.md`，一页速查见 `使用说明.txt`。

## 1. 最重要的架构事实：后台程序零硬编码

后台程序（`apps/razermate-headless`）是**通用解释器**，它：

- 不认识任何按键名字 —— 按钮 ID 表来自 `device.buttons`；
- 不内置「哪几个键固定」的判断 —— 每个键映射什么功能全由 `action` 决定；
- 不判断哪个应用该用什么颜色 / DPI —— 匹配规则来自 `profiles[].match`；
- 不知道灯区编号 —— 来自 `device.led_regions`；
- 忽略所有 `_` 开头字段和不认识的字段 —— 新增配置选项不会让程序崩溃。

**所以改配置 = 改行为，永远不需要重新编译后台程序。** 换鼠标只改 `device.buttons` 的 ID 表；
固件将来支持新功能，用 `raw:` 逃生舱直接写原始绑定字节，也不用改代码。

`locked: true` 只是**给人 / AI 看的提示**，程序只读 `action`，绝不会因为 locked 而拒绝修改。

## 2. 当前运行实例（改之前先确认改的是不是这份）

| 项 | 实际值 |
| --- | --- |
| 生效的配置文件 | `versions\v2-headless\profiles.json`（仓库内） |
| 后台程序 | `versions\v2-headless\dist\RazerMate\razermate-headless.exe` |
| 开机自启 | HKCU Run，命令行带 `--config` 指向上面这份 JSON |
| 日志 | `%LOCALAPPDATA%\RazerMate\headless.log` |
| 生效延迟 | 保存后约 1 秒（400 ms 检测 + 250 ms 去抖） |
| 常驻开销 | 私有内存 2.13 MB / 工作集 10.13 MB / 1 线程 / CPU 近零 |

⚠️ 程序在**未指定 `--config`** 时才读 `%LOCALAPPDATA%\RazerMate\profiles.json`，且文件不存在会自动生成内置默认模板。
当前自启实例没用那个路径 —— 改它不会生效。换位置要重新 `--install --config <新路径>`。

## 3. 用户当前偏好（此刻的 `base.bindings`，不是程序限制）

| 按键 | ID | 当前 action | 含义 |
| --- | --- | --- | --- |
| `left` | 1 | `native` | 左键原生，不参与宏和按键映射 |
| `right` | 2 | `native` | 右键原生，不参与宏和按键映射 |
| `wheelup` | 9 | `native` | 上滚原生，不参与宏和按键映射 |
| `wheeldown` | 10 | `native` | 下滚原生，不参与宏和按键映射 |
| `tiltleft` | 52 | `key:ctrl+v` | **粘贴** |
| `tiltright` | 53 | `key:ctrl+c` | **复制** |
| `middle` | 3 | `native` | 中键原生 |
| `back` | 4 | `native` | 后退（各组可覆盖） |
| `forward` | 5 | `native` | 前进（各组可覆盖） |
| `profile` | 14 | `off` | 禁用，v2 绝不切板载槽 |
| `clutch` | 15 | `off` | 狙击键禁用 |
| `dpi` | 96 | `dpi:cycle` | 循环 DPI 档位 |
| `wheelmode` | 106 | `wheelmode` | 切换滚轮阻尼模式 |

`native` 不是写死的行为：程序会去 `device.buttons[键名].native` 查具体动作串并展开。
例如 `left.native = "mouse:left"`（fn=1），`tiltleft.native = "mouseext:tiltleft"`（fn=0x0E，出厂是横向滚动）。

> 用户明确说过：这四个键现在只要原生功能，但**以后想改右键等功能必须能改**。
> 因此没有代码级封锁，只在这份 JSON 里写 `native`。改法见第 6 节。

## 4. 文件结构速查

| 段 | 作用 |
| --- | --- |
| `device.buttons` | 名称 → `{ id, zh, native, tags }`，程序据此解析所有绑定 |
| `device.led_regions` | 灯区名 → region ID（wheel=1 / logo=4 / strip=10） |
| `device.caps` | 设备能力上限，供 AI 生成配置时参考 |
| `service` | 自启、无 GUI、轮询、热更新、日志、p0 与宏策略 |
| `actions` / `lighting_effects` / `macro_steps` | 人读语法字典，程序不解析 |
| `macros` | 宏定义（steps / id / trigger），**是字典不是数组** |
| `base` | 基础层：`bindings` 写全 13 键，加 `sensor` 和三区 `lighting` |
| `profiles[]` | 每个应用组，`overrides` 只写要改的键 / 灯区，其余继承 `base` |

合并规则：`最终配置 = base + 选中 profile 的 overrides`。
**每次下发都是合并后的全量 13 键 + 三区灯效**，设备层再做读回差量写，因此绝不会残留上一组的动作或颜色 —— 不再需要「每组写全」。

## 5. 动作语法（`action` 字段）

| 写法 | 说明 |
| --- | --- |
| `native` | 查 `device.buttons[键名].native` 展开 |
| `mouse:left` / `mouse:wheeldown` | 普通鼠标原生功能，fn=1 |
| `mouseext:tiltleft` / `mouseext:tiltright` | 扩展鼠标功能，fn=0x0E |
| `key:ctrl+v`、`key:ctrl+shift+s`、`key:alt+f4` | 任意组合键；修饰键 ctrl/lctrl/rctrl/shift/alt/win，主键支持字母数字、F1~F24、方向键、编辑键 |
| `key:ctrl+0x1d` | 主键也接受十六进制 HID usage，程序没收录的键照样能配 |
| `macro:<名称>` / `macro:<id>x<次数>` | 播放宏；名称查 `macros`，否则按设备宏 ID 处理 |
| `macrohold:<…>` / `macrotoggle:<…>` | 按住播放 / 开关播放 |
| `media:volup` / `media:0x00e9` | 多媒体键，名称或十六进制 consumer code |
| `dpi:up` / `dpi:down` / `dpi:cycle` | DPI 档位 |
| `off` / `wheelmode` / `profile:next` / `double` / `hypershift` | 其它固件功能 |
| **`raw:<fn>,<size>,<d0..d4>`** | **逃生舱**：直接给设备写 fn / 长度 / 最多 5 字节 data，例 `raw:2,2,1,25`。固件新增功能无需重编译 |

单步组合键宏（`steps` 只有一条 `ctrl+w`）会自动展开成 `key:ctrl+w`，**零 Flash 写入**。
多步宏需要写设备宏区，必须先把 `service.macros.allow_flash_write` 设为 `true` 并回填 `id`。

## 6. 常见修改（全部只改 JSON）

**把右键改成别的功能**（以前被"固定"的键现在随便改）：

```json
"profiles": [ { "id": "browser", "overrides": { "bindings": {
  "right": { "action": "key:ctrl+w", "equivalent": "关标签页" }
} } } ]
```

保存后约 1 秒生效，不用重启、不用编译。想全局生效就改 `base.bindings.right`。
此例已做过端到端验证：不重编译，热更新后 `razerctl binding right --slot 0` 读回 `key:ctrl+w`；还原 JSON 后读回原生。

**交换复制 / 粘贴方向**：把 `base.bindings.tiltleft.action` 与 `tiltright.action` 对调。

**新增一个应用组**：往 `profiles` 加一项，给 `id` / `scope: "app"` / `priority` / `match.exe`
（exe 名带 `.exe`，小写比较）；`overrides` 里只写要变的键与灯区即可。

**适配别的鼠标**：改 `device.buttons` 的 `id` 与 `native`、`device.led_regions` 的编号；
`base.bindings` 的键名跟着新表走。

**关灯**：把该区 `brightness` 设为 `0`（设备没有 off 效果）。

## 7. 当前四组配置

| 组 | 命中 | 覆盖的按键 | DPI / 回报率 | 灯光 |
| --- | --- | --- | --- | --- |
| `global` | 兜底（priority 0） | middle / back / forward / wheelmode 原生，profile / clutch = off，dpi = cycle | 1600 / 500 | wheel + logo 静态 `#46d92d` 180；strip 光谱 140 |
| `browser` | chrome / msedge / Code / firefox（10） | back = `macro:close_tab`（等价 `ctrl+w`）、forward = `ctrl+t`、clutch = `media:volup`、wheelmode 原生 | 2000 / 1000 | 三区静态 `#4c9dff` 160 / 160 / 120 |
| `kicad` | kicad / pcbnew / eeschema，标题含 KiCad / PCB Editor（20） | back = `macro:save`、forward = `macro:save_as`、clutch = `ctrl+z`、dpi = `dpi:down`、wheelmode = off | 1200 / 500 | 三区静态 `#ffab4a` 150 / 150 / 100 |
| `game` | cs2 / LeagueClient / steam（30） | clutch = `dpi:down`、wheelmode = off | 800 / 1000 | 三区波浪 mode=1 speed=5，亮度 200 |

匹配：`service.match_by` 当前启用 `foreground_exe` + `window_title`（`process_path` 可选但未启用）；
`priority` 大的优先，没命中用 `service.fallback = global`。UWP / 打包应用落在 `ApplicationFrameHost.exe`，
要用 `title_contains` / `path_contains` 补足。

三个宏：`close_tab` = `ctrl+w`、`save` = `ctrl+s`、`save_as` = `ctrl+shift+s`，全是单步，`id: null`（零 Flash）。

## 8. 运行与校验

`H = dist\RazerMate\razermate-headless.exe`，`CFG = 本目录 profiles.json`（用绝对路径）。

| 事 | 命令 |
| --- | --- |
| 校验配置（含全部动作串 / 宏 / 灯区） | `H --validate --config <CFG>` |
| 看配置路径 / 当前组 / 自启 | `H --status --config <CFG>` |
| 立即下发一次 | `H --once --config <CFG>` |
| 强制某组 | `H --apply kicad --config <CFG>` |
| 生成默认模板（不覆盖） | `H --init --config <CFG>` |
| 写 / 删开机自启 | `H --install --config <CFG>` / `H --uninstall` |
| 后台常驻 | `H --run --config <CFG>`（无参数同此） |
| 控制台同时打日志 | `H --debug --config <CFG>` |
| 内置帮助 | `H --help` |
| 读回某个键的 p0 绑定 | `dist\RazerMate\razerctl.exe binding tiltleft --slot 0` |

`--install` 只有在同时传了 `--config` 时才会把路径写进自启命令；漏传则开机后读默认路径。
日志：`%LOCALAPPDATA%\RazerMate\headless.log`。

## 9. 必须守住的边界

- **只写 p0（direct profile / RAM 工作层）**，绝不读写、切换、擦除 5 个板载配置槽。
  `profiles[*].bindings.profile` 一律 `off` —— 用户按物理 Profile 键切槽会覆盖 p0。
- **DPI 必须最先写**：`set_dpi()` 会切 driver mode 再切回，为还原槽位会调 `set_active_profile()`，
  而任何切槽都会用板载槽内容覆盖整个 p0。所以 `apply_virtual_slot()` 顺序是
  `DPI → 回报率 → 灯效 → 亮度 → 按键绑定`，**按键绑定永远最后**。改顺序会让绑定看起来写成功了、读回却是旧值。
- **坏 JSON 保留上一份有效配置**（`service.on_error = keep_last`），绝不把半截配置写进设备。
- **p0 易失**：断电 / 切槽 / 休眠唤醒后会丢。当前 `reapply_interval_ms = 0`（定时重下发关闭），
  靠切换应用组、改一次 JSON、重新插拔或重启后台恢复；要全自动就把它设为 5000~10000。
  `reapply_on_device_arrival` / `reapply_on_wake` 已在 JSON 定义，但**当前后台程序尚未接线**，不要依赖它们。
- 宏若写设备宏区会动 Flash；默认 `allow_flash_write: false`。

## 10. 改完的检查清单

1. `H --validate --config <CFG>` 通过（会编译每一组，检查所有 action / 宏 / 灯区）；
2. 日志出现「配置已热更新，重新下发」，而不是「配置写坏，保留上一份有效配置」；
3. `razerctl binding tiltleft --slot 0` = `key:ctrl+v`，`tiltright` = `key:ctrl+c`；
4. 左 / 右 / 上下滚读回仍是 `fn=1` 原生值（除非有意改过）；
5. `razerctl slot` 显示板载 5 个槽颜色与激活槽未变化；
6. 实际按滚轮左右倾确认复制 / 粘贴生效。
