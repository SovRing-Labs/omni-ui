// SPDX-License-Identifier: MIT OR Apache-2.0
//! Orbit — Omni Web's chrome (BROWSER-PRODUCT-0, design approved by author 2026-10-07; spec docs/systems/omni-ui/WEB-DESIGN.md).
//! The page owns the screen; the chrome floats over it as glass and folds away: with *Hide bar* on, after 5 s unused the bar
//! slides left into the core icon top-left, and touching that icon springs it back. Settings sit behind one icon top-right.
//! Every panel grows out of the control that called it. The markup is drawn by our own engine (paint.rs: rounded glass,
//! shadows, gradients, backdrop blur) over the page's pixels; each widget is an element `id="w<i>"` of the app's tree, so focus,
//! hit tests and the AT-SPI tree stay the tree's. Motion is a spring integrated per tick — no resident animation loop.
use crate::widgets::{esc, Kind, Tree};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel { None, Settings, Site, Cmd }

pub struct Orbit {
    pub hide: bool,       // Settings → Hide bar (author 10-07)
    pub folded: bool,     // target state of the bar
    x: f32, v: f32,       // spring: 0 = open … 1 = folded, and its velocity per ms
    pub last_use: Instant,
    pub theme: u8,        // 0 dark · 1 light · 2 contrast
    pub solid: bool,      // surfaces solid instead of glass (contrast held on any page)
    pub still: bool,      // motion off
    pub panel: Panel,
    /// the panel being drawn (stays while it fades out) and its fade (0 hidden … 1 shown), on its own spring
    shown: Panel, pa: f32, pv: f32,
    pub hover_bar: bool,
    spin_until: Instant,  // the core's rings turn while the page loads / the bar moves
    t0: Instant,
    status_seen: std::cell::RefCell<(String, Instant)>,
    dir: Option<std::path::PathBuf>,
}

/// What the markup shows besides the tree: the page's identity and the site trit (connection · cookies · third parties).
pub struct View { pub site: String, pub host: String, pub path: String, pub trit: [i8; 3], pub trit_text: [String; 3], pub loading: bool, pub editing: bool, pub reader: bool }

const FOLD_AFTER_MS: u128 = 5000;

impl Orbit {
    pub fn load(dir: Option<std::path::PathBuf>) -> Orbit {
        let now = Instant::now();
        let mut o = Orbit { hide: false, folded: false, x: 0.0, v: 0.0, last_use: now, theme: 0, solid: false, still: false, panel: Panel::None, shown: Panel::None, pa: 0.0, pv: 0.0,
            hover_bar: false, spin_until: now, t0: now, status_seen: std::cell::RefCell::new((String::new(), now)), dir };
        if let Some(d) = &o.dir {
            for l in std::fs::read_to_string(d.join("orbit.tsv")).unwrap_or_default().lines() {
                match l.split_once('\t') {
                    Some(("hide", v)) => o.hide = v == "1", Some(("theme", v)) => o.theme = v.parse().unwrap_or(0).min(2),
                    Some(("solid", v)) => o.solid = v == "1", Some(("still", v)) => o.still = v == "1", _ => {}
                }
            }
        }
        o
    }
    pub fn save(&self) {
        let Some(d) = &self.dir else { return };
        let _ = std::fs::create_dir_all(d);
        let body = format!("hide\t{}\ntheme\t{}\nsolid\t{}\nstill\t{}\n", self.hide as u8, self.theme, self.solid as u8, self.still as u8);
        let tmp = d.join("orbit.tsv.tmp");
        if std::fs::write(&tmp, body).is_ok() { let _ = std::fs::rename(&tmp, d.join("orbit.tsv")); }
    }

