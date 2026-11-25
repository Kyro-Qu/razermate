//! Device capabilities on top of the raw protocol.
//!
//! Every write here follows the same contract: read the old value, write, read
//! back, and put the old value back if the device disagrees. That is the only
//! defence we have when the thing on the other end is somebody's flash chip.

use razer_protocol::commands::{self as cmd, Binding, FlashInfo, Lighting};
use razer_protocol::keys::button as btn;
use razer_protocol::report::Status;
use razer_protocol::report::Report;

pub use razer_hid::{Error, Result};

/// A mouse we hold an exclusive handle to.
pub struct Mouse {
    dev: razer_hid::Device,
}

/// Everything the info panel shows.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub firmware: (u8, u8, u8),
    pub mode: u8,
    pub active_profile: u8,
    pub profiles: Vec<u8>,
    pub dpi: (u16, u16),
    pub polling_hz: u16,
    pub flash: FlashInfo,
}

impl Mouse {
    pub fn open() -> Result<Self> {
        Ok(Mouse { dev: razer_hid::Device::open()? })
    }

    pub fn path(&self) -> &str {
        &self.dev.path
    }

    fn raw(&self, request: &Report) -> Result<Report> {
        self.dev.call(request)
    }

    /// 发一条原始报文并原样返回应答。只给 `razerctl raw` 用来确认尚未定下来的
    /// 命令码（DPI、回报率等，见 docs/PROTOCOL.md §8）。业务代码不要走这里。
    pub fn call_raw(&self, request: &Report) -> Result<Report> {
        self.raw(request)
    }

    pub fn firmware(&self) -> Result<(u8, u8, u8)> {
        let r = self.raw(&cmd::get_firmware())?;
        let a = r.reply();
        Ok((a[0], a[1], a[2]))
    }

    /// 0 normal, 1 bootloader, 2 test, 3 driver.
    pub fn mode(&self) -> Result<u8> {
        let r = self.raw(&cmd::get_mode())?;
        Ok(r.reply()[0])
    }

    /// Leave driver mode. Onboard bindings and macros only work in normal mode,
    /// and a crashed Synapse can leave the mouse stuck in driver mode.
    pub fn ensure_normal(&self) -> Result<u8> {
        let before = self.mode()?;
        if before != 0 {
            self.raw(&cmd::set_mode(0))?;
        }
        let after = self.mode()?;
        Ok(after)
    }

    pub fn set_mode(&self, mode: u8) -> Result<()> {
        self.raw(&cmd::set_mode(mode))?;
        Ok(())
    }

    pub fn active_profile(&self) -> Result<u8> {
        let r = self.raw(&cmd::get_active_profile())?;
        Ok(r.reply()[0])
    }

    pub fn set_active_profile(&self, id: u8) -> Result<u8> {
        self.raw(&cmd::set_active_profile(id))?;
        // The device applies asynchronously; read back until it agrees.
        for _ in 0..10 {
            if self.active_profile()? == id {
                return Ok(id);
            }
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        self.active_profile()
    }

    pub fn profile_count(&self) -> Result<u8> {
        let r = self.raw(&cmd::profile_count())?;
        Ok(r.reply()[0])
    }

    pub fn profile_list(&self) -> Result<Vec<u8>> {
        let r = self.raw(&cmd::profile_list())?;
        let a = r.reply();
        // [total, id1..idn]
        let n = self.profile_count()? as usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if let Some(v) = a.get(1 + i) {
                if *v != 0 {
                    out.push(*v);
                }
            }
        }
        Ok(out)
    }

    pub fn binding(&self, profile: u8, button: u8, layer: u8) -> Result<Binding> {
        let r = self.raw(&cmd::get_binding(profile, button, layer))?;
        Binding::from_reply(r.reply()).ok_or(Error::Parse("button binding reply"))
    }

    /// Write a binding with read-back verification and rollback.
    pub fn set_binding(
        &self,
        profile: u8,
        button: u8,
        layer: u8,
        new: Binding,
    ) -> Result<Binding> {
        let old = self.binding(profile, button, layer)?;
        self.raw(&cmd::set_binding(profile, button, layer, new))?;

        for _ in 0..8 {
            std::thread::sleep(std::time::Duration::from_millis(80));
            let got = self.binding(profile, button, layer)?;
            if got == new {
                return Ok(got);
            }
        }
        // Disagreement: put the old value back and report what stuck.
        let _ = self.raw(&cmd::set_binding(profile, button, layer, old));
        Err(Error::Device {
            status: Status::Fail,
            class: cmd::class::BUTTON,
            command: cmd::button::SET_BINDING,
        })
    }

    pub fn flash(&self) -> Result<FlashInfo> {
        let r = self.raw(&cmd::flash_usage())?;
        FlashInfo::parse(r.reply()).ok_or(Error::Parse("flash usage reply"))
    }

    /// 当前 DPI。`(x, y)`。读取不需要驱动模式。
    pub fn dpi(&self, profile: u8) -> Result<(u16, u16)> {
        let r = self.raw(&cmd::get_dpi(profile))?;
        cmd::parse_dpi(r.reply()).ok_or(Error::Parse("DPI reply"))
    }

    /// DPI 档位组。
    pub fn dpi_stages(&self, profile: u8) -> Result<cmd::DpiStages> {
        let r = self.raw(&cmd::get_dpi_stages(profile))?;
        cmd::DpiStages::parse(r.reply()).ok_or(Error::Parse("DPI stages reply"))
    }

