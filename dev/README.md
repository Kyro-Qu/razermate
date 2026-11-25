# RazerMate 版本划分

本仓库当前只交付 **v2 · 配置驱动**（`v2-headless/`）。v1 的交付物与源码都已移出仓库，
需要查阅时走 git 历史，例如 `git show 784cb5b:versions/v1-gui/README.md`。

| 版本 | 目录 | 交付形态 | 状态 |
| --- | --- | --- | --- |
| v2 · 配置驱动 | `v2-headless/` | `profiles.json` + 无 GUI 后台小程序（开机自启 / 纯在线模式 / 不碰板载配置） | 当前唯一版本 |
| v1 · 上位机 GUI | 已移出仓库 | 曾为 Slint 控制面板 + 托盘后台 + CLI | 已冻结归档，仅存于 git 历史 |

v2 的 `profiles.json` 是唯一事实源，配套 `PROFILES_GUIDE.md` 给 AI / Agent 读：
用户快捷键偏好、13 个按键的当前映射、每组配置与灯光明细。
后台程序零硬编码 —— 按钮 ID 表、动作串、匹配规则全在 JSON，改配置不用重新编译。

## 设备与协议资料（长期保留）

移除 v1 时**没有**删除任何设备知识。这些才是后续继续控制这只鼠标的依据：

| 文件 | 内容 |
| --- | --- |
| `docs/DEVICE_REFERENCE.md` | 设备知识档案：USB 接口结构、模式、槽位与 p0、按键 ID、宏、灯效、Flash 模型、已知坑清单 |
| `docs/PROTOCOL.md` | 报文规格：90 字节结构、CRC、六个命令族、宏编码、p0 语义、实现检查清单 |
| `docs/PLAN.md` | 实施计划与实机实验记录：p0 六组验证、Flash 空间语义、宏触发方式、时序约束 |

## 留在仓库根（源码与验证）

- `apps/`、`crates/`、`Cargo.toml` —— 全部 Rust 源码与构建配置
  （v2 新增 `crates/razer-config/` 与 `apps/razermate-headless/`，与旧版共用设备层）
- `tests/golden_compare.py`、`tests/holdtest.py` —— 协议 / 设备级验证

## 源码现状

仓库只保留 v2 的源码：`crates/`（协议 / 设备层 / 配置层）+ `apps/razerctl-rs`、`apps/razermate-headless`。
v1 的 Slint 界面（`apps/razermate/`）与托盘后台（`apps/razermate-agent/`）源码已移出仓库，
需要时走 git 历史，例如 `git show 784cb5b:apps/razermate/ui/app.slint`；
v1 的可执行文件在 `DeviceManagement/archive/razermate-v1-gui-2026-11-25.zip`。