    /// Any use of the chrome: the 5 s count restarts and a folded bar comes back.
    pub fn touch(&mut self) { self.last_use = Instant::now(); if self.folded { self.unfold(); } }
    pub fn unfold(&mut self) { if self.folded { self.folded = false; self.spin(700); } self.last_use = Instant::now(); }
    /// The panel drawn now (open, or fading out): its choices stay in the tree until it has gone.
    pub fn shown_panel(&self) -> Panel { if self.panel != Panel::None { self.panel } else { self.shown } }
    pub fn fold(&mut self) { if !self.folded { self.folded = true; self.panel = Panel::None; self.spin(700); } }
    pub fn spin(&mut self, ms: u64) { let t = Instant::now() + std::time::Duration::from_millis(ms); if t > self.spin_until { self.spin_until = t; } }
    pub fn set_hide(&mut self, on: bool) { self.hide = on; if !on { self.unfold(); } self.last_use = Instant::now(); self.save(); }

    /// The bar's rectangle in window px at the current spring position (x, y, w, h).
    pub fn bar(&self, w: usize) -> (f32, f32, f32, f32) {
        let e = self.x.clamp(-0.15, 1.15);
        let full = (w as f32 - 140.0).clamp(240.0, 660.0);
        let bw = full + (48.0 - full) * e;
        let open_x = (w as f32 - full) / 2.0;
        (open_x + (14.0 - open_x) * e, 14.0, bw.max(40.0), 48.0)
    }

    /// Pointer at (x, y): over the folded icon → spring back; over the open bar → it counts as use. True = redraw.
    pub fn pointer(&mut self, x: i32, y: i32, w: usize) -> bool {
        let (bx, by, bw, bh) = self.bar(w);
        let over = (x as f32) >= bx && (x as f32) <= bx + bw && (y as f32) >= by && (y as f32) <= by + bh + 6.0;
        let was = self.hover_bar;
        self.hover_bar = over;
        if over { self.last_use = Instant::now(); if self.folded { self.unfold(); return true; } }
        was != over && !self.folded
    }

    /// Advance the spring and the fold timer. True = the chrome changed (redraw).
    pub fn tick(&mut self, ms: u64, editing: bool) -> bool {
        let mut changed = false;
        if self.hide && !self.folded && !self.hover_bar && !editing && self.panel == Panel::None && self.last_use.elapsed().as_millis() >= FOLD_AFTER_MS {
            self.fold(); changed = true;
        }
        let target = if self.folded { 1.0 } else { 0.0 };
        if self.still { if self.x != target { self.x = target; self.v = 0.0; changed = true; } }
        else if (self.x - target).abs() > 0.002 || self.v.abs() > 0.0005 {
            // under-damped spring (a small overshoot = the "jump" back), integrated in 4 ms steps
            let (k, c) = (0.00055f32, 0.036f32);
            let mut left = ms.min(64) as f32;
            while left > 0.0 { let dt = left.min(4.0); self.v += (-k * (self.x - target) - c * self.v) * dt; self.x += self.v * dt; left -= dt; }
            if (self.x - target).abs() < 0.002 && self.v.abs() < 0.0005 { self.x = target; self.v = 0.0; }
            changed = true;
        }
        // panel fade: a quick, nearly critical spring (≈ 180 ms); a new panel replaces the shown one at once
        if self.panel != Panel::None && self.panel != self.shown { self.shown = self.panel; if self.pa > 0.5 { self.pa = 0.35; } }
        let pt = if self.panel == Panel::None { 0.0 } else { 1.0 };
        if self.still { if self.pa != pt { self.pa = pt; changed = true; } }
        else if (self.pa - pt).abs() > 0.004 || self.pv.abs() > 0.0005 {
            let (k, c) = (0.0011f32, 0.062f32);
            let mut left = ms.min(64) as f32;
            while left > 0.0 { let dt = left.min(4.0); self.pv += (-k * (self.pa - pt) - c * self.pv) * dt; self.pa += self.pv * dt; left -= dt; }
            if (self.pa - pt).abs() < 0.004 && self.pv.abs() < 0.0005 { self.pa = pt; self.pv = 0.0; }
            changed = true;
        }
        if self.panel == Panel::None && self.pa <= 0.0 && self.shown != Panel::None { self.shown = Panel::None; changed = true; }
        if Instant::now() < self.spin_until && !self.still { changed = true; }
        changed
    }
    /// Whether the chrome wants frame ticks now: moving (16 ms) or waiting to fold (250 ms).
    pub fn tick_every(&self) -> Option<std::time::Duration> {
        let target = if self.folded { 1.0 } else { 0.0 };
        let pt = if self.panel == Panel::None { 0.0 } else { 1.0 };
        let panel_moving = self.panel != self.shown || (self.pa - pt).abs() > 0.004 || self.pv.abs() > 0.0005;
        if (self.x - target).abs() > 0.002 || self.v.abs() > 0.0005 || panel_moving || (Instant::now() < self.spin_until && !self.still) { return Some(std::time::Duration::from_millis(16)); }
        if self.hide && !self.folded { return Some(std::time::Duration::from_millis(250)); }
        None
    }