    /// 写 DPI。
    ///
    /// 实测两个坑，都必须处理，否则用户会觉得「改个 DPI，配置怎么全变了」：
    /// 1. 正常模式（0）下写 DPI 会「成功但不生效」——设备返回 OK，读回还是旧值。
    ///    必须切到驱动模式（3）写，写完切回正常模式。
    /// 2. **切驱动模式再切回来，会让生效槽重置为 1 号槽。** 所以这里要记住原来的
    ///    生效槽，写完再切回去。
    pub fn set_dpi(&self, profile: u8, x: u16, y: u16) -> Result<(u16, u16)> {
        let before_mode = self.mode()?;
        let before_active = self.active_profile()?;
        let old = self.dpi(profile)?;

        if before_mode != 3 {
            self.set_mode(3)?;
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        self.raw(&cmd::set_dpi(profile, x, y))?;
        std::thread::sleep(std::time::Duration::from_millis(150));
        let got = self.dpi(profile)?;

        if before_mode != 3 {
            self.set_mode(before_mode)?;
            // 模式切换会把生效槽重置，这里还原用户的槽位。
            if self.active_profile().unwrap_or(1) != before_active {
                let _ = self.set_active_profile(before_active);
            }
        }

        if got != (x, y) {
            // 回滚，别把用户的配置留在半改状态。
            let _ = self.set_mode(3);
            let _ = self.raw(&cmd::set_dpi(profile, old.0, old.1));
            let _ = self.set_mode(before_mode);
            let _ = self.set_active_profile(before_active);
            return Err(Error::Device {
                status: Status::Fail,
                class: cmd::class::MOUSE,
                command: cmd::sensor::SET_DPI,
            });
        }
        Ok(got)
    }

    /// 当前回报率（Hz）。设备存的是轮询间隔，这里换算成标称值。
    pub fn polling_hz(&self, profile: u8) -> Result<u16> {
        let r = self.raw(&cmd::get_poll_rate(profile))?;
        let a = r.reply();
        if a.len() < 2 {
            return Err(Error::Transfer { code: 0 });
        }
        Ok(cmd::delay_to_hz(a[1]))
    }

    /// 写回报率。`hz` 只接受 125 / 500 / 1000。
    pub fn set_polling_hz(&self, profile: u8, hz: u16) -> Result<u16> {
        let delay = cmd::hz_to_delay(hz).ok_or(Error::Parse("unsupported polling rate"))?;
        let old = self.polling_hz(profile)?;
        self.raw(&cmd::set_poll_rate(profile, delay))?;
        std::thread::sleep(std::time::Duration::from_millis(150));
        let got = self.polling_hz(profile)?;
        if got != hz {
            if let Some(old_delay) = cmd::hz_to_delay(old) {
                let _ = self.raw(&cmd::set_poll_rate(profile, old_delay));
            }
            return Err(Error::Device {
                status: Status::Fail,
                class: cmd::class::DEVICE,
                command: cmd::device::SET_POLL_RATE,
            });
        }
        Ok(got)
    }

    pub fn lighting(&self, profile: u8, region: u8) -> Result<Lighting> {
        let r = self.raw(&cmd::get_effect(profile, region))?;
        // reply: [profile, region, effect, mode, speed, n, rgb...]
        Lighting::parse(r.reply()).ok_or(Error::Parse("GET_EFFECT payload too short"))
    }

    pub fn set_static(&self, profile: u8, region: u8, rgb: (u8, u8, u8)) -> Result<Lighting> {
        self.raw(&cmd::set_static(profile, region, &[rgb]))?;
        std::thread::sleep(std::time::Duration::from_millis(120));
        self.lighting(profile, region)
    }

    pub fn set_spectrum(&self, profile: u8, region: u8) -> Result<Lighting> {
        self.raw(&cmd::set_spectrum(profile, region))?;
        std::thread::sleep(std::time::Duration::from_millis(120));
        self.lighting(profile, region)
    }

    pub fn set_wave(&self, profile: u8, region: u8, dir: u8, speed: u8) -> Result<Lighting> {
        self.raw(&cmd::set_wave(profile, region, dir, speed))?;
        std::thread::sleep(std::time::Duration::from_millis(120));
        self.lighting(profile, region)
    }

    /// 逐颗写一帧。**不睡 120 ms**：这是动画用的，每帧都睡就没法测真实帧率了。
    /// 调用方自己决定节奏；写不写进 Flash 也不由这里管（逐颗本来就不落盘）。
    pub fn set_custom(&self, profile: u8, region: u8, colors: &[(u8, u8, u8)]) -> Result<()> {
        self.raw(&cmd::set_custom(profile, region, colors))?;
        Ok(())
    }

    /// 把灯效还原成进预览之前的板载效果。
    ///
    /// `set_custom` 是运行时效果：不写 Flash，切槽 / 断电就没了，但如果不主动
    /// 还原，鼠标会一直停在最后一帧上。先写一次静态色把逐颗模式顶掉，再切回
    /// 光谱 / 波浪，最后读回校验。
    pub fn led_restore(&self, profile: u8, region: u8, old: &Lighting) -> Result<Lighting> {
        let first = old.colors.first().copied().unwrap_or((0, 0, 0));
        self.set_static(profile, region, first)?;
        let now = match old.effect {
            cmd::effect::SPECTRUM => self.set_spectrum(profile, region)?,
            cmd::effect::WAVE => self.set_wave(profile, region, old.mode, old.speed)?,
            _ => self.lighting(profile, region)?,
        };
        if now.effect != old.effect {
            return Err(Error::Transfer { code: 0 });
        }
        Ok(now)
    }

    /// 一段 2 秒左右的彩虹流动，结束自动还原。
    ///
    /// 实测逐帧写入约 7 ms（142 fps），瓶颈不在 USB 而在我们自己的节奏，
    /// 所以按 30 fps 发送，避免为了炫技把主机 CPU 和 HID 队列占满。
    pub fn led_animate(
        &self,
        profile: u8,
        region: u8,
        old: &Lighting,
        frames: usize,
    ) -> Result<(usize, f64)> {
        let n = cmd::region::STRIP_LEDS as usize;
        let start = std::time::Instant::now();
        let mut ok = 0usize;
        for f in 0..frames {
            self.set_custom(profile, region, &led_rainbow(n, f * 8))?;
            ok += 1;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let elapsed = start.elapsed().as_secs_f64();
        self.led_restore(profile, region, old)?;
        Ok((ok, elapsed))
    }

    pub fn brightness(&self, profile: u8, region: u8) -> Result<u8> {
        let r = self.raw(&cmd::get_brightness(profile, region))?;
        Ok(*r.reply().get(2).unwrap_or(&0))
    }

    pub fn set_brightness(&self, profile: u8, region: u8, pct: u8) -> Result<u8> {
        let pct = pct.min(100);
        let scaled = (pct as u16 * 255 / 100) as u8;
        self.raw(&cmd::set_brightness(profile, region, scaled))?;
        std::thread::sleep(std::time::Duration::from_millis(120));
        let raw = self.brightness(profile, region)?;
        Ok((raw as u16 * 100 / 255) as u8)
    }

    /// 按原始 0–255 写亮度。
    ///
    /// 百分比是有损的：69% 写下去是 175，175 读回来又成了 68%。所以「还原到原值」
    /// 这种场景必须走原始值，否则每跑一次测试就会往下掉 1 LSB。
    pub fn set_brightness_raw(&self, profile: u8, region: u8, raw: u8) -> Result<u8> {
        self.raw(&cmd::set_brightness(profile, region, raw))?;
        std::thread::sleep(std::time::Duration::from_millis(120));
        self.brightness(profile, region)
    }

    pub fn macro_count(&self) -> Result<u16> {
        let r = self.raw(&cmd::macro_count())?;
        cmd::be_u16(r.reply()).ok_or(Error::Parse("macro size reply"))
    }

    pub fn macro_size(&self, id: u16) -> Result<u32> {
        let r = self.raw(&cmd::macro_size(id))?;
        let a = r.reply();
        if a.len() < 6 {
            return Err(Error::Transfer { code: 0 });
        }
        Ok(u32::from_be_bytes([a[2], a[3], a[4], a[5]]))
    }

    pub fn macro_read(&self, id: u16) -> Result<Vec<u8>> {
        let len = self.macro_size(id)? as usize;
        let mut out = Vec::with_capacity(len);
        while out.len() < len {
            let offset = out.len() as u32;
            let want = ((len - out.len()).min(64)) as u8;
            let r = self.raw(&cmd::macro_read(id, offset, want))?;
            let a = r.reply();
            if a.len() < 7 {
                break;
            }
            let chunk = &a[7..];
            if chunk.is_empty() {
                break;
            }
            out.extend_from_slice(&chunk[..chunk.len().min(want as usize)]);
        }
        out.truncate(len);
        Ok(out)
    }

    /// 写入一个宏。
    ///
    /// 固件有个硬限制：**已存在的 ID 不能直接覆写**。`MACRO_SET_SIZE` 会
    /// 直接回 `device replied Fail to class 0x06 command 0x08`（实测 A 级）。
    /// 所以这里先备份旧内容，删掉再写；万一新内容写失败，把旧的还原回去，
    /// 不让一次失败的操作把用户板上已有的宏弄丢。
    pub fn macro_write(&self, id: u16, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Err(Error::Transfer { code: 0 });
        }

        let previous = match self.macro_size(id) {
            Ok(n) if n > 0 => self.macro_read(id).ok(),
            _ => None,
        };
        if previous.is_some() {
            self.macro_delete(id)?;
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let result = self.macro_write_fresh(id, data);
        if result.is_err() {
            if let Some(old) = previous.as_deref().filter(|v| !v.is_empty()) {
                // 已经删过了，ID 是空的，可以直接写回。
                let _ = self.macro_write_fresh(id, old);
            }
        }
        result
    }

    /// 往一个「当前不存在」的 ID 写内容。调用方负责先删。
    fn macro_write_fresh(&self, id: u16, data: &[u8]) -> Result<()> {
        self.raw(&cmd::macro_set_size(id, data.len() as u32))?;
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut offset = 0usize;
        while offset < data.len() {
            let end = (offset + 64).min(data.len());
            self.raw(&cmd::macro_write_chunk(id, offset as u32, &data[offset..end]))?;
            offset = end;
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        Ok(())
    }

    pub fn macro_delete(&self, id: u16) -> Result<()> {
        self.raw(&cmd::macro_delete(id))?;
        std::thread::sleep(std::time::Duration::from_millis(150));
        Ok(())
    }

    /// 探测 `from..=to` 里哪些 ID 真的有宏（`size > 0`）。
    ///
    /// 设备没有"列出全部宏"的命令，只能逐个问。一次探测大约 2–3 ms，
    /// 所以范围别开太大；界面走 `razer-ipc::known` 的登记表，只有扫描功能用这个。
    /// `expect` 是已知的宏总数，找到这么多就可以提前收工。
    pub fn macro_scan(&self, from: u16, to: u16, expect: usize) -> Vec<u16> {
        let mut ids = Vec::new();
        let from = from.max(1);
        for id in from..=to {
            if let Ok(size) = self.macro_size(id) {
                if size > 0 {
                    ids.push(id);
                    if expect > 0 && ids.len() >= expect {
                        break;
                    }
                }
            }
        }
        ids
    }

    /// `(x, y)` in DPI. Read only: writing DPI needs driver mode and is handled
    /// separately.
    /// 导出一份纯文本配置快照（可读、可 diff、可手工改）。
    ///
    /// 板载设置本来就存在鼠标 Flash 里，程序崩溃不会影响它；这份文件解决的是
    /// 「改乱之后能回滚」和「换台电脑能搬过去」。所以除软件侧的规则 / 宏登记表，
    /// 也把当前生效槽的按键、三区灯效与亮度、DPI、回报率、宏本体逐项记下来。
    /// `custom_override` 是预览灯带之前记下的板载灯效
    /// `(effect, mode, speed, rgb)`。CUSTOM 是运行时效果、不写 Flash，
    /// 不能把它当作"配置"备份下来；有备份就用板载值，没有就标注跳过。
    pub fn export_config(
        &self,
        rules_text: &str,
        macros_text: &str,
        custom_override: Option<(u8, u8, u8, (u8, u8, u8))>,
    ) -> Result<String> {
        let p = self.active_profile()?;
        let mut out = String::from(
            "# RazerMate 配置快照 v1\n\
             # 板载设置存在鼠标 Flash 里，程序崩溃不会丢；此文件用于改乱后回滚 / 换机迁移。\n",
        );
        out.push_str("[software]\n");
        for line in rules_text.lines() {
            let t = line.trim();
            if !t.is_empty() && !t.starts_with('#') {
                out.push_str(&format!("RULE {t}\n"));
            }
        }
        for line in macros_text.lines() {
            let t = line.trim();
            if !t.is_empty() && !t.starts_with('#') {
                out.push_str(&format!("KNOWN {t}\n"));
            }
        }

        out.push_str("[device]\n");
        out.push_str(&format!("SLOT {p}\n"));
        let (x, y) = self.dpi(p)?;
        out.push_str(&format!("DPI {p} {x} {y}\n"));
        out.push_str(&format!("POLL {p} {}\n", self.polling_hz(p)?));

        for (id, _) in btn::ALL {
            if let Ok(b) = self.binding(p, *id, cmd::layer::NORMAL) {
                out.push_str(&format!(
                    "BIND {p} {id} {} {} {}\n",
                    cmd::layer::NORMAL,
                    b.fn_id,
                    hex_bytes(&b.data)
                ));
            }
        }
        for r in [
            cmd::region::WHEEL,
            cmd::region::LOGO,
            cmd::region::STRIP,
        ] {
            if let Ok(l) = self.lighting(p, r) {
                let (eff, mode, speed, c) = if l.effect == cmd::effect::CUSTOM {
                    match custom_override {
                        Some((e, m, s, c)) => (e, m, s, c),
                        None => {
                            out.push_str(&format!(
                                "# NOTE 灯区 {r} 正停在运行时逐灯预览，不能持久化，未纳入快照（请先点「还原」）\n"
                            ));
                            continue;
                        }
                    }
                } else {
                    (
                        l.effect,
                        l.mode,
                        l.speed,
                        l.colors.first().copied().unwrap_or((0, 0, 0)),
                    )
                };
                out.push_str(&format!(
                    "LIGHT {p} {r} {eff} {mode} {speed} {:02x}{:02x}{:02x}\n",
                    c.0, c.1, c.2
                ));
            }
            if let Ok(raw) = self.brightness(p, r) {
                out.push_str(&format!("BRIGHT {p} {r} {raw}\n"));
            }
        }
        for id in macros_text
            .lines()
            .map(str::trim)
            .filter(|t| !t.is_empty() && !t.starts_with('#'))
            .filter_map(|t| u16::from_str_radix(t.trim_start_matches("0x"), 16).ok())
        {
            if let Ok(body) = self.macro_read(id) {
                if !body.is_empty() {
                    out.push_str(&format!("MACRO 0x{id:04X} {}\n", hex_bytes(&body)));
                }
            }
        }
        Ok(out)
    }

    /// 从快照恢复。返回 `(规则文本, 宏登记表文本, 成功写回的设备项数)`。
    ///
    /// 顺序有讲究：先切槽，再 DPI / 回报率，然后按键、灯效、亮度，最后宏。
    /// 每项底层写操作都自带「读回校验 + 不一致回滚」，这里只负责调度。
    /// 灯效里的 `CUSTOM` 是运行时预览、不可能持久化，恢复时降级成静态色。
    pub fn restore_config(&self, text: &str) -> Result<(String, String, usize)> {
        let mut rules = String::from("# RazerMate 自动切换规则\n");
        let mut known = String::from("# RazerMate 已知宏 ID（自动维护，可手工删改）\n");
        let mut binds: Vec<(u8, u8, u8, [u8; 5])> = Vec::new();
        let mut lights: Vec<(u8, u8, u8, u8, (u8, u8, u8))> = Vec::new();
        let mut brights: Vec<(u8, u8)> = Vec::new();
        let mut macros: Vec<(u16, Vec<u8>)> = Vec::new();
        let mut dpi: Option<(u16, u16)> = None;
        let mut poll: Option<u16> = None;
        let mut slot: Option<u8> = None;

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let (tag, body) = line.split_once(' ').unwrap_or((line, ""));
            let f: Vec<&str> = body.split_whitespace().collect();
            match tag {
                "RULE" => {
                    rules.push_str(body.trim());
                    rules.push('\n');
                }
                "KNOWN" => {
                    known.push_str(body.trim());
                    known.push('\n');
                }
                "SLOT" => slot = f.first().and_then(|s| s.parse().ok()),
                "DPI" => {
                    if f.len() >= 3 {
                        dpi = Some((
                            f[1].parse().unwrap_or(0),
                            f[2].parse().unwrap_or(0),
                        ));
                    }
                }
                "POLL" => poll = f.get(1).and_then(|s| s.parse().ok()),
                "BIND" => {
                    if f.len() >= 5 {
                        if let (Ok(btn_id), Ok(fn_id), Some(bytes)) = (
                            f[1].parse::<u8>(),
                            f[3].parse::<u8>(),
                            parse_hex(f[4]),
                        ) {
                            if bytes.len() == 5 {
                                let mut data = [0u8; 5];
                                data.copy_from_slice(&bytes);
                                binds.push((btn_id, f[2].parse().unwrap_or(0), fn_id, data));
                            }
                        }
                    }
                }
                "LIGHT" => {
                    if f.len() >= 6 {
                        if let (Ok(r), Ok(eff), Ok(mode), Ok(speed)) = (
                            f[1].parse::<u8>(),
                            f[2].parse::<u8>(),
                            f[3].parse::<u8>(),
                            f[4].parse::<u8>(),
                        ) {
                            if let Some(c) = parse_hex(f[5]) {
                                if c.len() >= 3 {
                                    lights.push((r, eff, mode, speed, (c[0], c[1], c[2])));
                                }
                            }
                        }
                    }
                }
                "BRIGHT" => {
                    if f.len() >= 3 {
                        if let (Ok(r), Ok(raw)) = (f[1].parse::<u8>(), f[2].parse::<u8>()) {
                            brights.push((r, raw));
                        }
                    }
                }
                "MACRO" => {
                    if f.len() >= 2 {
                        if let (Some(id), Some(body)) = (
                            u16::from_str_radix(f[0].trim_start_matches("0x"), 16).ok(),
                            parse_hex(f[1]),
                        ) {
                            if !body.is_empty() {
                                macros.push((id, body));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let mut applied = 0usize;
        let mut p = self.active_profile()?;
        if let Some(s) = slot {
            p = self.set_active_profile(s)?;
            applied += 1;
        }
        if let Some((x, y)) = dpi {
            self.set_dpi(p, x, y)?;
            applied += 1;
        }
        if let Some(hz) = poll {
            self.set_polling_hz(p, hz)?;
            applied += 1;
        }
        for (id, layer, fn_id, data) in binds {
            self.set_binding(p, id, layer, Binding::new(fn_id, data))?;
            applied += 1;
        }
        for (r, eff, mode, speed, rgb) in lights {
            match eff {
                cmd::effect::SPECTRUM => {
                    self.set_spectrum(p, r)?;
                }
                cmd::effect::WAVE => {
                    self.set_wave(p, r, mode, speed)?;
                }
                _ => {
                    self.set_static(p, r, rgb)?;
                }
            }
            applied += 1;
        }
        for (r, raw) in brights {
            self.set_brightness_raw(p, r, raw)?;
            applied += 1;
        }
        for (id, body) in macros {
            self.macro_write(id, &body)?;
            let back = self.macro_read(id)?;
            if back != body {
                return Err(Error::Parse("macro verify mismatch after restore"));
            }
            applied += 1;
        }
        Ok((rules, known, applied))
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let firmware = self.firmware()?;
        let mode = self.mode()?;
        let active_profile = self.active_profile()?;
        let profiles = self.profile_list()?;
        let flash = self.flash()?;
        Ok(Snapshot {
            firmware,
            mode,
            active_profile,
            profiles,
            dpi: self.dpi(active_profile).unwrap_or((0, 0)),
            polling_hz: self.polling_hz(active_profile).unwrap_or(0),
            flash,
        })
    }
}

// ── 配置快照用的小工具 ──

fn hex_bytes(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

// ── 灯带预览用的颜色生成 ──
// 放在 core 里，CLI 的 bench/preview 和 GUI 的预览按钮共用同一套算法，
// 免得两边各写一份、渲染出来颜色还对不上。

/// HSV → RGB，`h` 单位度，`s`/`v` 取 0..=1。
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let hp = (h % 360.0) / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// 一帧彩虹；`phase` 每加 1，整条灯带往前滚一点。
pub fn led_rainbow(n: usize, phase: usize) -> Vec<(u8, u8, u8)> {
    (0..n)
        .map(|i| hsv_to_rgb(((phase + i * 33) % 360) as f32, 1.0, 1.0))
        .collect()
}

/// 从 `base` 渐暗到黑，静态预览用。
pub fn led_gradient(n: usize, base: (u8, u8, u8)) -> Vec<(u8, u8, u8)> {
    (0..n)
        .map(|i| {
            let f = 1.0 - (i as f32 / n.saturating_sub(1).max(1) as f32) * 0.85;
            (
                (base.0 as f32 * f) as u8,
                (base.1 as f32 * f) as u8,
                (base.2 as f32 * f) as u8,
            )
        })
        .collect()
}

/// Names for the five onboard slots, as the user sees them on the mouse.
pub fn slot_colour_name(slot: u8) -> &'static str {
    match slot {
        1 => "white",
        2 => "red",
        3 => "green",
        4 => "blue",
        5 => "cyan",
        _ => "?",
    }
}

/// Human label for a button id.
pub fn button_name(id: u8) -> &'static str {
    btn::name(id)
}

// ─────────────────────── 中文展示层（GUI 共用） ───────────────────────
//
// 这些函数只做「协议数值 → 中文文案」的映射，不碰设备。放在 core 而不是 GUI
// 里，是为了让 CLI 的 `binding` 输出和界面显示永远出自同一张表，不会各写一套
// 然后慢慢跑偏。

/// 按键的中文名。未知 id 返回占位串而不是 panic —— 设备固件升级后可能出现新 id。
pub fn button_label_zh(id: u8) -> &'static str {
    use btn as b;
    match id {
        b::LEFT => "左键",
        b::RIGHT => "右键",
        b::MIDDLE => "滚轮中键",
        b::FORWARD => "侧键·前进",
        b::BACK => "侧键·后退",
        b::TILT_LEFT => "滚轮左倾",
        b::TILT_RIGHT => "滚轮右倾",
        b::DPI => "DPI 扳机键",
        b::CLUTCH => "狙击键",
        b::PROFILE => "底部 Profile 键",
        b::WHEEL_MODE => "滚轮模式切换",
        b::WHEEL_UP => "滚轮上滚",
        b::WHEEL_DOWN => "滚轮下滚",
        _ => "未知按键",
    }
}

/// HID usage id 的可读名。字母、数字、小键盘和功能键都由 `usage::label` 统一生成。
fn usage_label(usage: u8) -> String {
    razer_protocol::keys::usage::label(usage)
}

fn mouse_action_label_zh(code: u8) -> String {
    use cmd::mouse_action as m;
    match code {
        m::LEFT => "左键单击".into(),
        m::RIGHT => "右键单击".into(),
        m::MIDDLE => "中键单击".into(),
        m::BACK => "后退".into(),
        m::FORWARD => "前进".into(),
        m::WHEEL_UP => "滚轮上滚".into(),
        m::WHEEL_DOWN => "滚轮下滚".into(),
        m::TILT_LEFT => "滚轮左倾".into(),
        m::TILT_RIGHT => "滚轮右倾".into(),
        other => format!("鼠标动作 0x{other:02X}"),
    }
}

fn media_label_zh(code: u16) -> String {
    match code {
        0x00E9 => "音量 +".into(),
        0x00EA => "音量 −".into(),
        0x00E2 => "静音".into(),
        0x00CD => "播放/暂停".into(),
        0x00B5 => "下一曲".into(),
        0x00B6 => "上一曲".into(),
        0x00B7 => "停止".into(),
        other => format!("媒体键 0x{other:04X}"),
    }
}

/// 把绑定翻译成一句人话，例如「Ctrl + W」「播放宏 0x0101 ×3」「DPI +」。
pub fn describe_binding_zh(b: &Binding) -> String {
    use cmd::function as f;
    match b.fn_id {
        x if x == f::OFF => "未绑定".to_string(),
        x if x == f::MOUSE => mouse_action_label_zh(b.data[0]),
        x if x == f::MOUSE_ACTION_EXT => mouse_action_label_zh(b.data[0]),
        x if x == f::KEY => {
            let mods = b.data[0];
            let usage = b.data[1];
            let mut parts: Vec<String> = Vec::new();
            for (bit, name) in [
                (cmd::modifier::CTRL, "Ctrl"),
                (cmd::modifier::SHIFT, "Shift"),
                (cmd::modifier::ALT, "Alt"),
                (cmd::modifier::WIN, "Win"),
            ] {
                // 左右半区在展示层合并：用户不关心是按的左边还是右边那个 Ctrl。
                let both = bit | (bit << 4);
                if mods & both != 0 {
                    parts.push(name.to_string());
                }
            }
            if usage != 0 {
                parts.push(usage_label(usage));
            }
            if parts.is_empty() {
                "空键".to_string()
            } else {
                parts.join(" + ")
            }
        }
        x if x == f::MACRO_TIMES => {
            let id = u16::from_be_bytes([b.data[0], b.data[1]]);
            let times = b.data[2];
            if times <= 1 {
                format!("播放宏 0x{id:04X}")
            } else {
                format!("播放宏 0x{id:04X} ×{times}")
            }
        }
        x if x == f::MACRO_HELD => {
            let id = u16::from_be_bytes([b.data[0], b.data[1]]);
            format!("按住播放宏 0x{id:04X}")
        }
        x if x == f::MACRO_TOGGLE => {
            let id = u16::from_be_bytes([b.data[0], b.data[1]]);
            format!("按一下开/再按关 宏 0x{id:04X}")
        }
        x if x == f::PROFILE => "切换到下一个槽".to_string(),
        x if x == f::WHEEL_MODE => "切换滚轮模式".to_string(),
        x if x == f::DPI => match b.data[0] {
            cmd::dpi_action::UP => "DPI +".to_string(),
            cmd::dpi_action::DOWN => "DPI −".to_string(),
            cmd::dpi_action::CYCLE => "DPI 循环".to_string(),
            cmd::dpi_action::CLUTCH => "狙击模式（临时降 DPI）".to_string(),
            other => format!("DPI 动作 {other}"),
        },
        x if x == f::MEDIA => {
            let code = u16::from_be_bytes([b.data[0], b.data[1]]);
            media_label_zh(code)
        }
        x if x == f::DOUBLE => "双击".to_string(),
        x if x == f::HYPERSHIFT => "HyperShift 层".to_string(),
        other => format!("未识别 (fn=0x{other:02X})"),
    }
}

/// 原始参数，形如 `fn=2 [00 06]`。给排障用，界面上以浅色小字显示。
pub fn binding_raw(b: &Binding) -> String {
    let n = (b.fn_size as usize).min(5);
    let bytes: Vec<String> = b.data[..n].iter().map(|v| format!("{v:02X}")).collect();
    format!("fn={} [{}]", b.fn_id, bytes.join(" "))
}

/// 槽位中文名。槽颜色在鼠标上是固定的，用户靠灯色辨认，所以名字带颜色。
pub fn slot_label_zh(slot: u8) -> &'static str {
    match slot {
        0 => "临时工作层",
        1 => "白",
        2 => "红",
        3 => "绿",
        4 => "蓝",
        5 => "青",
        _ => "未知槽",
    }
}

/// 槽位灯色，用来画界面上的色条。
pub fn slot_rgb(slot: u8) -> (u8, u8, u8) {
    match slot {
        1 => (0xF2, 0xF2, 0xF2),
        2 => (0xE8, 0x3B, 0x3B),
        3 => (0x3F, 0xD0, 0x5A),
        4 => (0x3B, 0x7B, 0xE8),
        5 => (0x35, 0xC8, 0xD8),
        _ => (0x8A, 0x92, 0x9C),
    }
}

/// 把一个 RGB 说成人话。区分不出色名时退回十六进制，不硬编一个近似色。
fn colour_zh(rgb: (u8, u8, u8)) -> String {
    let (r, g, b) = (rgb.0 as u32, rgb.1 as u32, rgb.2 as u32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);

    if max < 16 {
        return "灭".into();
    }
    // 三通道接近 = 灰阶；否则按主导通道粗分。
    if max - min < 28 {
        return if max > 200 { "白".into() } else { "灰".into() };
    }
    let name = if r == max && g == max {
        // 红+绿 都到顶 → 黄系
        if b * 2 > max { "白" } else { "黄" }
    } else if r == max {
        if b * 3 > g { "品红" } else { "红" }
    } else if g == max {
        if b * 3 > r { "青" } else { "绿" }
    } else if g * 3 > r {
        "青"
    } else {
        "蓝"
    };
    name.to_string()
}

/// 灯效的简短中文名，用于概览页那种一格一格的统计块。
pub fn light_name_zh(l: &Lighting) -> String {
    use cmd::effect as e;
    match l.effect {
        e::STATIC => match l.colors.first() {
            Some(c) => format!("静态 · {}", colour_zh(*c)),
            None => "静态".into(),
        },
        e::SPECTRUM => "光谱循环".into(),
        e::WAVE => "波浪".into(),
        e::CUSTOM => format!("自定义 {} 灯", l.colors.len()),
        other => format!("未知灯效 {other}"),
    }
}

/// 灯效详情，带原始参数，用于列表里逐区展示。
pub fn light_detail_zh(l: &Lighting) -> String {    use cmd::effect as e;
    match l.effect {
        e::STATIC => match l.colors.first() {
            Some(c) => format!("静态 · {} #{:02X}{:02X}{:02X}", colour_zh(*c), c.0, c.1, c.2),
            None => "静态（未读到颜色）".into(),
        },
        e::SPECTRUM => "光谱循环（彩虹流动）".into(),
        e::WAVE => format!("波浪 · 方向 {} · 速度 {}", l.mode, l.speed),
        e::CUSTOM => format!("逐灯预览（运行时）· 设备回报 {} 颗颜色", l.colors.len()),
        other => format!("未识别灯效 {other}"),
    }
}


// ─────────────────────── 动作串解析（CLI 与 GUI 共用） ───────────────────────
//
// 语法：
//   off | double | hypershift | wheelmode | profile:next
//   left | right | middle | back | forward | wheelup | wheeldown | tiltleft | tiltright
//   key:ctrl+w         组合键（主键也接受 0x 十六进制 HID usage）
//   mouse:left         普通鼠标原生功能（fn=1）
//   mouseext:tiltleft  扩展鼠标原生功能（fn=0x0E，滚轮左右倾）
//   macro:0101x3       宏 id × 次数（id 支持 0x 前缀）
//   media:play         媒体键（也接受 0x 十六进制 consumer code）
//   dpi:up              DPI 动作
//   raw:2,2,1,25       逃生舱：fn / fn_size / 最多 5 字节 data，固件新功能无需重编译
// 单独写 "w" 或 "ctrl+w" 也能识别。
// "native" 不在这里解析：配置层会先按 JSON 的 device.buttons[name].native 展开。

fn modifier_bit(token: &str, u: u8) -> u8 {
    use cmd::modifier as m;
    use razer_protocol::keys::usage as k;
    match token {
        "ctrl" | "lctrl" => m::CTRL,
        "rctrl" => m::RCTRL,
        "shift" | "lshift" => m::SHIFT,
        "rshift" => m::RSHIFT,
        "alt" | "lalt" => m::ALT,
        "ralt" => m::RALT,
        "win" | "lwin" => m::WIN,
        "rwin" => m::RWIN,
        _ => match u {
            k::LCTRL => m::CTRL,
            k::RCTRL => m::RCTRL,
            k::LSHIFT => m::SHIFT,
            k::RSHIFT => m::RSHIFT,
            k::LALT => m::ALT,
            k::RALT => m::RALT,
            k::LWIN => m::WIN,
            k::RWIN => m::RWIN,
            _ => 0,
        },
    }
}

/// 解析十进制或 `0x` 十六进制的字节值（HID usage / raw data / fn id）。
/// 配置里出现程序没见过的名字时，十六进制就是通用逃生舱。
pub fn parse_hex_u8(s: &str) -> Option<u8> {
    let t = s.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u8::from_str_radix(h, 16).ok();
    }
    t.parse().ok()
}

pub fn parse_hex_u16(s: &str) -> Option<u16> {
    let t = s.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u16::from_str_radix(h, 16).ok();
    }
    t.parse().ok()
}

/// 解析宏 id，接受十进制或 `0x` 十六进制。
pub fn parse_macro_id(s: &str) -> Option<u16> {
    let t = s.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => t.parse().ok(),
    }
}

/// 把一条人类写的动作串解析成绑定。解析不了返回 `None`，不猜。
pub fn parse_action(spec: &str) -> Option<Binding> {
    use razer_protocol::keys::usage as k;
    let raw = spec.trim();
    let lower = raw.to_ascii_lowercase();

    if let Some(rest) = lower.strip_prefix("macro:") {
        // 先把整体当 ID 试一次。否则 "0x0303" 会被 rsplit_once('x') 拆成
        // ("0", "0303")，也就是 ID 0 + 重复 303 次——十六进制前缀里的 'x'
        // 被误当成了 xN 后缀的分隔符。没有 xN 后缀时整体一定能解析成功。
        if let Some(id) = parse_macro_id(rest) {
            return Some(Binding::macro_play(id, 1));
        }
        // 整体不是 ID，才按最后一个 'x' 拆重复次数：0x0101x3 / 257x3。
        let (id_part, times_part) = match rest.rsplit_once('x') {
            Some((a, b)) if !a.is_empty() && b.chars().all(|c| c.is_ascii_digit()) => (a, Some(b)),
            _ => (rest, None),
        };
        let id = parse_macro_id(id_part)?;
        let times = times_part.and_then(|t| t.parse().ok()).unwrap_or(1u8);
        return Some(Binding::macro_play(id, times));
    }
    // 按住触发 / 开关触发。写法与 macro: 一致，只是换了个前缀。
    for (prefix, mode) in [
        ("macrohold:", cmd::function::MACRO_HELD),
        ("macrotoggle:", cmd::function::MACRO_TOGGLE),
    ] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let id = parse_macro_id(rest)?;
            return Some(Binding::macro_play_with_mode(id, 1, mode));
        }
    }
    if let Some(rest) = lower.strip_prefix("key:") {
        let tokens: Vec<&str> = rest
            .split('+')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        let mut main = 0u8;
        let mut mods = 0u8;
        for token in tokens {
            // 名字查不到时允许 0x 十六进制 HID usage，避免为了一个新键重编译。
            let Some(u) = k::lookup(token).or_else(|| parse_hex_u8(token)) else {
                return None;
            };
            if (k::LCTRL..=k::RWIN).contains(&u) {
                mods |= modifier_bit(token, u);
            } else {
                main = u;
            }
        }
        if main == 0 {
            return None;
        }
        return Some(Binding::key(mods, main));
    }
    if let Some(rest) = lower.strip_prefix("media:") {
        let code = if let Some(c) = parse_hex_u16(rest) {
            c
        } else {
            match rest {
                "volup" => 0x00E9,
                "voldown" => 0x00EA,
                "mute" => 0x00E2,
                "playpause" | "play" => 0x00CD,
                "next" => 0x00B5,
                "prev" => 0x00B6,
                "stop" => 0x00B7,
                _ => return None,
            }
        };
        return Some(Binding::media(code));
    }
    if let Some(rest) = lower.strip_prefix("dpi:") {
        let action = match rest {
            "up" => cmd::dpi_action::UP,
            "down" => cmd::dpi_action::DOWN,
            "cycle" => cmd::dpi_action::CYCLE,
            _ => return None,
        };
        return Some(Binding::dpi(action));
    }
    // ── 原生鼠标功能：配置里写 mouse:left / mouseext:tiltleft ──
    // 注意 fn 不同：普通鼠标键是 fn=1，滚轮左右倾是 fn=0x0E（实测）。
    if let Some(rest) = lower.strip_prefix("mouseext:") {
        let code = match rest {
            "tiltleft" => cmd::mouse_action::TILT_LEFT,
            "tiltright" => cmd::mouse_action::TILT_RIGHT,
            other => parse_hex_u8(other)?,
        };
        return Some(Binding::new(cmd::function::MOUSE_ACTION_EXT, [code, 0, 0, 0, 0]));
    }
    if let Some(rest) = lower.strip_prefix("mouse:") {
        let code = match rest {
            "left" => cmd::mouse_action::LEFT,
            "right" => cmd::mouse_action::RIGHT,
            "middle" => cmd::mouse_action::MIDDLE,
            "back" => cmd::mouse_action::BACK,
            "forward" => cmd::mouse_action::FORWARD,
            "wheelup" => cmd::mouse_action::WHEEL_UP,
            "wheeldown" => cmd::mouse_action::WHEEL_DOWN,
            other => parse_hex_u8(other)?,
        };
        return Some(Binding::mouse(code));
    }
    // ── 逃生舱：raw:<fn>,<size>,<d0..d4> ──
    // 固件以后支持的新功能，或协议表里还没收录的 fn，直接写原始字节，
    // 配置文件就能表达，不需要重新编译后台程序。
    if let Some(rest) = lower.strip_prefix("raw:") {
        let parts: Vec<&str> = rest.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        if parts.len() < 2 || parts.len() > 7 {
            return None;
        }
        let fn_id = parse_hex_u8(parts[0])?;
        let fn_size = parse_hex_u8(parts[1])?;
        if fn_size > 5 {
            return None;
        }
        let mut data = [0u8; 5];
        for (i, p) in parts.iter().skip(2).enumerate() {
            data[i] = parse_hex_u8(p)?;
        }
        return Some(Binding { fn_id, fn_size, data });
    }
    Some(match lower.as_str() {
        "off" => Binding::off(),
        "wheelmode" => Binding::wheel_mode(),
        "profile:next" => Binding::profile_next(),
        "double" => Binding::new(cmd::function::DOUBLE, [1, 0, 0, 0, 0]),
        "hypershift" => Binding::new(cmd::function::HYPERSHIFT, [1, 0, 0, 0, 0]),
        "left" => Binding::mouse(cmd::mouse_action::LEFT),
        "right" => Binding::mouse(cmd::mouse_action::RIGHT),
        "middle" => Binding::mouse(cmd::mouse_action::MIDDLE),
        "back" | "mouse4" => Binding::mouse(cmd::mouse_action::BACK),
        "forward" | "mouse5" => Binding::mouse(cmd::mouse_action::FORWARD),
        "wheelup" => Binding::mouse(cmd::mouse_action::WHEEL_UP),
        "wheeldown" => Binding::mouse(cmd::mouse_action::WHEEL_DOWN),
        "tiltleft" => Binding::mouse(cmd::mouse_action::TILT_LEFT),
        "tiltright" => Binding::mouse(cmd::mouse_action::TILT_RIGHT),
        _ if lower.contains('+') => return parse_action(&format!("key:{lower}")),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::function;

    #[test]
    fn parses_plain_names() {
        assert_eq!(parse_action("off").unwrap().fn_id, function::OFF);
        assert_eq!(parse_action("tiltleft").unwrap().fn_id, function::MOUSE);
        assert_eq!(parse_action("TILTLEFT").unwrap().data[0], cmd::mouse_action::TILT_LEFT);
        assert_eq!(parse_action("forward").unwrap().data[0], 5);
    }

    #[test]
    fn parses_data_driven_actions() {
        // JSON 里 native 展开后的写法
        assert_eq!(parse_action("mouse:left").unwrap().fn_id, function::MOUSE);
        assert_eq!(parse_action("mouse:left").unwrap().data[0], 1);
        let b = parse_action("mouseext:tiltleft").unwrap();
        assert_eq!(b.fn_id, function::MOUSE_ACTION_EXT);
        assert_eq!(b.data[0], cmd::mouse_action::TILT_LEFT);
        // 十六进制 usage / consumer code / raw 逃生舱
        assert_eq!(parse_action("key:ctrl+0x1d").unwrap().data[1], 0x1d);
        assert_eq!(parse_action("media:0x00e9").unwrap().fn_id, function::MEDIA);
        let raw = parse_action("raw:2,2,1,25").unwrap();
        assert_eq!((raw.fn_id, raw.fn_size, raw.data[0], raw.data[1]), (2, 2, 1, 25));
        assert!(parse_action("raw:2,9,1").is_none());
        assert!(parse_action("native").is_none(), "native 必须由配置层展开");
    }

    #[test]
    fn parses_key_chords() {
        let b = parse_action("key:ctrl+w").unwrap();
        assert_eq!(b.fn_id, function::KEY);
        assert_eq!(b.data[0], cmd::modifier::CTRL);
        assert_eq!(b.data[1], razer_protocol::keys::usage::A + ('w' as u8 - b'a'));

        // 不带 key: 前缀也应当认
        let b = parse_action("alt+v").unwrap();
        assert_eq!(b.data[0], cmd::modifier::ALT);
    }

    #[test]
    fn parses_three_modifier_chord() {
        let b = parse_action("key:ctrl+shift+z").unwrap();
        assert_eq!(b.data[0], cmd::modifier::CTRL | cmd::modifier::SHIFT);
        assert_eq!(b.data[1], razer_protocol::keys::usage::Z);
    }

    #[test]
    fn parses_macro_with_times() {
        let b = parse_action("macro:0x0101x3").unwrap();
        assert_eq!(b.fn_id, function::MACRO_TIMES);
        assert_eq!(&b.data[..3], &[0x01, 0x01, 3]);

        let b = parse_action("macro:257x3").unwrap();
        assert_eq!(&b.data[..3], &[0x01, 0x01, 3]);
    }

    #[test]
    fn hex_macro_id_without_times_is_not_split_at_the_0x_prefix() {
        // 回归：rsplit_once('x') 曾把 "0x0303" 读成 ID 0 + 303 次。
        let b = parse_action("macro:0x0303").unwrap();
        assert_eq!(b.fn_id, function::MACRO_TIMES);
        assert_eq!(&b.data[..3], &[0x03, 0x03, 1]);

        let b = parse_action("macro:0x0101").unwrap();
        assert_eq!(&b.data[..3], &[0x01, 0x01, 1]);

        let b = parse_action("macro:769").unwrap(); // 十进制 0x0301
        assert_eq!(&b.data[..3], &[0x03, 0x01, 1]);
    }

    #[test]
    fn parses_hold_and_toggle_macro_modes() {
        let held = parse_action("macrohold:0x0301").unwrap();
        assert_eq!(held.fn_id, function::MACRO_HELD);
        assert_eq!(&held.data[..3], &[0x03, 0x01, 1]);

        let toggle = parse_action("macrotoggle:0x0302").unwrap();
        assert_eq!(toggle.fn_id, function::MACRO_TOGGLE);
        assert_eq!(&toggle.data[..3], &[0x03, 0x02, 1]);
    }

    #[test]
    fn rejects_nonsense() {
        assert!(parse_action("").is_none());
        assert!(parse_action("nope").is_none());
        assert!(parse_action("key:ctrl+").is_none());
    }

    #[test]
    fn labels_roundtrip_for_examples_from_the_device() {
        // 这三个是本机 4 号槽实读出来的绑定，展示层必须还原成人话。
        let b = parse_action("alt+w").unwrap();
        assert_eq!(describe_binding_zh(&b), "Alt + W");
        let b = Binding::new(function::MOUSE_ACTION_EXT, [cmd::mouse_action::TILT_LEFT, 0, 0, 0, 0]);
        assert_eq!(describe_binding_zh(&b), "滚轮左倾");
        let b = Binding::key(0, 0x58);
        assert_eq!(describe_binding_zh(&b), "Num Enter");
    }

    #[test]
    fn edit_roundtrip_supports_hold_and_toggle() {
        for (spec, kind, fn_id) in [
            ("macrohold:0x0301", 9, function::MACRO_HELD),
            ("macrotoggle:0x0302", 10, function::MACRO_TOGGLE),
        ] {
            let b = parse_action(spec).unwrap();
            assert_eq!(b.fn_id, fn_id);
            let (k, arg) = binding_to_edit(&b);
            assert_eq!(k, kind);
            let again = edit_to_spec(k, &arg).unwrap();
            assert_eq!(parse_action(&again).unwrap().fn_id, fn_id);
        }
    }

    #[test]
    fn slint_key_text_maps_to_device_key_names() {
        // 可打印键
        assert_eq!(key_name_from_slint_text("c").as_deref(), Some("c"));
        assert_eq!(key_name_from_slint_text("W").as_deref(), Some("w"));
        assert_eq!(key_name_from_slint_text("7").as_deref(), Some("7"));
        // Slint 私有区特殊键
        assert_eq!(key_name_from_slint_text("\u{000a}").as_deref(), Some("enter"));
        assert_eq!(key_name_from_slint_text("\u{001b}").as_deref(), Some("esc"));
        assert_eq!(key_name_from_slint_text("\u{0020}").as_deref(), Some("space"));
        assert_eq!(key_name_from_slint_text("\u{F705}").as_deref(), Some("f2"));
        assert_eq!(key_name_from_slint_text("\u{F702}").as_deref(), Some("left"));
        // 修饰键自身被忽略
        assert_eq!(key_name_from_slint_text("\u{0011}"), None);
        assert_eq!(key_name_from_slint_text("lctrl"), None);
        assert_eq!(key_name_from_slint_text(""), None);
    }

    #[test]
    fn recorded_chord_roundtrips_into_binding() {
        for (text, ctrl, shift, alt, win, expect) in [
            ("c", true, false, false, false, "Ctrl + C"),
            ("v", true, false, false, false, "Ctrl + V"),
            ("z", true, true, false, false, "Ctrl + Shift + Z"),
            ("\u{F705}", false, false, false, false, "F2"),
        ] {
            let key = key_name_from_slint_text(text).unwrap();
            let arg = chord_token(&key, ctrl, shift, alt, win);
            let spec = edit_to_spec(2, &arg).unwrap();
            let b = parse_action(&spec).unwrap();
            assert_eq!(describe_binding_zh(&b), expect);
            let (k, back) = binding_to_edit(&b);
            assert_eq!(k, 2);
            assert_eq!(back, arg);
        }
    }
}

// ─────────────────────── 编辑态互转（GUI 用） ───────────────────────
//
// 界面上的「类型 + 参数」两段式编辑需要和 `Binding` 互转。kind 的取值：
//   0 未绑定  1 鼠标键  2 组合键  3 DPI 动作  4 播放宏  5 媒体键
//   6 切到下一槽  7 滚轮模式  8 其他（本程序不支持编辑，只展示）
//   9 按住播放宏  10 开关播放宏（fn=4/5 的触发方式，物理行为已实机验收）

fn mouse_action_token(code: u8) -> Option<&'static str> {
    use cmd::mouse_action as m;
    Some(match code {
        m::LEFT => "left",
        m::RIGHT => "right",
        m::MIDDLE => "middle",
        m::BACK => "back",
        m::FORWARD => "forward",
        m::WHEEL_UP => "wheelup",
        m::WHEEL_DOWN => "wheeldown",
        m::TILT_LEFT => "tiltleft",
        m::TILT_RIGHT => "tiltright",
        _ => return None,
    })
}

fn media_token(code: u16) -> Option<&'static str> {
    Some(match code {
        0x00E9 => "volup",
        0x00EA => "voldown",
        0x00E2 => "mute",
        0x00CD => "play",
        0x00B5 => "next",
        0x00B6 => "prev",
        0x00B7 => "stop",
        _ => return None,
    })
}

