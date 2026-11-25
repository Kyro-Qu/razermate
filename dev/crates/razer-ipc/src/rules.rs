//! 自动切换规则。
//!
//! 用带注释的纯文本而不是 JSON：这个文件用户会手改，`chrome.exe=2` 比
//! `{"exe":"chrome.exe","slot":2}` 好读也少出错，还不需要引一个 JSON 解析器。
//!
//! ```text
//! # RazerMate 自动切换规则
//! enabled=1
//! poll_ms=800
//! default=0
//! chrome.exe=2
//! kicad.exe=3
//! ```
//!
//! `default` 与 `slot=0` 都表示「不切换，保持当前板载槽」。

use std::io;
use std::path::Path;

/// 一条「前台程序 → 板载槽」规则。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// 可执行文件名，小写，例如 `chrome.exe`。
    pub exe: String,
    /// 1–5 为目标槽；0 表示不切换。
    pub slot: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rules {
    pub enabled: bool,
    pub poll_ms: u32,
    pub default_slot: u8,
    pub rules: Vec<Rule>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            enabled: true,
            poll_ms: 800,
            default_slot: 0,
            rules: Vec::new(),
        }
    }
}

impl Rules {
    pub fn parse(text: &str) -> Self {
        let mut out = Rules::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || value.is_empty() {
                continue;
            }
            match key.to_ascii_lowercase().as_str() {
                "enabled" => out.enabled = value != "0" && !value.eq_ignore_ascii_case("false"),
                "poll_ms" => {
                    if let Ok(v) = value.parse::<u32>() {
                        out.poll_ms = v.clamp(200, 10_000);
                    }
                }
                "default" => {
                    if let Ok(v) = value.parse::<u8>() {
                        out.default_slot = clamp_slot(v);
                    }
                }
                other => {
                    let exe = other.to_string();
                    if let Ok(v) = value.parse::<u8>() {
                        // 同一个 exe 出现多次时后者覆盖前者，手改文件时符合直觉。
                        let slot = clamp_slot(v);
                        match out.rules.iter_mut().find(|r| r.exe == exe) {
                            Some(r) => r.slot = slot,
                            None => out.rules.push(Rule { exe, slot }),
                        }
                    }
                }
            }
        }
        out
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str("# RazerMate 自动切换规则\n");
        s.push_str("# 前台程序名 = 使用的板载槽（1-5）。default=0 表示不切换。\n");
        s.push_str(&format!("enabled={}\n", u8::from(self.enabled)));
        s.push_str(&format!("poll_ms={}\n", self.poll_ms));
        s.push_str(&format!("default={}\n", self.default_slot));
        for r in &self.rules {
            s.push_str(&format!("{}={}\n", r.exe, r.slot));
        }
        s
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(_) => Rules::default(),
        }
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, self.to_text())
    }

    /// 查一个前台程序该用哪个槽。`exe` 大小写不敏感；没有匹配时用 `default_slot`。
    pub fn slot_for(&self, exe: &str) -> u8 {
        let needle = exe.to_ascii_lowercase();
        if let Some(r) = self.rules.iter().find(|r| r.exe == needle) {
            return r.slot;
        }
        self.default_slot
    }

    /// 设置一条规则。`slot = 0` 表示删除该条。
    pub fn set_rule(&mut self, exe: &str, slot: u8) {
        let exe = exe.to_ascii_lowercase();
        let slot = clamp_slot(slot);
        match self.rules.iter_mut().find(|r| r.exe == exe) {
            Some(r) => {
                if slot == 0 {
                    self.rules.retain(|x| x.exe != exe);
                } else {
                    r.slot = slot;
                }
            }
            None if slot != 0 => self.rules.push(Rule { exe, slot }),
            None => {}
        }
    }
}

fn clamp_slot(v: u8) -> u8 {
    if v <= 5 {
        v
    } else {
        0
    }
}

/// 从完整路径里取出用于匹配的程序名（小写）。
pub fn exe_name_of(path: &str) -> String {
    let last = path.rsplit(['\\', '/']).next().unwrap_or(path);
    last.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_round_trips() {
        let text = "# 注释\nenabled=1\npoll_ms=500\ndefault=1\nchrome.exe=2\nKiCad.exe=3\n";
        let r = Rules::parse(text);
        assert!(r.enabled);
        assert_eq!(r.poll_ms, 500);
        assert_eq!(r.default_slot, 1);
        assert_eq!(r.rules.len(), 2);
        assert_eq!(r.rules[1].exe, "kicad.exe");

        let again = Rules::parse(&r.to_text());
        assert_eq!(again, r);
    }

    #[test]
    fn slot_lookup_is_case_insensitive() {
        let r = Rules::parse("chrome.exe=2\ndefault=0\n");
        assert_eq!(r.slot_for("chrome.exe"), 2);
        assert_eq!(r.slot_for("CHROME.EXE"), 2);
        assert_eq!(r.slot_for("notepad.exe"), 0);
    }

    #[test]
    fn polls_are_clamped_to_sane_range() {
        let r = Rules::parse("poll_ms=1\n");
        assert_eq!(r.poll_ms, 200);
        let r = Rules::parse("poll_ms=999999\n");
        assert_eq!(r.poll_ms, 10_000);
    }

    #[test]
    fn set_rule_adds_updates_removes() {
        let mut r = Rules::default();
        r.set_rule("chrome.exe", 2);
        assert_eq!(r.slot_for("chrome.exe"), 2);
        r.set_rule("chrome.exe", 3);
        assert_eq!(r.slot_for("chrome.exe"), 3);
        assert_eq!(r.rules.len(), 1);
        r.set_rule("chrome.exe", 0);
        assert_eq!(r.rules.len(), 0);
    }

    #[test]
    fn exe_name_extraction() {
        assert_eq!(exe_name_of(r"C:\Program Files\Google\chrome.exe"), "chrome.exe");
        assert_eq!(exe_name_of("kicad.exe"), "kicad.exe");
    }
}
