# RazerMate 版本划分

同一套 Cargo workspace（`apps/` + `crates/`）支撑两个版本：**源码共用，按版本归档的是交付物、文档与配置**。

不整体复制源码树：`target/` 约 9.8 GB，且 git 仓库根在上一级 `DeviceManagement/`，物理分叉会破坏构建路径与版本追踪。

| 版本 | 目录 | 交付形态 | 状态 |
| --- | --- | --- | --- |
| v1 · 上位机 GUI | `v1-gui/` | `razermate.exe` 图形控制面板 + 后台服务 + CLI | 已冻结，不再投入界面 |
| v2 · 配置驱动 | `v2-headless/` | `profiles.json` + 无 GUI 后台小程序（开机自启 / 纯在线模式 / 不碰板载配置） | 已实现并真机验证 |

v2 的 `profiles.json` 是唯一事实源，`v2-headless/` 内的交付物：

| 文件 | 用途 |
| --- | --- |
| `profiles.json` / `profiles.schema.json` | 唯一事实源 / draft-07 校验契约 |
| `README.md` | 完整手册：当前状态、命令、边界、排障、验收基线 |
| `使用说明.txt` | 一页速查（发行包内也放了一份） |
| `PROFILES_GUIDE.md` | 给 AI / Agent 读：用户偏好、动作语法、四组配置明细 |
| `dist/RazerMate/` | release 交付：`razermate-headless.exe` + `razerctl.exe` + 使用说明 |

后台程序零硬编码 —— 按钮 ID 表、动作串、匹配规则全在 JSON，改配置不用重新编译。
开机自启命令带 `--config` 指向 `v2-headless/profiles.json`，改这一份约 1 秒生效；
实测常驻私有内存 2.13 MB / 工作集 10.13 MB / 单线程。

## 留在仓库根（两版共用）

- `apps/`、`crates/`、`Cargo.toml` —— 全部 Rust 源码与构建配置
  （v2 新增 `crates/razer-config/` 与 `apps/razermate-headless/`，与 v1 共用设备层）
- `docs/DEVICE_REFERENCE.md`、`docs/PROTOCOL.md`、`docs/PLAN.md` —— 设备事实与协议规格
- `tests/golden_compare.py`、`tests/holdtest.py` —— 协议 / 设备级验证

## 为何不把 GUI 源码搬进 v1

`apps/razermate-agent`、`crates/razer-core` 等是 v2 的直接依赖，切开会造成大量路径改动而收益为零。
v1 只归档「这一版独有的东西」：发行包、UI 设计文档与截图、使用说明、UI 自动化脚本、打包脚本。
源码归属在每个版本的 README 里写明。
