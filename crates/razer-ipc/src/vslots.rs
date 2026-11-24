//! 虚拟槽：无限数量的软件配置层，套用到鼠标的 p0（direct profile）。
//!
//! 内存设计（对应「在线模式后台尽可能省内存」的目标）：
//!   * 常驻 agent 内存的只有 [`Index`] —— 每个槽仅 id / 名字 / exe 匹配串，
//!     一条规则几十字节，几百个槽也只有几十 KB；
//!   * 槽的**完整配置**（按键、灯效、亮度、DPI）单独存文件，只在命中的那一刻
//!     读进来、套用完立刻丢弃，绝不让无限个槽的全文常驻；
//!   * 用纯文本而非 JSON：用户会手改，`bind.4=key:ctrl+w` 比嵌套对象好读好 diff，
//!     也不需要为后台进程引入一个 JSON 解析器（每多一个依赖都是常驻内存）。
//!
//! 索引文件格式（vslots.conf）：
//! ```text
//! # RazerMate 虚拟槽索引
//! enabled=1
//! default=办公
//! # 格式：id | 显示名 | exe1,exe2,exe3
//! 1|办公|chrome.exe,code.exe
//! 2|游戏|game.exe,steam.exe
//! ```
//!
//! 单个槽格式（vslots/<id>.vslot）：
//! ```text
//! # 虚拟槽：办公
//! # 按键绑定：按钮ID=动作串（与 razerctl bind 的写法一致）
//! bind.4=key:ctrl+w
//! bind.5=macro:0x0101x1
//! # 灯效：region.effect / region.rgb / region.mode / region.speed
//! led.1.effect=static
//! led.1.rgb=44d62c
//! led.10.effect=wave
//! led.10.mode=1
//! led.10.speed=5
//! # 亮度：region=brightness(0..255 原始值)
//! bright.1=255
//! bright.10=180
//! # 传感器：dpi / dpi_stage / polling
//! dpi=2700
//! polling=1000
//! ```

use std::io;
use std::path::Path;

use crate::paths;

/// 索引里的一条轻量元数据。常驻内存的就是这些。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotMeta {
    /// 槽 ID，同时是配置文件名（限制为安全字符）。
    pub id: String,
    /// 界面显示名。
    pub name: String,
    /// 匹配的前台 exe 名（小写）。命中任意一个即套用本槽。
    pub exes: Vec<String>,
}

/// 虚拟槽索引：常驻 agent 的全部内容。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    pub enabled: bool,
    /// 没有匹配规则时使用的默认槽 ID；空串表示不套用。
    pub default_id: String,
    pub slots: Vec<SlotMeta>,
}

impl Default for Index {
    fn default() -> Self {
        Index { enabled: true, default_id: String::new(), slots: Vec::new() }
    }
}

impl Index {
    pub fn parse(text: &str) -> Self {
        let mut out = Index::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                let value = value.trim();
                match key.as_str() {
                    "enabled" => out.enabled = value != "0" && !value.eq_ignore_ascii_case("false"),
                    "default" => out.default_id = safe_id(value),
                    _ => {}
                }
                continue;
            }
            // 槽行：id | name | exe,exe,...
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() < 2 {
                continue;
            }
            let id = safe_id(parts[0]);
            if id.is_empty() {
                continue;
            }
            let name = parts[1].to_string();
            let exes = parts
                .get(2)
                .map(|s| s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(|x| x.to_ascii_lowercase()).collect())
                .unwrap_or_default();
            out.slots.push(SlotMeta { id, name, exes });
        }
        out
    }

    pub fn to_text(&self) -> String {
        let mut s = String::from("# RazerMate 虚拟槽索引（在线模式）\n");
        s.push_str("# enabled=1 开启在线自动切换；default=没有匹配时用的槽（留空则不套用）\n");
        s.push_str("# 格式：id | 显示名 | exe1,exe2,exe3\n");
        s.push_str(&format!("enabled={}\n", u8::from(self.enabled)));
        s.push_str(&format!("default={}\n", self.default_id));
        for slot in &self.slots {
            s.push_str(&format!("{}|{}|{}\n", slot.id, slot.name, slot.exes.join(",")));
        }
        s
    }

    pub fn load() -> Self {
        Self::load_from(&paths::vslots_index_path())
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(_) => Index::default(),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&paths::vslots_index_path())
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_text())
    }

    /// 查某个 exe 该用哪个槽 ID。大小写不敏感。
    pub fn slot_id_for(&self, exe: &str) -> String {
        let needle = exe.to_ascii_lowercase();
        for slot in &self.slots {
            if slot.exes.iter().any(|x| x == &needle) {
                return slot.id.clone();
            }
        }
        self.default_id.clone()
    }

    pub fn find(&self, id: &str) -> Option<&SlotMeta> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// 新增或更新一个槽的元数据，返回是否是新增。
    pub fn upsert(&mut self, meta: SlotMeta) -> bool {
        match self.slots.iter_mut().find(|s| s.id == meta.id) {
            Some(s) => { *s = meta; false }
            None => { self.slots.push(meta); true }
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.slots.len();
        self.slots.retain(|s| s.id != id);
        self.slots.len() != before
    }
}

