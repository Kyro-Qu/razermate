//! 已知宏 ID 的登记表。
//!
//! 为什么需要这个东西：设备**只**给一个宏的总数（`cls 0x06 / cmd 0x80`），
//! 没有"列出所有 ID"的命令。ID 是 u16，从 1 到 65535 全扫一遍要几十秒，
//! 界面每 3 秒刷新一次根本不能这么干。
//!
//! 所以：写入时登记、删除时注销，读的时候只查登记表里的 ID（并且顺手校验
//! 那些 ID 还在不在，被别的手段删掉的会自动清掉）。用户如果从别处（旧版
//! Synapse、别人给的配置）带进来宏，用扫描功能发现一次即可。

use crate::paths;

/// 读登记表。文件不存在或内容坏了都返回空表，不报错。
pub fn load() -> Vec<u16> {
    let Ok(text) = std::fs::read_to_string(paths::known_macros_path()) else {
        return Vec::new();
    };
    let mut out: Vec<u16> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| u16::from_str_radix(l.trim_start_matches("0x"), 16).ok())
        .filter(|id| *id != 0)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn save(ids: &[u16]) -> std::io::Result<()> {
    let mut text = String::from("# RazerMate 已知宏 ID（自动维护，可手工删改）\n");
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    for id in sorted {
        text.push_str(&format!("0x{id:04X}\n"));
    }
    std::fs::write(paths::known_macros_path(), text)
}

pub fn remember(id: u16) {
    if id == 0 {
        return;
    }
    let mut ids = load();
    if !ids.contains(&id) {
        ids.push(id);
        let _ = save(&ids);
    }
}

pub fn forget(id: u16) {
    let mut ids = load();
    if let Some(pos) = ids.iter().position(|x| *x == id) {
        ids.remove(pos);
        let _ = save(&ids);
    }
}

/// 批量记住（扫描用）。
pub fn remember_all(found: &[u16]) {
    if found.is_empty() {
        return;
    }
    let mut ids = load();
    let before = ids.len();
    ids.extend_from_slice(found);
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != before {
        let _ = save(&ids);
    }
}

/// 只保留仍然存在的：`exists` 由调用方拿设备去问。
pub fn retain_existing<F>(exists: F) -> Vec<u16>
where
    F: Fn(u16) -> bool,
{
    let ids = load();
    let live: Vec<u16> = ids.iter().copied().filter(|id| exists(*id)).collect();
    if live.len() != ids.len() {
        let _ = save(&live);
    }
    live
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<u16> {
        let mut out: Vec<u16> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| u16::from_str_radix(l.trim_start_matches("0x"), 16).ok())
            .filter(|id| *id != 0)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    #[test]
    fn parses_hex_lines_and_skips_noise() {
        let ids = parse("# 注释\n0x0101\n\n0x0202\nzzz\n0x0000\n0x0101\n");
        assert_eq!(ids, vec![0x0101, 0x0202]);
    }

    #[test]
    fn path_lives_next_to_the_rest_of_the_config() {
        use std::path::Path;
        let p = paths::known_macros_path();
        assert_eq!(p.parent(), Some(Path::new(&paths::config_dir()).as_ref()));
        assert!(p.to_string_lossy().ends_with("macros.conf"));
    }
}
