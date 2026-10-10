// SPDX-License-Identifier: MIT OR Apache-2.0
//! The app contract: an OmniUI app is a widget tree + one `act(verb, arg, source)` for every input method. Focus ring,
//! activation, keys and the intent-socket line protocol are shared here, so every app gets key / pointer / AT-SPI / voice /
//! text / switch input the same way.
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::io::Write;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

/// access.bus Intent.action(): lower-case, whitespace collapsed.
pub fn canon(arg: &str) -> String { arg.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ") }

/// One action record per intent (OMNI_UI_LOG, JSONL). Same shape for every app: source + canonical action + app extras.
pub struct Log(Option<std::fs::File>);
impl Log {
    pub fn open() -> Log { Log(std::env::var("OMNI_UI_LOG").ok().and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())) }
    pub fn record(&mut self, verb: &str, arg: &str, source: &str, extra: Value) -> Value {
        let mut rec = json!({"t": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
                             "source": source, "action": [verb, canon(arg)]});
        if let (Some(r), Value::Object(e)) = (rec.as_object_mut(), extra) { r.extend(e); }
        if let Some(f) = self.0.as_mut() { let _ = f.write_all(format!("{rec}\n").as_bytes()); } // one write: never half a line
        rec
    }
}

pub trait App {
    fn title(&self) -> String;
    fn css(&self) -> String;
    fn tree(&self) -> Tree;
    fn focus_ord(&self) -> usize;
    fn set_focus_ord(&mut self, i: usize);
    /// The one entry point for every input method.
    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value;
    /// App state for the intent-socket reply (tests, other processes).
    fn state(&self) -> Value { json!({}) }
    /// The argument a widget carries when activated (default: a link's target).
    fn arg_for(&self, t: &Tree, w: u32, _verb: &str) -> String { if t.kind[w as usize] == Kind::Link { t.value(w).to_string() } else { String::new() } }
    /// App keys beyond the shared ones (editing, paging, size). Return Some(record) when a key became an intent.
    fn app_key(&mut self, _t: &Tree, _key: &str, _text: Option<&str>) -> Option<Value> { None }
    /// Vertical scroll offset in px, and the extent the frame measured (page height, view height).
    fn scroll(&self) -> i32 { 0 }
    fn set_extent(&mut self, _page_h: i32, _view_h: i32) {}
    fn wheel(&mut self, _dy: i32) {}
    /// Some((text px, horizontal padding px)) -> over-long tokens get soft breaks for drawing (fit.rs).
    fn fit(&self) -> Option<(i32, i32)> { None }
    /// Tab moves focus; true = Tab is a "next"/"back" intent (Find), false = a plain focus move (apps whose next/back page).
    fn tab_is_intent(&self) -> bool { true }
    /// A pixel surface below the widget chrome (the Web app's page, drawn by its own sandboxed renderer): `w` x `h`
    /// pixels (0x00RRGGBB) for the area under the chrome, or None (the app is widgets only).
    fn surface(&mut self, _w: usize, _h: usize) -> Option<Vec<u32>> { None }
    /// A live app redraws every frame (a firing core); others redraw on input only.
    fn live(&self) -> bool { false }
    /// A pointer press inside the surface, in surface px.
    fn surface_click(&mut self, _x: i32, _y: i32) -> Option<Value> { None }
    /// Real time for the app's own clock (the Web app's page timers): `ms` passed since the last tick; true = redraw.
    fn tick(&mut self, _ms: u64) -> bool { false }
    /// Some(period) while the app wants ticks (a page with timers waiting); None = idle (no wakeups).
    fn tick_every(&self) -> Option<Duration> { None }
    /// A middle (auxiliary) press inside the surface, in surface px.
    fn surface_aux_click(&mut self, _x: i32, _y: i32) -> Option<Value> { None }
    /// The surface's content as assistive technology reads it (the Web app's page): rows [role, name, parent, x, y, w, h,
    /// extra] in page px, the page scroll, and the row holding keyboard focus (a11y.rs mounts them under a Document node).
    fn page_a11y(&mut self) -> Option<(std::rc::Rc<Vec<Value>>, i32, Option<usize>)> { None }
    /// The app's own markup for its widgets (every widget still an element `id="w<i>"`, so hit tests, focus and the AT tree
    /// map back to the tree) — Omni Web's Orbit chrome (BROWSER-PRODUCT-0 C1). None = the generic widget markup.
    fn markup(&self, _t: &Tree, _focus: u32, _w: usize, _h: usize) -> Option<String> { None }
    /// The surface fills the whole window and the widgets are drawn OVER it (their glass blurs it); false = surface below them.
    fn overlay(&self) -> bool { false }
    /// The pointer moved to (x, y) window px; true = redraw (hover-driven chrome, e.g. the folded bar springing back).
    fn pointer_move(&mut self, _x: i32, _y: i32) -> bool { false }
    /// An assistive-technology action on page row `row`: "click", "focus" or "set_value" (with the value).
    fn page_action(&mut self, _row: usize, _action: &str, _value: Option<&str>) -> Option<Value> { None }

    fn focus_widget(&self, t: &Tree) -> u32 { let f = t.focusables(); if f.is_empty() { 0 } else { f[self.focus_ord().min(f.len() - 1)] } }
    fn set_focus(&mut self, t: &Tree, w: u32) { if let Some(p) = t.focusables().iter().position(|&x| x == w) { self.set_focus_ord(p); } }
    fn step_focus(&mut self, t: &Tree, d: i32) {
        let n = t.focusables().len().max(1) as i32;
        self.set_focus_ord(((self.focus_ord() as i32 + d).rem_euclid(n)) as usize);
    }

    /// Activate a widget (Enter on it, a click, an AT-SPI Click): its verb with the argument the widget carries.
    fn activate(&mut self, t: &Tree, w: u32, source: &str) -> Option<Value> {
        let mut w = w;
        while !t.kind[w as usize].interactive() { w = t.parent[w as usize]; if w == NONE { return None; } }
        self.set_focus(t, w);
        let verb = t.verb(w)?;
        let arg = self.arg_for(t, w, verb);
        Some(self.act(verb, &arg, source))
    }

    fn key(&mut self, t: &Tree, key: &str, text: Option<&str>, shift: bool) -> Option<Value> {
        match key {
            "Tab" if self.tab_is_intent() => Some(self.act(if shift { "back" } else { "next" }, "", "key")),
            "Tab" => { self.step_focus(t, if shift { -1 } else { 1 }); None }
            "Enter" => { let w = self.focus_widget(t); self.activate(t, w, "key") }
            _ => self.app_key(t, key, text),
        }
    }

    /// One line from the intent socket: an access.bus Intent ({"verb","arg","source"}) or a key ({"key","text","shift"}).
    fn handle_line(&mut self, t: &Tree, line: &str) -> Value {
        let v: Value = match serde_json::from_str(line) { Ok(v) => v, Err(e) => return json!({"ok": false, "error": e.to_string()}) };
        let action = if let Some(verb) = v["verb"].as_str() {
            Some(self.act(verb, v["arg"].as_str().unwrap_or(""), v["source"].as_str().unwrap_or("bus")))
        } else if v.get("key").is_some() || v.get("text").is_some() {
            self.key(t, v["key"].as_str().unwrap_or(""), v["text"].as_str(), v["shift"].as_bool().unwrap_or(false))
        } else { None };
        json!({"ok": true, "action": action, "focus_ord": self.focus_ord(), "state": self.state()})
    }
}