    /// Contrast law (author 10-07, accessibility principle): glass is translucent, so text colours are held to ≥ 4.5:1 against the glass
    /// over the WORST page (white / black / grey / red) — measured, not eyeballed: dark ink-2 7.4 · ink-3 5.6, light ink-2 7.1 · ink-3 5.4 at glass α .88.
    pub fn css(&self) -> String {
        let (void, ink, ink2, ink3, glass, hi, edge, lo, acc, acc2, glow, pos, zero, neg) = match self.theme {
            1 => ("#eef2fa", "#0d1324", "#3c4760", "#4d5870", "rgba(255,255,255,.88)", "rgba(255,255,255,.75)", "rgba(13,19,36,.12)", "rgba(13,19,36,.05)", "#2563eb", "#7c3aed", "rgba(37,99,235,.30)", "#0f9d63", "#b7791f", "#d23c56"),
            2 => ("#000000", "#ffffff", "#ffffff", "#ffffff", "#000000", "#000000", "#ffffff", "#262626", "#ffe600", "#ffe600", "rgba(255,230,0,0)", "#00ff9c", "#ffe600", "#ff5c7a"),
            _ => ("#05070d", "#e9eefb", "#c4ccdf", "#a9b3cc", "rgba(22,28,44,.88)", "rgba(255,255,255,.10)", "rgba(255,255,255,.14)", "rgba(255,255,255,.05)", "#7cc7ff", "#b48cff", "rgba(124,199,255,.45)", "#4ee6a5", "#ffd166", "#ff6b81"),
        };
        let glass = if self.solid || self.theme == 2 { match self.theme { 1 => "#ffffff", 2 => "#000000", _ => "#121828" } } else { glass };
        let blur = if self.solid || self.theme == 2 { "" } else { "backdrop-filter:blur(22px) saturate(160%);" };
        format!(":root{{--void:{void};--ink:{ink};--ink-2:{ink2};--ink-3:{ink3};--edge:{edge};--lo:{lo};--acc:{acc};--acc-2:{acc2};--glow:{glow};--pos:{pos};--zero:{zero};--neg:{neg}}}\
html,body{{margin:0;background:transparent}}\
.reader{{position:absolute;color:var(--ink);font-family:serif;font-size:19px;line-height:31px}}\
.reader h1,.reader h2,.reader h3,.reader h4{{font-family:system-ui,sans-serif;color:var(--ink);line-height:1.25;margin:28px 0 12px 0}}\
.reader h1{{font-size:34px}}.reader h2{{font-size:25px}}.reader h3{{font-size:20px}}\
.reader .para{{margin:0 0 18px 0;font-size:19px;line-height:31px;color:var(--ink)}}\
.reader .link{{color:var(--acc);text-decoration:none}}.reader .link.focus{{background:var(--lo);box-shadow:0 0 0 2px var(--acc)}}\
.reader ol{{margin:0 0 18px 0;padding:0;list-style:none}}.reader li{{margin:0 0 8px 0;font-size:19px}}.reader .n{{display:inline-block;width:30px;color:var(--ink-3)}}\
.reader pre.code{{font-size:15px;line-height:22px;background:var(--lo);border-radius:10px;padding:12px;white-space:pre-wrap;font-family:monospace}}\
.reader .status{{color:var(--ink-3);font-size:14px}}\
body{{color:var(--ink);font-family:system-ui,sans-serif;font-size:14px;line-height:20px}}\
.glass{{background:{glass};{blur}border:1px solid var(--edge);box-shadow:inset 0 1px 0 {hi},0 18px 50px -18px rgba(0,0,0,.6)}}\
.bar{{position:absolute;border-radius:24px;overflow:hidden}}\
.row{{position:absolute;left:6px;top:6px;right:8px;height:36px;display:flex;align-items:center}}\
.ic{{width:34px;height:34px;border-radius:17px;display:flex;align-items:center;justify-content:center;color:var(--ink-2);flex:none;margin-right:4px}}\
.ic.focus{{box-shadow:0 0 0 2px var(--acc),0 0 14px 2px var(--glow);color:var(--ink)}}\
.ic.dim{{opacity:.35}}\
.where{{flex:1;display:flex;align-items:center;height:36px;padding:0 10px;border-radius:14px;color:var(--ink);overflow:hidden;white-space:nowrap}}\
.where.focus{{background:var(--lo);box-shadow:inset 0 0 0 2px var(--acc)}}\
.host{{font-weight:600;color:var(--ink)}}.path{{color:var(--ink-3)}}.ph{{color:var(--ink-3)}}\
.caret{{display:inline-block;width:2px;height:18px;background:var(--acc);margin-left:1px;vertical-align:middle}}\
.trit{{width:26px;height:26px;border-radius:13px;flex:none;margin-right:10px;display:flex;align-items:center;justify-content:center}}\
.trit .in{{width:20px;height:20px;border-radius:10px;background:var(--void);color:var(--ink);font-weight:700;font-size:11px;line-height:20px;text-align:center}}\
.gear{{position:absolute;top:14px;width:48px;height:48px;border-radius:24px;display:flex;align-items:center;justify-content:center;color:var(--ink-2)}}\
.gear.focus{{box-shadow:0 0 0 2px var(--acc),0 0 14px 2px var(--glow)}}\
.panel{{position:absolute;border-radius:16px;padding:10px 14px;color:var(--ink)}}\
.panel h2{{margin:2px 0 6px 0;font-size:16px;line-height:22px}}\
.prow{{display:flex;align-items:center;padding:7px 0;border-top:1px solid var(--lo)}}\
.prow .lbl{{flex:1}}.prow small{{display:block;color:var(--ink-3);font-size:12px;line-height:16px}}\
.opt{{padding:3px 10px;border-radius:12px;font-size:12px;color:var(--ink-3);margin-left:2px}}\
.opt.on{{background:var(--acc);color:var(--void);font-weight:600}}\
.opt.focus{{box-shadow:0 0 0 2px var(--acc)}}\
.sug{{display:flex;align-items:center;padding:8px 12px;border-radius:12px;color:var(--ink-2)}}\
.sug.focus{{background:var(--lo);color:var(--ink);box-shadow:inset 2px 0 0 var(--acc)}}\
.sug .why{{margin-left:auto;font-size:12px;color:var(--ink-3)}}\
.grp{{margin:8px 12px 4px 12px;font-size:11px;letter-spacing:1px;color:var(--ink-3);font-weight:600}}\
.rail{{position:absolute;left:14px;border-radius:22px;padding:6px}}\
.tab{{display:flex;align-items:center;justify-content:center;width:34px;height:34px;border-radius:17px;margin:4px 0;color:var(--ink);font-weight:700;font-size:11px;background:var(--lo)}}\
.tab.on{{box-shadow:inset 0 0 0 2px var(--acc)}}.tab.focus{{box-shadow:0 0 0 2px var(--acc),0 0 12px 2px var(--glow)}}\
.pill{{position:absolute;bottom:16px;border-radius:16px;padding:5px 14px;font-size:13px;color:var(--ink-2);white-space:nowrap;overflow:hidden}}\
.sweep{{position:absolute;left:18px;right:18px;bottom:0;height:2px;border-radius:1px}}")
    }