/// 单条灯效设置。三种板载效果可持久化：static / spectrum / wave。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedSetting {
    pub region: u8,
    pub effect: String,
    pub rgb: Option<(u8, u8, u8)>,
    pub mode: u8,
    pub speed: u8,
}

/// 一个虚拟槽的**完整配置**。只在套用那一刻载入内存。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SlotConfig {
    pub name: String,
    /// 按钮 ID → 动作串（parse_action 能识别的写法）。
    pub bindings: Vec<(u8, String)>,
    pub leds: Vec<LedSetting>,
    /// region → 亮度原始值 0..255。
    pub brightness: Vec<(u8, u8)>,
    pub dpi: Option<(u16, u16)>,
    pub polling: Option<u16>,
}

impl SlotConfig {
    pub fn parse(text: &str) -> Self {
        let mut out = SlotConfig::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim();
            let value = value.trim();
            let key_lower = key.to_ascii_lowercase();

            if key_lower == "name" {
                out.name = value.to_string();
                continue;
            }
            if key_lower == "dpi" {
                if let Ok(v) = value.parse::<u16>() {
                    out.dpi = Some((v, v));
                } else if let Some((x, y)) = value.split_once(',') {
                    if let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse()) {
                        out.dpi = Some((x, y));
                    }
                }
                continue;
            }
            if key_lower == "polling" {
                if let Ok(v) = value.parse::<u16>() {
                    out.polling = Some(v);
                }
                continue;
            }

