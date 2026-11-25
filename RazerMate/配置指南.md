# RazerMate v2 配置总表

> `profiles.json` 是唯一事实源；本表是它的**可读整理**，两者冲突时以 JSON 为准。
> 全面转向 JSON 配置驱动 + 无界面后台，不再维护 GUI：没有界面、截图、主题、控件等任何资料。

## 1. 用户偏好速览

| 项 | 当前选择 |
| --- | --- |
| 滚轮左倾 | 粘贴 `Ctrl+V` |
| 滚轮右倾 | 复制 `Ctrl+C` |
| 左键 / 右键 / 上滚 / 下滚 | 保持原生功能（只是此刻的选择，随时可改） |
| 底部 Profile 键 / 狙击键 | 禁用（避免切槽冲掉在线配置） |
| 运行方式 | 开机自启、无窗口无托盘、后台常驻 |
| 设备策略 | 只写 p0 工作层，绝不读写 / 切换 / 擦除 5 个板载配置槽 |
| DPI / 回报率 | 软件不下发，用鼠标 DPI 键或板载配置设置（原因见第 8 节） |

## 2. 按键总表（13 键）

名称与物理 ID 来自 `device.buttons`；"当前动作"是 `base.bindings`，各组可覆盖任意键。

| 键名 | ID | 中文 | 当前动作 | 含义 |
| --- | --- | --- | --- | --- |
| `left` | 1 | 左键 | `native` → `mouse:left` | 原生 |
| `right` | 2 | 右键 | `native` → `mouse:right` | 原生 |
| `wheelup` | 9 | 滚轮上滚 | `native` → `mouse:wheelup` | 原生 |
| `wheeldown` | 10 | 滚轮下滚 | `native` → `mouse:wheeldown` | 原生 |
| `tiltleft` | 52 | 滚轮左倾 | `key:ctrl+v` | **粘贴**（出厂是横向滚动） |
| `tiltright` | 53 | 滚轮右倾 | `key:ctrl+c` | **复制**（出厂是横向滚动） |
| `middle` | 3 | 滚轮中键 | `native` → `mouse:middle` | 原生 |
| `back` | 4 | 侧键·后退 | `native` → `mouse:back` | 各组常覆盖 |
| `forward` | 5 | 侧键·前进 | `native` → `mouse:forward` | 各组常覆盖 |
| `profile` | 14 | 底部 Profile 键 | `off` | 禁用，绝不切板载槽 |
| `clutch` | 15 | 狙击键 | `off` | 禁用，避免误触 |
| `dpi` | 96 | DPI 扳机键 | `dpi:cycle` | 循环 DPI 档位 |
| `wheelmode` | 106 | 滚轮模式切换 | `wheelmode` | 切换滚轮阻尼 |

`native` 会去 `device.buttons[键名].native` 查具体动作串，不是写死的行为。
`locked` / `equivalent` / `aliases` 只是给人看的说明字段，程序只读 `action`。

## 3. 四组配置

按前台程序自动切换，`priority` 大的优先，没命中用 `global`。
每组只写要改的键 / 灯区，其余继承 `base`（下表已列出完整结果）。

### `global` · 全局兜底（priority 0）

| 项 | 值 |
| --- | --- |
| 命中 | 无任何组命中时（桌面、资源管理器、未知程序） |
| 按键 | middle / back / forward / wheelmode 原生；profile / clutch = `off`；dpi = `dpi:cycle`；左右倾复制粘贴；四主键原生 |
| 灯光 | 滚轮、Logo 静态 `#46d92d` 亮度 180；灯带 `spectrum` 亮度 140 |
| DPI / 回报率偏好（不下发） | 1600 / 500 Hz |

### `browser` · 浏览器 / 编辑器（priority 10）

| 项 | 值 |
| --- | --- |
| 命中 exe | `chrome.exe`、`msedge.exe`、`Code.exe`、`firefox.exe` |
| 按键 | `back` = 关标签页（`macro:close_tab` ≈ `Ctrl+W`）、`forward` = `key:ctrl+t` 新标签、`clutch` = `media:volup` 音量+、其余同 base |
| 灯光 | 三区静态 `#4c9dff`，滚轮 160 / Logo 160 / 灯带 120 |
| DPI / 回报率偏好（不下发） | 2000 / 1000 Hz |

### `kicad` · KiCad / EDA（priority 20）

| 项 | 值 |
| --- | --- |
| 命中 exe | `kicad.exe`、`pcbnew.exe`、`eeschema.exe`；标题含 `KiCad` / `PCB Editor` |
| 按键 | `back` = `macro:save`（`Ctrl+S`）、`forward` = `macro:save_as`（`Ctrl+Shift+S`）、`clutch` = `key:ctrl+z` 撤销、`dpi` = `dpi:down`、`wheelmode` = `off` |
| 灯光 | 三区静态 `#ffab4a`，滚轮 150 / Logo 150 / 灯带 100 |
| DPI / 回报率偏好（不下发） | 1200 / 500 Hz |

### `game` · 游戏（priority 30）

| 项 | 值 |
| --- | --- |
| 命中 exe | `cs2.exe`、`LeagueClient.exe`、`steam.exe` |
| 按键 | `clutch` = `dpi:down`、`wheelmode` = `off`，其余同 base |
| 灯光 | 三区 `wave`（mode=1、speed=5），亮度 200 |
| DPI / 回报率偏好（不下发） | 800 / 1000 Hz |

## 4. 宏