    /// The core: three rings, each turning at its own rhythm (3 / 5 / 7 s) on its own axis (VOCAB "jump rope"), drawn as the
    /// ellipse a tilted circle projects to; they move only while the page loads or the bar moves (lightweight at rest).
    fn core_svg(&self, size: i32) -> String {
        let t = (if Instant::now() < self.spin_until && !self.still { self.t0.elapsed().as_secs_f32() } else { 0.0 }) * if self.still { 0.0 } else { 1.0 };
        let mut s = format!("<svg width=\"{size}\" height=\"{size}\" viewBox=\"0 0 24 24\">");
        for (k, (period, axis, col)) in [(3.0f32, 0.0f32, "var(--acc)"), (5.0, 60.0, "var(--acc-2)"), (7.0, 120.0, "var(--pos)")].iter().enumerate() {
            let ph = std::f32::consts::TAU * t / period + k as f32 * 0.9;
            let (rx, ry) = (9.0 - k as f32 * 1.6, ((9.0 - k as f32 * 1.6) * (0.35 + 0.3 * ph.cos())).abs().max(1.0));
            let colour = match self.theme { 2 => "#ffe600", 1 => ["#2563eb", "#7c3aed", "#0f9d63"][k], _ => ["#7cc7ff", "#b48cff", "#4ee6a5"][k] };
            let _ = col;
            s.push_str(&format!("<ellipse cx=\"12\" cy=\"12\" rx=\"{rx:.2}\" ry=\"{ry:.2}\" transform=\"rotate({axis} 12 12)\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"1.5\"/>"));
        }
        let dot = match self.theme { 1 => "#0d1324", 2 => "#ffffff", _ => "#e9eefb" };
        s.push_str(&format!("<circle cx=\"12\" cy=\"12\" r=\"2\" fill=\"{dot}\"/></svg>"));
        s
    }