            let seg: Vec<&str> = key_lower.split('.').collect();
            match seg.as_slice() {
                ["bind", id] => {
                    if let Ok(button) = id.parse::<u8>() {
                        out.bindings.push((button, value.to_string()));
                    }
                }
                ["bright", region] => {
                    if let (Ok(region), Ok(v)) = (region.parse::<u8>(), value.parse::<u8>()) {
                        out.brightness.push((region, v));
                    }
                }
                ["led", region, field] => {
                    if let Ok(region) = region.parse::<u8>() {
                        let idx = out.leds.iter().position(|l| l.region == region).unwrap_or_else(|| {
                            out.leds.push(LedSetting {
                                region,
                                effect: "static".to_string(),
                                rgb: None,
                                mode: 0,
                                speed: 0,
                            });
                            out.leds.len() - 1
                        });
                        let led = &mut out.leds[idx];
                        match *field {
                            "effect" => led.effect = value.to_ascii_lowercase(),
                            "rgb" => led.rgb = parse_rgb(value),
                            "mode" => led.mode = value.parse().unwrap_or(0),
                            "speed" => led.speed = value.parse().unwrap_or(0),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn to_text(&self) -> String {
        // name 必须是真正的键值行，不能写成注释——否则 round-trip 会把它读丢。
        let mut s = format!("name={}\n", self.name);
        s.push_str("# 按键：bind.<按钮ID>=<动作串>（如 key:ctrl+w / macro:0x0101x1 / dpi:up）\n");
        let mut binds: Vec<_> = self.bindings.iter().collect();
        binds.sort_by_key(|(id, _)| *id);
        for (id, action) in binds {
            s.push_str(&format!("bind.{id}={action}\n"));
        }
        s.push_str("# 灯效：static / spectrum / wave；rgb 为 6 位十六进制\n");
        let mut leds: Vec<_> = self.leds.iter().collect();
        leds.sort_by_key(|l| l.region);
        for led in leds {
            s.push_str(&format!("led.{}.effect={}\n", led.region, led.effect));
            if let Some((r, g, b)) = led.rgb {
                s.push_str(&format!("led.{}.rgb={r:02x}{g:02x}{b:02x}\n", led.region));
            }
            s.push_str(&format!("led.{}.mode={}\n", led.region, led.mode));
            s.push_str(&format!("led.{}.speed={}\n", led.region, led.speed));
        }
        s.push_str("# 亮度：0..255 原始值\n");
        let mut brights: Vec<_> = self.brightness.iter().collect();
        brights.sort_by_key(|(r, _)| *r);
        for (region, value) in brights {
            s.push_str(&format!("bright.{region}={value}\n"));
        }
        if let Some((x, y)) = self.dpi {
            if x == y {
                s.push_str(&format!("dpi={x}\n"));
            } else {
                s.push_str(&format!("dpi={x},{y}\n"));
            }
        }
        if let Some(hz) = self.polling {
            s.push_str(&format!("polling={hz}\n"));
        }
        s
    }

    /// 按需读取一个槽的完整配置。返回 None 表示文件不存在。
    pub fn load(id: &str) -> Option<Self> {
        Self::load_from(&paths::vslot_path(id))
    }

    pub fn load_from(path: &Path) -> Option<Self> {
        std::fs::read_to_string(path).ok().map(|text| Self::parse(&text))
    }

    pub fn save(&self, id: &str) -> io::Result<()> {
        self.save_to(&paths::vslot_path(id))
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_text())
    }
}

fn parse_rgb(value: &str) -> Option<(u8, u8, u8)> {
    let v = value.trim().trim_start_matches('#');
    if v.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&v[0..2], 16).ok()?;
    let g = u8::from_str_radix(&v[2..4], 16).ok()?;
    let b = u8::from_str_radix(&v[4..6], 16).ok()?;
    Some((r, g, b))
}

/// 只保留安全字符作为槽 ID，避免路径穿越。
pub fn safe_id(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_round_trips() {
        let text = "# 注释\nenabled=1\ndefault=work\n1|办公|chrome.exe,code.exe\n2|游戏|game.exe\n";
        let idx = Index::parse(text);
        assert!(idx.enabled);
        assert_eq!(idx.default_id, "work");
        assert_eq!(idx.slots.len(), 2);
        assert_eq!(idx.slots[0].name, "办公");
        assert_eq!(idx.slots[0].exes, vec!["chrome.exe", "code.exe"]);
        let again = Index::parse(&idx.to_text());
        assert_eq!(again, idx);
    }

    #[test]
    fn index_lookup_is_case_insensitive_and_defaults() {
        let idx = Index::parse("default=base\n1|办公|CHROME.EXE\n");
        assert_eq!(idx.slot_id_for("chrome.exe"), "1");
        assert_eq!(idx.slot_id_for("Chrome.EXE"), "1");
        assert_eq!(idx.slot_id_for("notepad.exe"), "base");
    }

    #[test]
    fn slot_config_round_trips() {
        let text = "# 测试\nname=办公\nbind.4=key:ctrl+w\nbind.5=macro:0x0101x1\n\
                    led.1.effect=static\nled.1.rgb=44d62c\n\
                    led.10.effect=wave\nled.10.mode=1\nled.10.speed=5\n\
                    bright.10=180\ndpi=2700\npolling=1000\n";
        let cfg = SlotConfig::parse(text);
        assert_eq!(cfg.name, "办公");
        assert_eq!(cfg.bindings.len(), 2);
        assert_eq!(cfg.bindings[0], (4, "key:ctrl+w".to_string()));
        assert_eq!(cfg.leds.len(), 2);
        assert_eq!(cfg.leds[0].rgb, Some((0x44, 0xd6, 0x2c)));
        assert_eq!(cfg.leds[1].speed, 5);
        assert_eq!(cfg.brightness, vec![(10, 180)]);
        assert_eq!(cfg.dpi, Some((2700, 2700)));
        assert_eq!(cfg.polling, Some(1000));
        let again = SlotConfig::parse(&cfg.to_text());
        assert_eq!(again, cfg);
    }

    #[test]
    fn safe_id_blocks_path_traversal() {
        assert_eq!(safe_id("..\\evil"), "___evil");
        assert_eq!(safe_id("slot-1"), "slot-1");
    }
}
