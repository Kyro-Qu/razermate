# AGENTS.md — 项目约定

面向所有在本仓库工作的 AI 编码助手与人。无论你用什么工具，动手前请读完这一份。
文件位置固定在仓库根，文件名保持不变（`AGENTS.md` 是跨工具的通用约定文件名）。

---

## 1. 这是什么项目

给 **Razer Basilisk V3**（USB `1532:0099`）用的 Windows 小工具：检测当前前台程序，自动切换鼠标的按键映射与灯效。

技术核心一句话：**只写鼠标的 Profile 0（p0）临时层，永不碰板载 Flash**。
p0 是固件维护的 RAM 工作副本，写入立即生效、断电即失效，因此不磨损设备、不占用 5 个硬件槽位。

Rust workspace，二进制无 GUI / 无托盘（v2 起）。

---

## 2. 不可越界的硬约束

违反这些会直接损伤用户体验或设备，改代码前先确认没有踩到：

| 约束 | 说明 |
| --- | --- |
| **只写 p0** | 绝不写板载 5 个硬件槽（Flash）。任何新增下发逻辑都必须落在 p0。 |
| **不下发 DPI / 回报率** | `sensor._preference` 只是记录值，刻意不被下发。写 DPI 必须先切 driver mode 再切回，每次都会还原板载槽并覆盖整个 p0，导致设备反复重置。不要改回裸 `dpi` / `polling`。 |
| **`reapply_interval_ms` 保持 0** | 非 0 会周期性重写 p0，观感是滚轮阻尼与灯效持续抖动。 |
| **`RazerMate/profiles.json` 全仓唯一** | 它被 `include_str!` 编进二进制当默认模板。另建副本必然漂移。 |
| **只支持一个型号** | VID/PID 固定 `1532:0099`，不要为"兼容更多设备"放宽匹配。 |
| **按键绑定最后写** | 任何切槽/slot 操作都会覆盖整个 p0，所以 bindings 必须在下发序列末尾。 |

设备侧还有一批易踩的坑（`forward=5 / back=4` 而非 90/91、写完读要重试 BUSY、normal 模式写 DPI 静默失败等），
集中记录在 [`dev/docs/DEVICE_REFERENCE.md`](dev/docs/DEVICE_REFERENCE.md) 的「坑位清单」一节，动协议层前必读。

---

## 3. 目录约定

```
razermate/
├── RazerMate/     用户交付物：exe、启停脚本、配置、面向用户的文档
├── dev/           开发：crates/ · apps/ · docs/ · tests/ · target/
├── Cargo.toml · Cargo.lock · .cargo/ · .gitignore
└── AGENTS.md      这份文件
```

- 顶层**只有这两个目录**。新增内容按「用户要不要用」归入其一，不要在根下铺平级目录。
- 构建产物统一落在 `dev/target`（由 `.cargo/config.toml` 的 `target-dir` 指定），仓库根不应出现 `target/`。
- `RazerMate/*.exe` 不入库（`.gitignore`），脚本、配置、文档入库。
- `versions/` 那套四层嵌套（`versions/v2-headless/dist/RazerMate/`）已废除，不要恢复。

---

## 4. 文档分工：同一件事只有一个家

| 文档 | 写什么 | 不写什么 |
| --- | --- | --- |
| [`RazerMate/配置指南.md`](RazerMate/配置指南.md) | **当前配置是什么**：4 套配置何时生效、13 个按键的基准动作与各组差异、三个灯区的颜色亮度、每套的 DPI/回报率偏好、宏的按键序列 | 原理、语法、命令、schema、历史沿革、为什么这么设计 |
| [`RazerMate/配置参考.md`](RazerMate/配置参考.md) | 怎么改：动作串语法、可用值、匹配字段、常见改动、命令行、边界说明 | — |
| [`RazerMate/使用说明.md`](RazerMate/使用说明.md) | 面向最终用户的操作步骤 | — |
| [`dev/docs/`](dev/docs/) | 设备知识档案、报文规格、实机实验记录 | 应用层配置 |

**配置的权威来源是 `配置指南.md`，不是 `profiles.json`。**
讨论或修改配置时以指南为准，改完指南再同步 JSON 让程序生效；两者冲突时指南赢。

判断标准：**一行信息如果对「这个键现在绑的是什么」没有帮助，就不该出现在配置指南里。**

---

## 5. 构建、测试、交付

```bash
cargo build --release -p razermate-headless -p razerctl-rs   # 产物在 dev/target/release/
cargo test --workspace                                       # 应为 68/68 通过
```

交付到用户目录：

```bash
cp dev/target/release/razermate-headless.exe RazerMate/
cp dev/target/release/razerctl.exe RazerMate/
```

> 覆盖前必须先停掉运行中的实例（`RazerMate/stop.cmd`），Windows 不允许覆盖被占用的 exe。
> 交付后确认 `RazerMate/` 下的 exe 与 `dev/target/release/` 的 SHA256 一致，避免出现「改了代码但用户跑的还是旧程序」。

`profiles.json` 改动**不需要重新编译**：程序约 1 秒热加载。只有改 `.rs` 才需要重建。

---

## 6. 本机环境坑（Windows / Git Bash 混合）

- **`.cmd` 脚本必须 GBK + CRLF**。cmd 按控制台代码页解析，UTF-8 写入会整屏问号。改完用 `python -c "open('x.cmd','rb').read().decode('gbk')"` 验一遍。
- **cmd 脚本里先钉死 PATH**：`set "PATH=%SystemRoot%\System32;%SystemRoot%;%SystemRoot%\System32\Wbem"`。
  否则从 Git Bash 之类的混合环境启动时，脚本里的 `find` / `timeout` 会被 Unix 同名程序抢先匹配（现象：`find: '/I': No such file or directory`）。
- **延时统一写 `timeout /t N >nul 2>&1`**：stdin 被重定向时 `timeout` 会报「不支持输入重新定向」。
- **脚本的输出提示必须来自实测结果**：动作执行完重新查一次进程表，按真实状态报告成功/失败，不要背台词。
- **不要用 `git add -A`**。仓库旁曾有 12 GB 的构建目录，`-A` 会开始往索引里塞产物、命令被中断后还会漏提交根配置。
  显式列文件，或先 `git check-ignore` 确认忽略规则生效。被中断的 git 会留下 `.git/index.lock`，需手工删除。
- 长时间构建/测试请放后台跑，不要阻塞。

---

## 7. 协作方式

- **不要每改一点就推送**。攒到一个完整阶段（功能闭合、构建通过、文档同步）再 `git push`；中间过程留在本地。
- **结论必须有据**。禁止「大概是」「应该是」的猜测式判断；不确定就去读代码、跑测试、查日志、看设备实机状态。
  报告时把证据一起给出来（哪一行、哪条日志、哪个哈希相等）。
- **删文件前先证明它有备份**：对照磁盘、`git ls-files`、`origin/main` 三方一致后再动手。
- 交付物面向中文用户，用户可见的文档、提示、提交说明都用中文；代码标识符与命令保持英文。
