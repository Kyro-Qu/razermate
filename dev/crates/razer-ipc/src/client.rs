//! 管道客户端。GUI 用它跟守护进程说话；`razermate-agent --status` 之类也走这里。

use std::io::{BufRead, BufReader, Write};

use crate::{paths, OK_AUTHED};

#[derive(Debug)]
pub enum IpcError {
    /// 守护进程没在跑（连不上管道）。这是最常见的情况，不算错误。
    NotRunning,
    Io(std::io::Error),
    /// 认证失败或应答不符合预期。
    Protocol(String),
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcError::NotRunning => write!(f, "后台服务未运行"),
            IpcError::Io(e) => write!(f, "管道通信失败：{e}"),
            IpcError::Protocol(s) => write!(f, "应答异常：{s}"),
        }
    }
}

impl std::error::Error for IpcError {}

/// 读共享令牌。令牌文件不存在就返回空串——那时服务端也不会接受连接。
pub fn read_token() -> String {
    std::fs::read_to_string(paths::token_path())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// 发一条命令，返回服务端的应答（不含行尾）。
pub fn call(cmd: &str) -> Result<String, IpcError> {
    let token = read_token();
    if token.is_empty() {
        return Err(IpcError::NotRunning);
    }

    let stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(crate::PIPE_NAME)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => IpcError::NotRunning,
            _ => IpcError::Io(e),
        })?;

    let mut writer = stream.try_clone().map_err(IpcError::Io)?;
    let mut reader = BufReader::new(stream);

    writeln!(writer, "AUTH {token}").map_err(IpcError::Io)?;
    writer.flush().map_err(IpcError::Io)?;

    let mut line = String::new();
    reader.read_line(&mut line).map_err(IpcError::Io)?;
    if line.trim() != OK_AUTHED {
        return Err(IpcError::Protocol(line.trim().to_string()));
    }

    writeln!(writer, "{cmd}").map_err(IpcError::Io)?;
    writer.flush().map_err(IpcError::Io)?;

    line.clear();
    reader.read_line(&mut line).map_err(IpcError::Io)?;
    Ok(line.trim().to_string())
}

/// 守护进程是否在跑（发一次 PING）。
pub fn is_running() -> bool {
    matches!(call("PING"), Ok(ref s) if s.starts_with("OK"))
}

/// 解析 `OK key=value key=value` 形式的应答。
pub fn parse_fields(reply: &str) -> Vec<(String, String)> {
    reply
        .split_whitespace()
        .skip(1) // 跳过 OK
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub fn field(reply: &str, key: &str) -> Option<String> {
    parse_fields(reply)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}