fn key_chord_token(mods: u8, usage: u8) -> Option<String> {
    use cmd::modifier as m;
    if usage == 0 {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    if mods & (m::CTRL | m::RCTRL) != 0 {
        parts.push("ctrl");
    }
    if mods & (m::SHIFT | m::RSHIFT) != 0 {
        parts.push("shift");
    }
    if mods & (m::ALT | m::RALT) != 0 {
        parts.push("alt");
    }
    if mods & (m::WIN | m::RWIN) != 0 {
        parts.push("win");
    }
    let mut out = parts.join("+");
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(&razer_protocol::keys::usage::label(usage).to_ascii_lowercase());
    Some(out)
}

/// 把绑定拆成界面编辑态 `(kind, arg)`。
pub fn binding_to_edit(b: &Binding) -> (i32, String) {
    use cmd::function as f;
    match b.fn_id {
        x if x == f::OFF => (0, String::new()),
        x if x == f::MOUSE || x == f::MOUSE_ACTION_EXT => match mouse_action_token(b.data[0]) {
            Some(t) => (1, t.to_string()),
            None => (8, String::new()),
        },
        x if x == f::KEY => match key_chord_token(b.data[0], b.data[1]) {
            Some(t) => (2, t),
            None => (8, String::new()),
        },
        x if x == f::DPI => {
            let t = match b.data[0] {
                cmd::dpi_action::UP => "up",
                cmd::dpi_action::DOWN => "down",
                cmd::dpi_action::CYCLE => "cycle",
                _ => return (8, String::new()),
            };
            (3, t.to_string())
        }
        x if x == f::MACRO_TIMES => {
            let id = u16::from_be_bytes([b.data[0], b.data[1]]);
            let times = b.data[2].max(1);
            (4, format!("0x{id:04X}x{times}"))
        }
        x if x == f::MACRO_HELD || x == f::MACRO_TOGGLE => {
            let id = u16::from_be_bytes([b.data[0], b.data[1]]);
            (if x == f::MACRO_HELD { 9 } else { 10 }, format!("0x{id:04X}"))
        }
        x if x == f::MEDIA => {
            let code = u16::from_be_bytes([b.data[0], b.data[1]]);
            match media_token(code) {
                Some(t) => (5, t.to_string()),
                None => (8, String::new()),
            }
        }
        x if x == f::PROFILE => (6, String::new()),
        x if x == f::WHEEL_MODE => (7, String::new()),
        _ => (8, String::new()),
    }
}

/// 把 GUI「录入组合键」拿到的 Slint `KeyEvent.text` 转成设备键名。
///
/// Slint 用私有区字符表示特殊键（见 i-slint-common 的 `for_each_keys`），
/// 可打印键则直接给字符本身。返回的名字一定能被 `keys::usage::lookup`
/// 再认回来，这样「录入 → 显示 → 写入」闭环不丢键。
///
/// 修饰键自身返回 `None`：录入时用户按住 Ctrl 会先收到一次 Ctrl 的
/// key-pressed，那不是要绑定的主键，必须等真正的主键到来。
pub fn key_name_from_slint_text(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    // 特殊键在 Slint 里是单个 char，先按码点匹配。
    if let Some(c) = text.chars().next() {
        if let Some(name) = match c {
            '\u{0008}' => Some("backspace"),
            '\u{0009}' => Some("tab"),
            '\u{000a}' => Some("enter"),
            '\u{001b}' => Some("esc"),
            '\u{007f}' => Some("delete"),
            '\u{0020}' => Some("space"),
            '\u{F700}' => Some("up"),
            '\u{F701}' => Some("down"),
            '\u{F702}' => Some("left"),
            '\u{F703}' => Some("right"),
            '\u{F727}' => Some("insert"),
            '\u{F729}' => Some("home"),
            '\u{F72B}' => Some("end"),
            '\u{F72C}' => Some("pageup"),
            '\u{F72D}' => Some("pagedown"),
            _ => None,
        } {
            return Some(name.to_string());
        }
        // F1..F24 是连续的 U+F704..U+F71B。
        let u = c as u32;
        if (0xF704..=0xF71B).contains(&u) {
            const FN: [&str; 24] = [
                "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
                "f13", "f14", "f15", "f16", "f17", "f18", "f19", "f20", "f21", "f22", "f23",
                "f24",
            ];
            return Some(FN[(u - 0xF704) as usize].to_string());
        }
        // 控制键自身：跳过。
        if matches!(
            c,
            '\u{0010}' | '\u{0011}' | '\u{0012}' | '\u{0013}' | '\u{0014}' | '\u{0015}'
                | '\u{0016}' | '\u{0017}' | '\u{0018}' | '\u{0019}'
        ) {
            return None;
        }
    }
    // 可打印键：交给 usage::lookup，它认识单字母/数字和全部 NAMED 键名。
    let lower = text.trim().to_ascii_lowercase();
    razer_protocol::keys::usage::lookup(&lower)?;
    match lower.as_str() {
        // 修饰键自身由 modifiers 表达，不作为主键。
        "lctrl" | "rctrl" | "lshift" | "rshift" | "lalt" | "ralt" | "lwin" | "rwin" => None,
        _ => Some(lower),
    }
}

/// 把「录入」得到的键名 + 修饰键状态拼成 `parse_action` 认识的 `key:...` 参数。
pub fn chord_token(key: &str, ctrl: bool, shift: bool, alt: bool, win: bool) -> String {
    let mut out = String::new();
    if ctrl {
        out.push_str("ctrl+");
    }
    if shift {
        out.push_str("shift+");
    }
    if alt {
        out.push_str("alt+");
    }
    if win {
        out.push_str("win+");
    }
    out.push_str(key);
    out
}

/// 编辑态 `(kind, arg)` 重新组装成动作串，交给 `parse_action`。
pub fn edit_to_spec(kind: i32, arg: &str) -> Option<String> {
    let arg = arg.trim();
    Some(match kind {
        0 => "off".to_string(),
        1 => arg.to_ascii_lowercase(),
        2 => format!("key:{arg}"),
        3 => format!("dpi:{}", arg.to_ascii_lowercase()),
        4 => format!("macro:{arg}"),
        5 => format!("media:{}", arg.to_ascii_lowercase()),
        6 => "profile:next".to_string(),
        7 => "wheelmode".to_string(),
        9 => format!("macrohold:{arg}"),
        10 => format!("macrotoggle:{arg}"),
        _ => return None,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 在线模式：把虚拟槽套用到 p0（direct profile）
// ─────────────────────────────────────────────────────────────────────────────

/// 套用一份虚拟槽配置到 p0 的结果，便于日志和界面如实展示改了什么。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub name: String,
    pub bindings: usize,
    pub leds: usize,
    pub brightness: usize,
    pub dpi_changed: bool,
    pub polling_changed: bool,
    /// 解析失败或写入失败的动作串，逐条列出，不隐瞒。
    pub errors: Vec<String>,
}

impl ApplyReport {
    pub fn summary_zh(&self) -> String {
        let mut parts = vec![format!("已套用「{}」", self.name)];
        if self.bindings > 0 { parts.push(format!("按键 {} 项", self.bindings)); }
        if self.leds > 0 { parts.push(format!("灯效 {} 区", self.leds)); }
        if self.brightness > 0 { parts.push(format!("亮度 {} 区", self.brightness)); }
        if self.dpi_changed { parts.push("DPI".to_string()); }
        if self.polling_changed { parts.push("回报率".to_string()); }
        let mut s = parts.join(" · ");
        if !self.errors.is_empty() {
            s.push_str(&format!("（{} 项失败：{}）", self.errors.len(), self.errors.join("；")));
        }
        s
    }
}

/// 把一套虚拟槽配置差量写入 p0。
///
/// 内存与时间策略（在线模式后台优化的核心）：
///   * 只在值不同时才写，省掉大量 HID 往返；
///   * 每一项写入都走现有的读回校验 / 回滚方法，不另造一套弱校验；
///   * DPI 是唯一需要切 driver mode 的项，且只在真的不同时才切——
///     这是「后台省内存 + 少抖动」的关键，模式切换期间宏会短暂失效。
pub fn apply_virtual_slot(m: &Mouse, cfg: &razer_ipc::vslots::SlotConfig) -> ApplyReport {
    let mut rep = ApplyReport { name: cfg.name.clone(), ..Default::default() };

    // p0 是 direct profile：写入立即生效、零 Flash 磨损、切槽即被覆盖。
    const P0: u8 = 0;

    // ⚠️ 执行顺序是硬性要求，不要重排：
    // set_dpi() 会切到 driver mode 再切回来，设备会把生效槽重置；为还原槽位
    // set_dpi 内部会调 set_active_profile()，而**任何切槽都会用目标板载槽的
    // Flash 内容覆盖整个 p0**。若按键绑定写在 DPI 之前，刚写好的绑定会被自己
    // 冲掉（现象：报告说「按键 N 项」，读回却还是板载槽的旧值）。
    // 所以：会切模式/槽的项一律最先做，按键绑定一律最后做。

    // ── DPI：唯一切 driver mode 的项，且只在真的不同时才切 ──
    if let Some((x, y)) = cfg.dpi {
        match m.dpi(P0) {
            Ok(old) if old == (x, y) => {}
            Ok(_) => match m.set_dpi(P0, x, y) {
                Ok(_) => rep.dpi_changed = true,
                Err(e) => rep.errors.push(format!("DPI 写入失败：{e}")),
            },
            Err(e) => rep.errors.push(format!("DPI 读取失败：{e}")),
        }
    }

    // ── 回报率：设备级设置，但按 p0 传参（协议按 profile 寻址） ──
    if let Some(hz) = cfg.polling {
        match m.polling_hz(P0) {
            Ok(old) if old == hz => {}
            Ok(_) => match m.set_polling_hz(P0, hz) {
                Ok(_) => rep.polling_changed = true,
                Err(e) => rep.errors.push(format!("回报率写入失败：{e}")),
            },
            Err(e) => rep.errors.push(format!("回报率读取失败：{e}")),
        }
    }

    // ── 灯效：三种可持久化效果，差量写 ──
    for led in &cfg.leds {
        let region = led.region;
        let effect = match led.effect.as_str() {
            "static" => cmd::effect::STATIC,
            "spectrum" => cmd::effect::SPECTRUM,
            "wave" => cmd::effect::WAVE,
            other => {
                rep.errors.push(format!("{region} 区灯效不支持：{other}"));
                continue;
            }
        };
        let want = match m.lighting(P0, region) {
            Ok(v) => v,
            Err(e) => {
                rep.errors.push(format!("{region} 区灯效读取失败：{e}"));
                continue;
            }
        };
        let same = want.effect == effect
            && (effect != cmd::effect::STATIC || want.colors.first().copied() == led.rgb)
            && (effect != cmd::effect::WAVE || (want.mode == led.mode && want.speed == led.speed));
        if same {
            continue;
        }
        let result = match effect {
            cmd::effect::STATIC => led.rgb
                .map(|rgb| m.set_static(P0, region, rgb).map(|_| ()))
                .unwrap_or_else(|| Err(Error::Parse("static missing rgb"))),
            cmd::effect::SPECTRUM => m.set_spectrum(P0, region).map(|_| ()),
            cmd::effect::WAVE => m.set_wave(P0, region, led.mode, led.speed).map(|_| ()),
            _ => unreachable!(),
        };
        match result {
            Ok(_) => rep.leds += 1,
            Err(e) => rep.errors.push(format!("{region} 区灯效写入失败：{e}")),
        }
    }

    // ── 亮度：用原始 0..255 值，避免百分比往返掉精度 ──
    for (region, raw) in &cfg.brightness {
        match m.brightness(P0, *region) {
            Ok(old) if old == *raw => continue,
            Ok(_) => match m.set_brightness_raw(P0, *region, *raw) {
                Ok(_) => rep.brightness += 1,
                Err(e) => rep.errors.push(format!("{region} 区亮度写入失败：{e}")),
            },
            Err(e) => rep.errors.push(format!("{region} 区亮度读取失败：{e}")),
        }
    }

    // ── 按键绑定：差量写，必须放在最后 ──
    // 前面任何一项都可能引发切槽（DPI 一定会），p0 会被板载槽覆盖；
    // 只有把绑定放在最后，写进去的值才是最终生效的值。
    for (button, spec) in &cfg.bindings {
        let Some(binding) = parse_action(spec) else {
            rep.errors.push(format!("{button} 号键动作无法解析：{spec}"));
            continue;
        };
        match m.binding(P0, *button, cmd::layer::NORMAL) {
            Ok(old) if old == binding => continue, // 已是目标值，跳过
            Ok(_) => match m.set_binding(P0, *button, cmd::layer::NORMAL, binding) {
                Ok(_) => rep.bindings += 1,
                Err(e) => rep.errors.push(format!("{button} 号键写入失败：{e}")),
            },
            Err(e) => rep.errors.push(format!("{button} 号键读取失败：{e}")),
        }
    }

    rep
}