    /// The whole chrome as markup for a w × h window.
    pub fn markup(&self, t: &Tree, focus: u32, w: usize, _h: usize, v: &View, status: &str, reader: Option<&str>, rscroll: i32) -> String {
        { let mut ss = self.status_seen.borrow_mut(); if status != ss.0 { *ss = (status.to_string(), Instant::now()); } }
        let find = |label: &str| (0..t.len() as u32).find(|&i| t.label(i) == label);
        let fc = |i: u32| if i == focus { " focus" } else { "" };
        let icon = |d: &str| format!("<svg width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\"><path d=\"{d}\"/></svg>");
        let (bx, by, bw, bh) = self.bar(w);
        let e = self.x.clamp(0.0, 1.0);
        let bg = ""; // reader view: the reader surface carries the background (drawn once, scrolled as pixels)
        let mut s = format!("<!doctype html><html><head><title>Omni Web</title><style>{}</style></head><body id=w0{bg}>", self.css());
        // reader view: a reading column under the bar, shifted up by its scroll
        if let Some(r) = reader {
            let cw = (w as i32 - 160).clamp(280, 720);
            // laid out but not painted: its pixels come from the reader surface (drawn once, scrolled as a page); the boxes keep
            // hit tests, focus and the AT tree
            s.push_str(&format!("<div class=reader style=\"left:{}px;top:{}px;width:{cw}px;visibility:hidden\">{r}</div>", (w as i32 - cw) / 2, 92 - rscroll));
        }
        // ── the bar ──
        s.push_str(&format!("<div class=\"bar glass\" style=\"left:{bx:.1}px;top:{by:.1}px;width:{bw:.1}px;height:{bh:.1}px\">"));
        let core = find("Omni core").unwrap_or(0);
        s.push_str(&format!("<div id=w{core} class=\"ic{}\" style=\"position:absolute;left:7px;top:7px\">{}</div>", fc(core), self.core_svg(26)));
        if e < 0.85 {
            let op = ((1.0 - e * 1.6).clamp(0.0, 1.0) * 100.0).round() / 100.0;
            s.push_str(&format!("<div class=row style=\"left:46px;opacity:{op}\">"));
            for (label, d) in [("Back", "M15 5l-7 7 7 7"), ("Forward", "M9 5l7 7-7 7")] {
                if let Some(i) = find(label) { s.push_str(&format!("<div id=w{i} class=\"ic{}\">{}</div>", fc(i), icon(d))); }
            }
            let site = find("Site").unwrap_or(0);
            let tc = |k: usize| match v.trit[k] { 1 => "var(--pos)", 0 => "var(--zero)", _ => "var(--neg)" };
            let letter: String = v.site.chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_else(|| "·".into());
            if let Some(a) = find("Address or search") {
                s.push_str(&format!("<div id=w{a} class=\"where{}\">", fc(a)));
                s.push_str(&format!("<div id=w{site} class=trit style=\"background:conic-gradient(from -60deg,{} 0 112deg,transparent 0 120deg,{} 0 232deg,transparent 0 240deg,{} 0 352deg,transparent 0)\"><div class=in>{}</div></div>",
                    tc(0), tc(1), tc(2), esc(&letter)));
                if v.editing {
                    let val = t.value(a);
                    if val.is_empty() { s.push_str("<span class=ph>Search or type an address</span>"); } else { s.push_str(&esc(val)); }
                    s.push_str("<span class=caret></span>");
                } else if v.host.is_empty() { s.push_str("<span class=ph>Search or type an address</span>"); }
                else { s.push_str(&format!("<span class=host>{}</span><span class=path>{}</span>", esc(&v.host), esc(&v.path))); }
                s.push_str("</div>");
            }
            for (label, d) in [("Reload", "M20 12a8 8 0 1 1-2.4-5.7M20 4v5h-5"), (if v.reader { "Page" } else { "Reader" }, "M4 6h16M4 11h16M4 16h10")] {
                if let Some(i) = find(label) { s.push_str(&format!("<div id=w{i} class=\"ic{}\">{}</div>", fc(i), icon(d))); }
            }
            s.push_str("</div>");
            if v.loading { s.push_str("<div class=sweep style=\"background:linear-gradient(90deg,transparent,var(--acc),var(--acc-2),transparent)\"></div>"); }
        }
        s.push_str("</div>");
        // ── settings icon, top-right ──
        if let Some(g) = find("Settings") {
            s.push_str(&format!("<div id=w{g} class=\"gear glass{}\" style=\"left:{}px\">{}</div>", fc(g), w as i32 - 62,
                icon("M12 9a3 3 0 1 0 0 6 3 3 0 1 0 0-6M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1")));
        }
        // ── tab rail (only with more than one tab, and with the bar) ──
        let tabs: Vec<u32> = (0..t.len() as u32).filter(|&i| t.kind[i as usize] == Kind::Button && t.label(i).starts_with("Tab ")).collect();
        if tabs.len() > 1 && e < 0.5 {
            let top = 90;
            s.push_str(&format!("<div class=\"rail glass\" style=\"top:{top}px\">"));
            for &i in &tabs {
                let l = t.label(i);
                let name = l.split_once(": ").map(|x| x.1).unwrap_or(l);
                let on = if l.ends_with("(shown)") { " on" } else { "" };
                let c = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_else(|| "·".into());
                s.push_str(&format!("<div id=w{i} class=\"tab{on}{}\">{}</div>", fc(i), esc(&c)));
            }
            if let Some(n) = find("New tab") { s.push_str(&format!("<div id=w{n} class=\"tab{}\">+</div>", fc(n))); }
            s.push_str("</div>");
        }
        // ── panels: each grows under the control that called it ──
        let pw = 380.0f32.min(w as f32 - 28.0);
        let op = (self.pa.clamp(0.0, 1.0) * 100.0).round() / 100.0;
        match self.shown {
            _ if op <= 0.0 => {}
            Panel::Settings => {
                s.push_str(&format!("<div class=\"panel glass\" style=\"left:{}px;top:72px;width:{}px;opacity:{op}\"><h2>Settings</h2>", w as f32 - pw - 14.0, pw - 30.0));
                for (title, sub, opts) in [
                    ("Hide bar", "folds into the top-left icon after 5 s unused · touch it to bring it back", vec!["Hide bar: off", "Hide bar: on"]),
                    ("Theme", "", vec!["Theme: dark", "Theme: light", "Theme: contrast"]),
                    ("Surfaces", "solid keeps text contrast on any page", vec!["Surfaces: glass", "Surfaces: solid"]),
                    ("Motion", "", vec!["Motion: fluid", "Motion: still"]),
                    ("Page scripts", "", vec!["Page scripts: off", "Page scripts: on"]),
                ] {
                    s.push_str(&format!("<div class=prow><div class=lbl>{title}{}</div>", if sub.is_empty() { String::new() } else { format!("<small>{sub}</small>") }));
                    for o in opts {
                        if let Some(i) = find(o) { let on = if t.value(i) == "on" { " on" } else { "" };
                            s.push_str(&format!("<div id=w{i} class=\"opt{on}{}\">{}</div>", fc(i), esc(o.split_once(": ").map(|x| x.1).unwrap_or(o)))); }
                    }
                    s.push_str("</div>");
                }
                s.push_str("</div>");
            }
            Panel::Site if e < 0.5 => {
                s.push_str(&format!("<div class=\"panel glass\" style=\"left:{:.0}px;top:72px;width:{:.0}px;opacity:{op}\"><h2>{}</h2>", (bx + 40.0).min(w as f32 - pw - 14.0), pw - 30.0, esc(&v.host)));
                for (k, name) in ["Connection", "Cookies", "Third parties"].iter().enumerate() {
                    let col = match v.trit[k] { 1 => "var(--pos)", 0 => "var(--zero)", _ => "var(--neg)" };
                    // the cookie switch (block · session · keep) sits on its own row under the label so the text keeps its width
                    let mut opts = String::new();
                    if k == 1 { for o in ["Cookies: blocked", "Cookies: this session", "Cookies: kept"] { if let Some(i) = find(o) {
                        let on = if t.value(i) == "on" { " on" } else { "" };
                        opts.push_str(&format!("<div id=w{i} class=\"opt{on}{}\">{}</div>", fc(i), if o.ends_with("kept") { "Keep" } else if o.ends_with("blocked") { "Block" } else { "Session" })); } } }
                    let opts = if opts.is_empty() { opts } else { format!("<div style=\"display:flex;margin-top:6px\">{opts}</div>") };
                    s.push_str(&format!("<div class=prow><div style=\"width:10px;height:10px;border-radius:5px;margin-right:12px;background:{col}\"></div><div class=lbl>{name}<small>{}</small>{opts}</div>",
                        esc(&v.trit_text[k])));
                    s.push_str("</div>");
                }
                s.push_str("</div>");
            }
            _ => {}
        }
        // command sheet: suggestions while the address is being edited
        let sugs: Vec<u32> = (0..t.len() as u32).filter(|&i| t.kind[i as usize] == Kind::Link && t.value(i).starts_with("http") && t.parent[i as usize] == 0).collect();
        if v.editing && e < 0.5 && !sugs.is_empty() {
            s.push_str(&format!("<div class=\"panel glass\" style=\"left:{bx:.1}px;top:68px;width:{bw:.1}px;padding:6px 6px\"><div class=grp>GO TO</div>"));
            for &i in sugs.iter().take(7) {
                s.push_str(&format!("<div id=w{i} class=\"sug{}\"><span>{}</span><span class=why>{}</span></div>", fc(i), esc(t.label(i)), esc(&crate::net::host_of(t.value(i)))));
            }
            s.push_str("</div>");
        }
        // status pill: visible for 4 s after it changes (the tree always carries it for AT)
        if let Some(st) = find_kind(t, Kind::Status) {
            if self.status_seen.borrow().1.elapsed().as_millis() < 4000 && !t.label(st).is_empty() {
                let pwid = (t.label(st).chars().count() as f32 * 7.4 + 30.0).min(w as f32 - 40.0);
                s.push_str(&format!("<div id=w{st} class=\"pill glass\" style=\"left:{:.0}px;max-width:{pwid:.0}px\">{}</div>", (w as f32 - pwid) / 2.0, esc(t.label(st))));
            }
        }
        s.push_str("</body></html>");
        s
    }

    /// The pill fades on a timer: keep ticking until it has gone.
    pub fn status_pending(&self) -> bool { self.status_seen.borrow().1.elapsed().as_millis() < 4200 }
}

fn find_kind(t: &Tree, k: Kind) -> Option<u32> { (0..t.len() as u32).find(|&i| t.kind[i as usize] == k) }
