//! 配置文件位置。统一放 `%LOCALAPPDATA%\RazerMate\`，不放程序目录——
//! 程序目录可能在 Program Files 下，普通用户没有写权限。

use std::path::PathBuf;

/// 配置目录，不存在时尽量创建。创建失败也不 panic：调用方会退化成默认值。
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("RazerMate");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 自动切换规则。
pub fn rules_path() -> PathBuf {
    config_dir().join("rules.conf")
}

/// 守护进程的共享令牌。
pub fn token_path() -> PathBuf {
    config_dir().join("agent.token")
}

/// 守护进程日志。出问题时有据可查，总比"它就是不工作"强。
pub fn log_path() -> PathBuf {
    config_dir().join("agent.log")
}

/// 已知宏 ID 登记表。设备不提供"列出全部宏"的命令，见 `known.rs` 的说明。
pub fn known_macros_path() -> PathBuf {
    config_dir().join("macros.conf")
}

/// 灯带预览前的板载灯效备份，见 `ledbak.rs`。
pub fn led_backup_path() -> PathBuf {
    config_dir().join("led-preview.bak")
}

/// 配置备份 / 恢复时导出的快照。
pub fn backup_dir() -> PathBuf {
    config_dir().join("backups")
}

/// 虚拟槽索引：只含每个槽的名字与 exe 匹配规则，轻量、常驻 agent 内存。
/// 槽的完整配置不在这里，而在 `virtual_slots_dir()` 下按需读取。
pub fn vslots_index_path() -> PathBuf {
    config_dir().join("vslots.conf")
}

/// 虚拟槽完整配置的存放目录，一个槽一个文件（文件名就是槽 ID）。
pub fn virtual_slots_dir() -> PathBuf {
    config_dir().join("vslots")
}

/// 某个虚拟槽的完整配置文件。
pub fn vslot_path(id: &str) -> PathBuf {
    virtual_slots_dir().join(format!("{id}.vslot"))
}
