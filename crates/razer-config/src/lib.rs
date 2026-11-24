//! RazerMate v2 配置层：把 `profiles.json` 解析、校验并编译成设备层对象。
//!
//! 设计铁律：**本文件不认识任何按键**。没有 `if button == "left"`，没有
//! 「这六个键固定」之类的判断。按钮 ID 表、每个键映射什么动作、灯区编号、
//! 匹配规则、服务策略全部来自 JSON，后台程序只是通用解释器。
//! 因此换鼠标、改右键、加新功能，都只需改 JSON，不用重新编译。
//!
//! 解析完成后 `serde_json::Value` 树立即释放，常驻的只有下面这些紧凑结构。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use serde::Deserialize;

pub use razer_ipc::vslots::SlotConfig;
use razer_ipc::vslots::LedSetting;

/// 编译进二进制的默认配置，唯一事实源就是仓库里的 v2 样例。
/// 首次运行会把它写到 `%LOCALAPPDATA%\RazerMate\profiles.json`。
pub const DEFAULT_PROFILES_JSON: &str =
    include_str!("../../../versions/v2-headless/profiles.json");

pub const SCHEMA_VERSION: u32 = 3;

// ────────────────────────────── 数据结构 ──────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub schema: u32,
    pub device: Device,
    #[serde(default)]
    pub service: Service,
    #[serde(default)]
    pub macros: HashMap<String, MacroDef>,
    pub base: Layer,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub vid: u16,
    #[serde(default)]
    pub pid: u16,
    /// 名称 → 物理按钮。程序只从这里查 ID，不内置按钮表。
    pub buttons: HashMap<String, ButtonDef>,
    /// 灯区名称 → region ID。
    pub led_regions: HashMap<String, u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ButtonDef {
    pub id: u8,
    #[serde(default)]
    pub zh: String,
    /// 恢复出厂行为时要写的动作串。
    pub native: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BindingDef {
    pub action: String,
    // equivalent / aliases / note / locked 都是给人看的，程序故意不声明，
    // serde 会静默忽略，JSON 里随便加字段也不会让后台程序崩。
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layer {
    pub bindings: HashMap<String, BindingDef>,
    #[serde(default)]
    pub sensor: Sensor,
    #[serde(default)]
    pub lighting: HashMap<String, LightDef>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PartialLayer {
    #[serde(default)]
    pub bindings: Option<HashMap<String, BindingDef>>,
    #[serde(default)]
    pub sensor: Option<Sensor>,
    #[serde(default)]
    pub lighting: Option<HashMap<String, LightDef>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Sensor {
    #[serde(default)]
    pub dpi: Option<Dpi>,
    #[serde(default)]
    pub polling: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Dpi {
    Both(u16),
    Split(Vec<u16>),
}

impl Dpi {
    pub fn pair(&self) -> Option<(u16, u16)> {
        match self {
            Dpi::Both(v) => Some((*v, *v)),
            Dpi::Split(v) if v.len() == 2 => Some((v[0], v[1])),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LightDef {
    pub effect: String,
    #[serde(default)]
    pub rgb: Option<String>,
    #[serde(default)]
    pub mode: Option<u8>,
    #[serde(default)]
    pub speed: Option<u8>,
    pub brightness: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, rename = "match")]
    pub match_rule: Option<MatchDef>,
    #[serde(default)]
    pub overrides: PartialLayer,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MatchDef {
    #[serde(default)]
    pub exe: Vec<String>,
    #[serde(default)]
    pub title_contains: Vec<String>,
    #[serde(default)]
    pub path_contains: Vec<String>,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MacroDef {
    pub steps: Vec<String>,
    #[serde(default)]
    pub id: Option<u16>,
    #[serde(default)]
    pub trigger: Option<String>,
}

// ── service：只声明程序真正会读的项，其余配置原样忽略 ──

fn default_true() -> bool { true }
fn default_fallback() -> String { "global".into() }
fn default_poll_ms() -> u64 { 800 }
fn default_watch_ms() -> u64 { 400 }
fn default_debounce_ms() -> u64 { 250 }
fn default_match_by() -> Vec<String> {
    vec!["foreground_exe".into(), "window_title".into()]
}
fn default_on_error() -> String { "keep_last".into() }

#[derive(Debug, Clone, Deserialize)]
pub struct Service {
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default = "default_true")]
    pub background: bool,
    #[serde(default = "default_true")]
    pub headless: bool,
    #[serde(default)]
    pub tray_icon: bool,
    #[serde(default)]
    pub mode: String,
    #[serde(default = "default_true")]
    pub single_instance: bool,
    #[serde(default = "default_match_by")]
    pub match_by: Vec<String>,
    #[serde(default = "default_fallback")]
    pub fallback: String,
    #[serde(default = "default_on_error")]
    pub on_error: String,
    #[serde(default = "default_poll_ms")]
    pub poll_ms: u64,
    #[serde(default = "default_watch_ms")]
    pub config_watch_ms: u64,
    #[serde(default = "default_debounce_ms")]
    pub config_debounce_ms: u64,
    #[serde(default)]
    pub reapply_on_device_arrival: bool,
    #[serde(default)]
    pub reapply_on_wake: bool,
    #[serde(default)]
    pub reapply_interval_ms: u64,
    #[serde(default)]
    pub restore_on_exit: bool,
    #[serde(default)]
    pub p0: P0Policy,
    #[serde(default)]
    pub macros: MacroPolicy,
    #[serde(default)]
    pub log: LogPolicy,
}

impl Default for Service {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Service defaults must deserialize")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct P0Policy {
    #[serde(default = "default_true")]
    pub write_all_buttons: bool,
    #[serde(default = "default_true")]
    pub skip_unchanged: bool,
    #[serde(default = "default_true")]
    pub normal_mode_only: bool,
}

impl Default for P0Policy {
    fn default() -> Self { serde_json::from_str("{}").unwrap() }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MacroPolicy {
    #[serde(default)]
    pub allow_flash_write: bool,
    #[serde(default = "default_true")]
    pub single_step_shortcut: bool,
    #[serde(default)]
    pub auto_assign_ids: bool,
    #[serde(default = "default_id_start")]
    pub id_start: u16,
    #[serde(default = "default_id_end")]
    pub id_end: u16,
}

fn default_id_start() -> u16 { 1 }
fn default_id_end() -> u16 { 64 }

impl Default for MacroPolicy {
    fn default() -> Self { serde_json::from_str("{}").unwrap() }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogPolicy {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub level: String,
    #[serde(default = "default_max_kb")]
    pub max_kb: u32,
}

fn default_max_kb() -> u32 { 512 }

impl Default for LogPolicy {
    fn default() -> Self { serde_json::from_str("{}").unwrap() }
}

// ────────────────────────────── 解析 / 校验 ──────────────────────────────

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str::<Config>(text).map_err(|e| format!("JSON 解析失败：{e}"))
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("读不到 {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    /// 逐项校验。返回所有错误，一次性报全，避免反复试错。
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut err: Vec<String> = Vec::new();
        if self.schema != SCHEMA_VERSION {
            err.push(format!("schema={}，本程序只认 {}", self.schema, SCHEMA_VERSION));
        }
        if self.device.buttons.is_empty() {
            err.push("device.buttons 为空".into());
        }
        if self.device.led_regions.is_empty() {
            err.push("device.led_regions 为空".into());
        }
        let mut seen_id: HashMap<u8, String> = HashMap::new();
        for (name, b) in &self.device.buttons {
            if let Some(prev) = seen_id.insert(b.id, name.clone()) {
                err.push(format!("按钮 {name} 与 {prev} 用了同一个 id {}", b.id));
            }
            if razer_core::parse_action(&b.native).is_none() && b.native != "native" {
                err.push(format!("按钮 {name} 的 native 动作无法解析：{}", b.native));
            }
        }
        // 基础层允许缺键（缺的自动补 native），但写了的必须认识。
        for name in self.base.bindings.keys() {
            if !self.device.buttons.contains_key(name) {
                err.push(format!("base.bindings 里出现未知按钮 {name}"));
            }
        }
        self.validate_light_map(&self.base.lighting, "base.lighting", &mut err);
        if let Some(dpi) = &self.base.sensor.dpi {
            if dpi.pair().is_none() { err.push("base.sensor.dpi 必须是一个数或 [x,y]".into()); }
        }

        let mut ids = HashSet::new();
        for p in &self.profiles {
            if !ids.insert(p.id.as_str()) {
                err.push(format!("重复的 profile id：{}", p.id));
            }
            if let Some(ov) = &p.overrides.bindings {
                for name in ov.keys() {
                    if !self.device.buttons.contains_key(name) {
                        err.push(format!("profile {} 覆盖了未知按钮 {name}", p.id));
                    }
                }
            }
            if let Some(lights) = &p.overrides.lighting {
                self.validate_light_map(lights, &format!("profile {}.lighting", p.id), &mut err);
            }
            if let Some(s) = &p.overrides.sensor {
                if let Some(dpi) = &s.dpi {
                    if dpi.pair().is_none() {
                        err.push(format!("profile {} 的 sensor.dpi 格式不对", p.id));
                    }
                }
            }
            // 真正编译一次，动作串、宏引用、native 展开都会在这里被验证。
            if let Err(e) = self.compile(&p.id) {
                err.push(format!("profile {}: {e}", p.id));
            }
        }
        if !self.profiles.iter().any(|p| p.id == self.service.fallback) {
            err.push(format!("service.fallback 指向不存在的 profile：{}", self.service.fallback));
        }
        if self.service.mode != "online" {
            err.push(format!("service.mode 只能是 online，当前 {}", self.service.mode));
        }
        if err.is_empty() { Ok(()) } else { Err(err) }
    }

    fn validate_light_map(
        &self,
        lights: &HashMap<String, LightDef>,
        where_: &str,
        err: &mut Vec<String>,
    ) {
        for (name, l) in lights {
            if !self.device.led_regions.contains_key(name) {
                err.push(format!("{where_} 里出现未知灯区 {name}"));
            }
            if !matches!(l.effect.as_str(), "static" | "spectrum" | "wave") {
                err.push(format!("{where_}.{name} 的灯效不支持：{}", l.effect));
            }
            if l.effect == "static" && parse_rgb(l.rgb.as_deref()).is_none() {
                err.push(format!("{where_}.{name} 是 static 却没给合法的 6 位 rgb"));
            }
        }
    }

    /// 非致命提示：基础层没写全的按钮会自动按 native 补齐。
    pub fn warnings(&self) -> Vec<String> {
        self.device.buttons.keys()
            .filter(|n| !self.base.bindings.contains_key(*n))
            .map(|n| format!("base.bindings 未写 {n}，自动按 native 补齐"))
            .collect()
    }

    /// 按前台信息挑一组配置。priority 大的优先，相同则数组靠前的优先。
    pub fn select_profile(&self, exe: &str, title: &str, path: &str) -> Option<&str> {
        let use_exe = self.service.match_by.iter().any(|s| s == "foreground_exe");
        let use_title = self.service.match_by.iter().any(|s| s == "window_title");
        let use_path = self.service.match_by.iter().any(|s| s == "process_path");
        fn norm(s: &str, case_sensitive: bool) -> String {
            if case_sensitive { s.to_string() } else { s.to_ascii_lowercase() }
        }

        let mut candidates: Vec<(i32, usize, &Profile)> = self.profiles.iter().enumerate()
            .filter(|(_, p)| p.enabled && p.scope != "global")
            .map(|(i, p)| (p.priority, i, p))
            .collect();
        // sort_by：priority 降序；相同 priority 时保留原始顺序（索引小的在前）。
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, _, p) in candidates {
            let Some(m) = &p.match_rule else { continue };
            let (e, t, pa) = (
                norm(exe, m.case_sensitive),
                norm(title, m.case_sensitive),
                norm(path, m.case_sensitive),
            );
            let hit = (use_exe && m.exe.iter().any(|x| !x.is_empty() && e == norm(x, m.case_sensitive)))
                || (use_title && m.title_contains.iter().any(|x| !x.is_empty() && t.contains(&norm(x, m.case_sensitive))))
                || (use_path && m.path_contains.iter().any(|x| !x.is_empty() && pa.contains(&norm(x, m.case_sensitive))));
            if hit { return Some(p.id.as_str()); }
        }
        self.profiles.iter()
            .find(|p| p.id == self.service.fallback)
            .map(|p| p.id.as_str())
    }

    /// 展开 `native` 与 `macro:<名字>`，并检查动作串能被设备层识别。
    fn expand_action(&self, button: &str, raw: &str) -> Result<String, String> {
        let action = raw.trim();
        let lower = action.to_ascii_lowercase();
        if lower == "native" {
            return self.device.buttons.get(button)
                .map(|b| b.native.clone())
                .ok_or_else(|| format!("按钮 {button} 不在 device.buttons 里"));
        }
        for (prefix, mode) in [("macro:", "once"), ("macrohold:", "hold"), ("macrotoggle:", "toggle")] {
            if let Some(name) = lower.strip_prefix(prefix) {
                let name = name.trim();
                // 不在 macros 表里就当成 ID 写法（macro:0x0101x3），原样交给 parse_action。
                let Some(def) = self.macros.get(name) else {
                    return Ok(action.to_string());
                };
                if def.steps.is_empty() {
                    return Err(format!("按钮 {button} 引用的宏 {name} 没有 steps"));
                }
                // 单步组合键宏直接展开成按键绑定：零 Flash 写入。
                if mode == "once"
                    && self.service.macros.single_step_shortcut
                    && def.steps.len() == 1
                {
                    let step = def.steps[0].trim();
                    let candidate = format!("key:{step}");
                    if razer_core::parse_action(&candidate).is_some() {
                        return Ok(candidate);
                    }
                }
                if !self.service.macros.allow_flash_write {
                    return Err(format!(
                        "按钮 {button} 的多步宏 {name} 需要写设备宏区；请把 service.macros.allow_flash_write 设为 true（或改用单步组合键）"
                    ));
                }
                let Some(id) = def.id else {
                    return Err(format!("按钮 {button} 的宏 {name} 还没回填 id"));
                };
                return Ok(match mode {
                    "hold" => format!("macrohold:{id}"),
                    "toggle" => format!("macrotoggle:{id}"),
                    _ => format!("macro:{id}x1"),
                });
            }
        }
        Ok(action.to_string())
    }

    /// 合并 base + 选中 profile 的 overrides，产出设备层 `SlotConfig`。
    /// 每次都产出**全量**按键表，p0 差量写不会残留上一组的动作。
    pub fn compile(&self, profile_id: &str) -> Result<SlotConfig, String> {
        let profile = self.profiles.iter().find(|p| p.id == profile_id)
            .ok_or_else(|| format!("没有这个配置组：{profile_id}"))?;

        // 1) 按键：base 打底，overrides 覆盖任意键（含 left/right/wheelup/wheeldown）。
        let mut actions: BTreeMap<String, String> = BTreeMap::new();
        for (name, b) in &self.base.bindings {
            actions.insert(name.clone(), b.action.clone());
        }
        if let Some(ov) = &profile.overrides.bindings {
            for (name, b) in ov {
                actions.insert(name.clone(), b.action.clone());
            }
        }
        if self.service.p0.write_all_buttons {
            for (name, def) in &self.device.buttons {
                actions.entry(name.clone()).or_insert_with(|| def.native.clone());
            }
        }
        let mut bindings: Vec<(u8, String)> = Vec::with_capacity(actions.len());
        for (name, raw) in actions {
            let def = self.device.buttons.get(&name)
                .ok_or_else(|| format!("未知按钮 {name}"))?;
            let expanded = self.expand_action(&name, &raw)?;
            if razer_core::parse_action(&expanded).is_none() {
                return Err(format!("按钮 {name} 的动作无法解析：{expanded}（原始：{raw}）"));
            }
            bindings.push((def.id, expanded));
        }
        bindings.sort_by_key(|(id, _)| *id);

        // 2) 传感器：逐字段继承。
        let dpi = profile.overrides.sensor.as_ref().and_then(|s| s.dpi.as_ref())
            .or(self.base.sensor.dpi.as_ref())
            .and_then(|d| d.pair());
        let polling = profile.overrides.sensor.as_ref().and_then(|s| s.polling)
            .or(self.base.sensor.polling);

        // 3) 灯效：base 打底，overrides 逐区覆盖。
        let mut lights: BTreeMap<String, LightDef> = BTreeMap::new();
        for (name, l) in &self.base.lighting {
            lights.insert(name.clone(), l.clone());
        }
        if let Some(ov) = &profile.overrides.lighting {
            for (name, l) in ov {
                lights.insert(name.clone(), l.clone());
            }
        }
        let mut leds = Vec::new();
        let mut brightness = Vec::new();
        for (name, l) in lights {
            let region = *self.device.led_regions.get(&name)
                .ok_or_else(|| format!("未知灯区 {name}"))?;
            leds.push(LedSetting {
                region,
                effect: l.effect.clone(),
                rgb: parse_rgb(l.rgb.as_deref()),
                mode: l.mode.unwrap_or(0),
                speed: l.speed.unwrap_or(0),
            });
            brightness.push((region, l.brightness));
        }
        leds.sort_by_key(|l| l.region);
        brightness.sort_by_key(|b| b.0);

        Ok(SlotConfig {
            name: if profile.name.is_empty() { profile.id.clone() } else { profile.name.clone() },
            bindings,
            leds,
            brightness,
            dpi,
            polling,
        })
    }
}

fn parse_rgb(s: Option<&str>) -> Option<(u8, u8, u8)> {
    let s = s?.trim();
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

// ────────────────────────────── 路径 / 首次部署 ──────────────────────────────

pub fn runtime_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    base.join("RazerMate").join("profiles.json")
}

/// 文件不存在时写一份默认配置。返回是否新建。
pub fn ensure_default(path: &std::path::Path) -> Result<bool, String> {
    if path.exists() { return Ok(false); }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
    }
    std::fs::write(path, DEFAULT_PROFILES_JSON)
        .map_err(|e| format!("写默认配置失败：{e}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config { Config::parse(DEFAULT_PROFILES_JSON).unwrap() }

    #[test]
    fn default_config_validates() {
        let c = sample();
        c.validate().unwrap_or_else(|e| panic!("{}", e.join("\n")));
        assert!(c.warnings().is_empty());
        assert_eq!(c.device.buttons.len(), 13);
        assert_eq!(c.profiles.len(), 4);
    }

    #[test]
    fn compile_expands_native_and_fixed_bindings() {
        let c = sample();
        let s = c.compile("global").unwrap();
        assert_eq!(s.bindings.len(), 13, "每次都下发全量按键表");
        let get = |id: u8| -> String {
            s.bindings.iter().find(|(i, _)| *i == id).unwrap().1.clone()
        };
        assert_eq!(get(1), "mouse:left");
        assert_eq!(get(2), "mouse:right");
        assert_eq!(get(9), "mouse:wheelup");
        assert_eq!(get(10), "mouse:wheeldown");
        assert_eq!(get(52), "key:ctrl+v");
        assert_eq!(get(53), "key:ctrl+c");
        assert_eq!(s.leds.len(), 3);
        assert_eq!(s.brightness.len(), 3);
        assert_eq!(s.dpi, Some((1600, 1600)));
    }

    #[test]
    fn any_button_can_be_overridden_without_recompiling() {
        // 关键验收：把「固定」的右键改成别的动作，程序不该有任何阻拦。
        let mut text = DEFAULT_PROFILES_JSON.to_string();
        text = text.replace(
            "\"back\":      { \"action\": \"macro:close_tab\"",
            "\"right\":     { \"action\": \"key:ctrl+w\" },\n          \"back\":      { \"action\": \"macro:close_tab\"",
        );
        let c = Config::parse(&text).unwrap();
        c.validate().unwrap();
        let s = c.compile("browser").unwrap();
        assert_eq!(s.bindings.iter().find(|(i, _)| *i == 2).unwrap().1, "key:ctrl+w");
    }

    #[test]
    fn single_step_macro_needs_no_flash() {
        let c = sample();
        let s = c.compile("browser").unwrap();
        assert_eq!(s.bindings.iter().find(|(i, _)| *i == 4).unwrap().1, "key:ctrl+w");
        let s = c.compile("kicad").unwrap();
        assert_eq!(s.bindings.iter().find(|(i, _)| *i == 5).unwrap().1, "key:ctrl+shift+s");
    }

    #[test]
    fn overrides_inherit_the_rest() {
        let c = sample();
        let s = c.compile("game").unwrap();
        // game 只覆盖了 2 个键，其余仍来自 base，六个固定键不会丢。
        assert_eq!(s.bindings.len(), 13);
        assert_eq!(s.bindings.iter().find(|(i, _)| *i == 52).unwrap().1, "key:ctrl+v");
        assert_eq!(s.bindings.iter().find(|(i, _)| *i == 15).unwrap().1, "dpi:down");
        assert_eq!(s.dpi, Some((800, 800)));
    }

    #[test]
    fn profile_matching_respects_priority() {
        let c = sample();
        assert_eq!(c.select_profile("chrome.exe", "", ""), Some("browser"));
        assert_eq!(c.select_profile("", "KiCad PCB Editor", ""), Some("kicad"));
        assert_eq!(c.select_profile("notepad.exe", "", ""), Some("global"));
    }

    #[test]
    fn broken_json_is_rejected() {
        assert!(Config::parse("{").is_err());
        let bad = DEFAULT_PROFILES_JSON.replace("\"action\": \"key:ctrl+v\"", "\"action\": \"nonsense:xyz\"");
        let c = Config::parse(&bad).unwrap();
        assert!(c.validate().is_err(), "未知动作必须被拦下");
    }
}
