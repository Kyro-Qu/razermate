//! razermate GUI 与后台守护进程之间的本地契约。
//!
//! 三件事放在这里，是因为两边的理解必须逐字一致，各写一份迟早会跑偏：
//!   1. 配置目录与文件名；
//!   2. 自动切换规则的文件格式；
//!   3. 命名管道的名字与请求/应答文法。
//!
//! 协议很简单，够用就好：一行请求，一行应答，UTF-8。
//!
//! ```text
//! 客户端 → 服务端                     服务端 → 客户端
//! AUTH <32 位 hex>                    OK authed   |  ERR auth
//! PING                                OK pong
//! STATUS                              OK slot=4 mode=0 connected=1 paused=0 rules=3
//! SLOT 3                              OK slot 3
//! PAUSE 1                             OK paused
//! RELOAD                              OK rules 3
//! QUIT                                OK bye
//! ```
//!
//! 没通过 AUTH 的连接只能收到 ERR auth，别的什么都不做。

pub mod client;
pub mod known;
pub mod ledbak;
pub mod paths;
pub mod rules;
pub mod vslots;

/// 管道全名。加 `Local\` 前缀，只在本机登录会话内可见。
pub const PIPE_NAME: &str = r"\\.\pipe\razermate-agent";

/// 认证通过后服务端返回的第一行。
pub const OK_AUTHED: &str = "OK authed";
