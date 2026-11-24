# RazerMate v2 · 配置驱动版手册

不依赖上位机。`profiles.json` 是唯一事实源，一个无窗口、无托盘、开机自启的后台程序
按前台应用自动切换按键 / 宏 / DPI / 回报率与三区灯光。

按需要选文档：

| 你要做什么 | 看哪份 |
| --- | --- |
| 改配置、日常排障 | 本文件；一页速查见 `使用说明.txt` |
| 让 AI / Agent 生成或改配置 | `PROFILES_GUIDE.md`（用户偏好与字段语义） |
| 机器校验 / IDE 提示 | `profiles.schema.json`（JSON Schema draft-07） |

## 1. 本机当前状态（照这个用就行）

| 项 | 实际值 |
| --- | --- |
| 后台程序 | `dist\RazerMate\razermate-headless.exe` |
| **要改的配置文件** | `versions\v2-headless\profiles.json` |
| 开机自启 | 已写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，命令行带 `--config` 指向上面这份 JSON |
| 运行日志 | `%LOCALAPPDATA%\RazerMate\headless.log`（超过 512 KB 自动裁半） |
| 生效方式 | 保存 JSON 约 1 秒热更新，不重启、不重编译、不开界面 |
| 只读的辅助工具 | `dist\RazerMate\razerctl.exe`（读回设备绑定 / 查看槽位） |

⚠️ **别改错文件**：不指定 `--config` 时，程序默认读 `%LOCALAPPDATA%\RazerMate\profiles.json`，
且文件不存在会自动生成一份内置默认模板。当前自启实例用的是仓库里这份，改 LOCALAPPDATA 那份不生效。
要换配置位置，必须重新执行 `--install --config <新路径>` —— 自启命令里的 `--config` 是安装时写死的。

## 2. 实测资源占用（2026-11-24，真机常驻）

| 指标 | 数值 |
| --- | --- |
| 工作集 | 10.13 MB |
| 私有提交（独占内存） | **2.13 MB** |
| 线程 / 句柄 | 1 / 146 |
| 累计 CPU | 0.16 秒 / 运行 3 分钟（约 0.09%） |
| 程序体积 | 533 KB（`razermate-headless.exe`） |
| 界面 / 托盘 | 无 |

连续 4 次采样无增长。10 MB 工作集里包含映射进来的系统 DLL 共享页，任务管理器默认「内存」列
显示的是活动专用工作集，约 2 MB；「详细信息 → 工作集」才显示 10 MB。两者都是对的。

轻量原因：无窗口、无托盘、单线程；每 800 ms 只做一次前台窗口查询 + 配置文件 mtime/len 比对；
JSON 解析成紧凑索引后原文即释放；只有切换配置组或配置变更时才下发，设备层还会读回比对做差量写。

## 3. 后台程序零硬编码

`razermate-headless` 只是通用解释器：按钮 ID 表、每个键映射什么动作、灯区编号、应用匹配规则、
轮询间隔、服务策略全部来自 JSON。程序里没有 `if button == "left"`，也没有「哪几个键固定」的判断。

- `left / right / wheelup / wheeldown` 当前写 `native` 只是**用户此刻的选择**，改一行 `action` 就能换功能；
- `locked: true` 是给人和 AI 看的提示，程序只读 `action`，不会因此拒绝修改；
- `native` 会去 `device.buttons[键名].native` 查具体动作串展开，不是写死的行为；
- 固件将来支持新功能，用 `raw:<fn>,<size>,<data...>` 直接写原始绑定字节；
- `_` 开头和不认识的字段一律忽略，可以安全往 JSON 里加选项。

**结论：改配置 = 改行为，永远不用重新编译后台程序。**

## 4. 配置合并与下发

```
profiles.json ──> razer-config ──> SlotConfig ──> apply_virtual_slot ──> p0
                  │                                 │
                  ├─ device.buttons（名称→ID）       ├─ DPI（会切槽，必须最先）
                  ├─ base.bindings（13 键全量）      ├─ 回报率 / 灯效 / 亮度
                  ├─ profile.overrides（覆盖任意键） └─ 按键绑定（必须最后）
                  └─ native / 宏名展开
```

每次下发**合并后的全量 13 键 + 三区灯效**（`base + 选中组的 overrides`），设备层读回比对做差量写，
因此不会残留上一组的动作或颜色，各组也不必写全。

⚠️ **顺序是硬性要求**：`set_dpi()` 切 driver mode 再切回时会还原槽位，而任何切槽都会用板载槽内容
覆盖整个 p0。按键绑定必须最后写，否则会出现「报告写入成功、读回却是旧值」。
固定顺序：`DPI → 回报率 → 灯效 → 亮度 → 按键绑定`。

## 5. 命令

程序路径下文简写为 `H = dist\RazerMate\razermate-headless.exe`，
`CFG = versions\v2-headless\profiles.json`（用绝对路径更稳）。

