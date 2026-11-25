# dev —— 开发与设备资料

仓库根只保留构建入口（`Cargo.toml`、`Cargo.lock`、`.cargo/`）和两个目录：
`RazerMate/`（用户交付物）与 `dev/`（这份）。源码、文档、测试、构建产物全部收在这里。

## 目录

| 路径 | 内容 |
| --- | --- |
| `crates/razer-protocol` | 报文编解码、CRC、按键表、宏编码，纯函数可单测 |
| `crates/razer-hid` | windows-sys 封装 HID 特性报告、BUSY 重试 |
| `crates/razer-core` | 读写封装、`apply_virtual_slot` 差量下发、读回校验、备份恢复 |
| `crates/razer-ipc` | `SlotConfig`、虚拟槽、规则、路径等跨 crate 契约 |
| `crates/razer-config` | `profiles.json` 解析、校验、合并编译（不含任何按键判断） |
| `apps/razermate-headless` | v2 后台程序：纯 JSON 驱动，无 GUI、无托盘 |
| `apps/razerctl-rs` | 命令行工具 `razerctl` |
| `docs/` | 设备知识档案、报文规格、实机实验记录 |
| `tests/` | 协议 / 设备级验证脚本（Python） |
| `target/` | 构建产物，由 `.cargo/config.toml` 的 `target-dir` 指到这里 |

## 构建

```bash
cargo build --release          # 产物落在 dev/target/release/
cargo test --workspace         # 68 个测试
```

产物同步到用户目录：

```bash
cp dev/target/release/razermate-headless.exe RazerMate/
cp dev/target/release/razerctl.exe RazerMate/
```

> 覆盖前必须先停掉正在运行的实例（运行 `RazerMate/stop.cmd`），Windows 不允许覆盖被占用的 exe。

## 版本划分

当前只交付 **v2 · 配置驱动**。v1 的交付物与源码都已移出仓库。

| 版本 | 交付形态 | 状态 |
| --- | --- | --- |
| v2 · 配置驱动 | `RazerMate/` 下的 `profiles.json` + 无 GUI 后台程序 | 当前唯一版本 |
| v1 · 上位机 GUI | 曾为 Slint 控制面板 + 托盘后台 + CLI | 已冻结归档，仅存于 git 历史 |

v1 源码回查：`git show 784cb5b:apps/razermate/ui/app.slint`
v1 可执行文件：`DeviceManagement/archive/razermate-v1-gui-2026-11-25.zip`

## 源码与用户目录的耦合点

`crates/razer-config/src/lib.rs` 用 `include_str!("../../../../RazerMate/profiles.json")`
把用户目录里的配置编进二进制，作为首次运行的默认模板。

所以：**`RazerMate/profiles.json` 只有这一份**，不要在别处建副本。改动它会影响下一次构建的内嵌模板。
