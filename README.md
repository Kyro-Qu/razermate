# RazerMate

给 **Razer Basilisk V3**（`1532:0099`）用的 Windows 小工具：按当前前台程序自动切换按键映射和灯效。

技术核心只有一句：**只往鼠标的 Profile 0 临时层写配置，永不碰板载 Flash**，所以不会磨损设备、也不占用 5 个硬件槽。

---

## 我是用户，只想用

打开 **[`RazerMate/`](RazerMate/)** 这一个文件夹就够了，里面是：

```
RazerMate/
├── start.cmd                 双击启动（会问你是否确认）
├── stop.cmd                  双击关闭（会问你是否确认）
├── razermate-headless.exe    后台程序本体
├── razerctl.exe              命令行调试工具
├── profiles.json             配置：改它就能改行为，改完 1 秒生效
├── profiles.schema.json      配置的格式定义（编辑器补全用）
├── 配置指南.md               怎么改配置
└── 使用说明.md               先看这个
```

第一次用，读 [`RazerMate/使用说明.md`](RazerMate/使用说明.md)，然后双击 `start.cmd`。

---

## 我是开发者，要改代码

所有开发相关的东西都在 **[`dev/`](dev/)** 里，仓库根只有 `Cargo.toml`、`Cargo.lock`、`.cargo/` 这几个构建入口：

```
dev/
├── crates/       协议 / HID / 设备层 / IPC / 配置层
├── apps/         razermate-headless（v2 后台）· razerctl-rs（CLI）
├── docs/         设备知识档案、报文规格、实机实验记录
├── tests/        协议与设备级验证脚本
└── target/       构建产物（12 GB 级，已忽略入库）
```

```bash
cargo build --release     # 产物在 dev/target/release/
cargo test --workspace    # 68 个测试
```

构建产物通过 `.cargo/config.toml` 里的 `target-dir` 落在 `dev/target`，所以仓库根不会冒出 `target/`。

构建完成后，把 `dev/target/release/` 下的两个 exe 复制到 `RazerMate/` 即可（`RazerMate/*.exe` 不入库）。

---

## 为什么 `profiles.json` 在用户目录

它是**唯一事实源**，并且在编译期用 `include_str!` 嵌进二进制当首次运行的默认模板。
所以它必须只有一份——放在用户目录，程序读它、也嵌它。不要另建副本，否则两份会漂移。

---

## 之前的目录结构

`versions/v2-headless/dist/RazerMate/` 那种四层嵌套已经废除，`versions/` 目录已移除。
v1（Slint 界面版）已归档，源码查 `git log`，可执行文件在
`DeviceManagement/archive/razermate-v1-gui-2026-11-25.zip`。
