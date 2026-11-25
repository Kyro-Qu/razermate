//! 预览灯带之前，把原来的板载灯效记到一个小文件里。
//!
//! `set_custom` 是运行时效果，不会写进鼠标 Flash，但它会把当前显示顶掉。
//! 用户点了「彩虹」之后可能关掉程序、也可能隔天才想起来要还原，所以备份
//! 不能只放在进程内存里。格式就一行六个十进制数，坏了就当没有。

use crate::paths;
use std::io;

/// `effect mode speed r g b`
pub fn save(effect: u8, mode: u8, speed: u8, rgb: (u8, u8, u8)) -> io::Result<()> {
    let text = format!(
        "{} {} {} {} {} {}\n",
        effect, mode, speed, rgb.0, rgb.1, rgb.2
    );
    if let Some(dir) = paths::led_backup_path().parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(paths::led_backup_path(), text)
}

pub fn load() -> Option<(u8, u8, u8, (u8, u8, u8))> {
    let text = std::fs::read_to_string(paths::led_backup_path()).ok()?;
    let v: Vec<u8> = text
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if v.len() < 6 {
        return None;
    }
    Some((v[0], v[1], v[2], (v[3], v[4], v[5])))
}

pub fn clear() {
    let _ = std::fs::remove_file(paths::led_backup_path());
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_a_six_number_line() {
        let v: Vec<u8> = "3 0 64 0 0 255\n"
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        assert_eq!(v, vec![3, 0, 64, 0, 0, 255]);
    }

    #[test]
    fn rejects_a_truncated_line() {
        let v: Vec<u8> = "3 0 64 0 0"
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        assert!(v.len() < 6);
    }
}
