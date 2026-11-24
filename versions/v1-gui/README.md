# v1 · 上位机 GUI（已冻结）

第一版：Slint 图形控制面板，全中文界面，只针对 Basilisk V3 有线版（`1532:0099`）。
界面难以继续维护，因此冻结；第二版改为「JSON 配置 + 后台常驻程序」，不再依赖本界面。

## 本目录内容

| 路径 | 说明 |
| --- | --- |
| `dist/RazerMate/` | 免安装发行包：`razermate.exe`（控制面板）、`razermate-agent.exe`（后台服务）、`razerctl.exe`（CLI）、`使用说明.txt` |
| `docs/UI_DESIGN.md` | 视觉系统定义与逐页排版审计记录（第 1~26 条） |
| `docs/screenshots/` | 最终界面截图（概览、自定义两态、自动切换、设备卡、设置卡） |
| `docs/使用说明.txt` | 用户说明；打包脚本会把它复制进发行包 |
| `scripts/package.ps1` | 重新构建并刷新发行包 |
| `tests/` | UI 自动化脚本：窗口截图、真实点击、滚动到底验证 |

## 源码位置（共用 workspace）

- 界面：`../../apps/razermate/`（Rust + `ui/app.slint`）
- 后台服务：`../../apps/razermate-agent/`
- 命令行：`../../apps/razerctl-rs/`
- 设备与协议知识：`../../docs/`

## 重新打包

在任意目录执行：

```
powershell -ExecutionPolicy Bypass -File versions\v1-gui\scripts\package.ps1
```

产物覆盖写回本目录 `dist/RazerMate/`。脚本自行定位 workspace 根，不依赖当前工作目录。

## 状态

`cargo test --release --workspace` 60 项全过；最后运行模式为离线，且已重启验证持久化。

已知未处理：顶栏「离线 / 在线」分段切换的语义表达（当前是绿色实心按钮，容易被读成状态而非开关）——不影响功能，且本版已冻结，不再改。