| 宏名 | 步骤 | 等价 | 触发 | Flash |
| --- | --- | --- | --- | --- |
| `close_tab` | `ctrl+w` | 关闭当前标签页 | once | 零（单步展开成按键绑定） |
| `save` | `ctrl+s` | 保存 | once | 零 |
| `save_as` | `ctrl+shift+s` | 另存为 | once | 零 |

三个都是单步组合键，`id: null`。多步宏（含 `wait:` / `text:`）需要写设备宏区，
默认 `service.macros.allow_flash_write: false`，开启后才会分配并回填 `id`。

## 5. 灯光与传感器

| 灯区 | region ID | 可用效果 |
| --- | --- | --- |
| `wheel` 滚轮 | 1 | `static`（需 `rgb`）/ `spectrum` / `wave`（可给 mode、speed） |
| `logo` Logo | 4 | 同上 |
| `strip` 灯带 | 10 | 同上 |

- 亮度范围 0~255；设备没有 off 效果，关灯用 `brightness: 0`。
- 每个区都要写 `effect` + `brightness`，漏写的区继承 `base`。
- `device.caps`：DPI 100~26000、回报率 125/500/1000 Hz、绑定层 normal/hypershift。
- `sensor._preference` 里的 DPI / 回报率只是偏好记录，程序不下发。

## 6. 应用匹配

| 字段 | 说明 |
| --- | --- |
| `match.exe` | 前台进程名，带 `.exe`，小写比较 |
| `match.title_contains` | 窗口标题包含 |
| `match.path_contains` | 进程路径包含（可选维度） |
| `service.match_by` | 当前启用 `foreground_exe` + `window_title` |
| `service.fallback` | `global` |

轮询 800 ms。UWP / 打包应用会落在 `ApplicationFrameHost.exe`，要用 `title_contains` 补足。

## 7. 动作语法速查（改配置时用）

| 写法 | 说明 |
| --- | --- |
| `native` | 保持该键出厂功能 |
| `mouse:<名称>` / `mouseext:<名称>` | 鼠标原生 / 扩展功能（fn=1 / fn=14） |
| `key:ctrl+v`、`key:ctrl+shift+s` | 任意组合键；主键也接受 `0x` 十六进制 HID usage |
| `macro:<名称>` / `macrohold:` / `macrotoggle:` | 播放 / 按住播放 / 开关播放宏 |
| `media:volup` / `media:0x00e9` | 媒体键 |
| `dpi:up` / `dpi:down` / `dpi:cycle` | 把按键绑成 DPI 档位动作 |
| `off` / `wheelmode` / `profile:next` / `double` / `hypershift` | 其它固件功能 |
| `raw:<fn>,<size>,<d0..d4>` | 逃生舱：直接写原始绑定字节，固件新增功能也不用重编译 |

## 8. 改配置与运行

**改哪份**：`versions\v2-headless\profiles.json`（开机自启命令带 `--config` 指向它）。
保存约 1 秒生效，不重编译、不重启、不开界面。改 `%LOCALAPPDATA%\RazerMate\profiles.json` 无效。

常改写法：

- 换右键功能：在 `base.bindings.right`（全局）或某组 `overrides.bindings.right` 写 `"action": "key:ctrl+w"`
- 交换复制粘贴：对调 `base.bindings.tiltleft.action` 与 `tiltright.action`
- 新增应用组：往 `profiles` 加 `id` / `scope: "app"` / `priority` / `match.exe`，`overrides` 只写要变的项
- 关灯：该区 `brightness: 0`
- 换鼠标：改 `device.buttons` 的 ID / native 与 `device.led_regions` 编号

程序是通用解释器：按钮表、动作、灯区、匹配规则全在 JSON，改 JSON 即改行为。
`_` 开头和不认识的字段一律忽略，可以安全加新选项。

常用命令（`H = dist\RazerMate\razermate-headless.exe`，`CFG = 本目录 profiles.json` 的绝对路径）：

| 命令 | 作用 |
| --- | --- |
| `H --validate --config <CFG>` | 校验配置 |
| `H --status --config <CFG>` | 看生效路径 / 前台 / 选中组 / 自启 |
| `H --once --config <CFG>` | 立即重新下发一次 |
| `H --apply kicad --config <CFG>` | 强制套用某组 |
| `H --install --config <CFG>` / `--uninstall` | 写 / 删开机自启（装自启必须带 `--config`） |
| `H --run --config <CFG>` / `--debug` / `--help` | 常驻 / 控制台日志 / 帮助 |
| `dist\RazerMate\razerctl.exe binding tiltleft --slot 0` | 读回某个键的实际绑定 |

日志：`%LOCALAPPDATA%\RazerMate\headless.log`。

## 9. 当前边界

- **只写 p0**（RAM 工作层）：断电 / 按物理 Profile 键切槽 / 休眠会丢，靠切换应用组、改一次 JSON、
  `--once` 或重启后台恢复；`reapply_interval_ms` 刻意保持 0。
- **DPI / 回报率不下发**：写它们必须切 driver mode 再切回，会还原板载槽并覆盖 p0，
  形成「写 DPI → 切模式 → p0 被覆盖 → 再写」的死循环，设备反复重载板载配置，
  表现为滚轮阻尼反复切换、灯效反复重刷、按键读回变回出厂值。偏好保留在 `sensor._preference`，
  不要把它改回裸 `dpi` / `polling`，也不要开启定时重下发。
- 当前三个宏零 Flash 写入；多步宏才会动设备宏区。
- 只支持 Basilisk V3 有线版（VID 1532 / PID 0099）。