| 命令 | 作用 |
| --- | --- |
| `H --run --config <CFG>` | 后台常驻（不带任何参数同此） |
| `H --once --config <CFG>` | 只下发一次当前前台对应的配置后退出 |
| `H --apply <组id> --config <CFG>` | 强制套用某一组后退出，如 `--apply kicad` |
| `H --validate --config <CFG>` | 校验配置：全部动作串、宏引用、灯区、组优先级 |
| `H --status --config <CFG>` | 配置路径 / schema / 前台 / 选中组 / 自启状态 |
| `H --init --config <CFG>` | 配置不存在时生成内置默认模板（不覆盖已有文件） |
| `H --install --config <CFG>` | 写开机自启，**带上 --config 才指向你要的那份 JSON** |
| `H --uninstall` | 删除开机自启 |
| `H --debug --config <CFG>` | 常驻同时把日志打到控制台 |
| `H --help` | 打印内置帮助 |

`--config` 可放在命令前后任意位置；`--validate` 后面也可直接跟文件路径。

## 6. 服务参数（`service` 段）

| 字段 | 当前值 | 含义 |
| --- | --- | --- |
| `poll_ms` | 800 | 前台窗口轮询间隔 |
| `config_watch_ms` / `config_debounce_ms` | 400 / 250 | mtime+len 检测 / 去抖，合计约 1 秒热更新 |
| `on_error` | `keep_last` | 配置写坏时保留上一份有效配置，绝不把半截配置写进设备 |
| `reapply_interval_ms` | **0（关闭）** | 定时重下发 p0 的间隔；设为 5000~10000 可在切槽 / 断电 / 唤醒后自动恢复 |
| `match_by` | `foreground_exe`, `window_title` | 当前启用的匹配维度；`process_path` 可选但未启用 |
| `fallback` | `global` | 没命中任何组时的兜底组 |
| `log.max_kb` | 512 | 日志超过上限只保留后半段 |

如实说明：`reapply_on_device_arrival` 与 `reapply_on_wake` 已在 JSON 里定义，但当前后台程序**尚未接线**；
p0 丢失后的自动恢复请靠 `reapply_interval_ms`（见下节）。开启后每次重下发都会写一行日志，日志有轮转上限。

## 7. 边界与已知限制

- **只写 p0**（direct profile / RAM 工作层），绝不读写、切换、擦除 5 个板载配置槽；所有组的
  `profile` 键一律 `off`。
- **p0 易失**：断电、按物理 Profile 键切槽、休眠唤醒会丢。当前 `reapply_interval_ms = 0`，
  丢失后靠「切换应用组」「改一次 JSON」「重新插拔 / 重启后台」恢复；想要全自动，把该值设为 5000 即 5 秒兜底。
- **多步宏要写设备宏区（Flash）**，默认 `service.macros.allow_flash_write: false`；
  当前 3 个宏（`close_tab` / `save` / `save_as`）都是单步组合键，自动展开成按键绑定，零 Flash 写入。
- **前台匹配认 exe 名**，UWP / 打包应用会落在 `ApplicationFrameHost.exe`，可用
  `match.title_contains` / `path_contains` 补足。
- 程序只支持 Basilisk V3 有线版（VID 1532 / PID 0099）；换设备先改 `device.buttons` 的 ID 表。

## 8. 排障

| 现象 | 先查什么 |
| --- | --- |
| 改了 JSON 没反应 | 改的是不是第 1 节那份；`H --status --config <CFG>` 看路径；日志里有没有「配置已热更新」或「配置写坏」 |
| 复制 / 粘贴失效 | 多半是 p0 被切槽覆盖：`razerctl binding tiltleft --slot 0` 读回，正常应为 `key:ctrl+v`；执行 `H --once` 立即恢复 |
| 后台没在跑 | 任务管理器看 `razermate-headless`；`H --status` 看自启是否「开」；`H --install --config <CFG>` 重写 |
| 配置写坏了 | 保留上一份有效配置，看日志里的具体错误；`H --validate` 定位；修好保存即自动恢复 |
| 某组没生效 | 看 `--status` 的「选中组」；`priority` 大的优先，exe 名要带 `.exe` 且小写匹配 |
| 灯效不对 | 三区都要写 `effect` + `brightness`；设备没有 off 效果，用 `brightness: 0` |

## 9. 验收基线（改配置或改代码后照此回归）

1. `H --validate --config <CFG>` 通过，退出码 0；
2. `razerctl binding tiltleft --slot 0` = `key:ctrl+v`，`tiltright` = `key:ctrl+c`；
3. `left / right / wheelup / wheeldown` 读回仍为 `fn=1` 原生；
4. `razerctl slot` 显示 5 个板载槽颜色与激活槽均未变化；
5. 临时改一个键（例如给 browser 组加 `right: key:ctrl+w`），不重编译，热更新后读回生效，
   还原 JSON 后读回原生 —— 用来证明「改 JSON 即改行为」链路完整。
