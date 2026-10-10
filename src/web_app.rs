// SPDX-License-Identifier: MIT OR Apache-2.0
//! Web — BROWSER-BODY-0: a light-use web browser on OmniUI (author 10-06: replace Firefox for reading docs, wikis, news, search
//! results, following links — at a fraction of its RAM). This process is the BROKER: it owns the window, the network
//! (net.rs: no cookies, no referer) and the history; every page is parsed, styled, laid out and rastered by its own
//! locked-down page process (engine pageproc.rs: no_new_privs + Landlock + seccomp), which answers frames / hits / find.
//! Reader view = agent::markdown of the page drawn as widgets (the Notes renderer's blocks).
//! Intents: open <url | words>, back, forward, reload, scroll <±px | page | -page | top | end>, follow <link index | text>,
//! find <text> (empty = next match), click <x> <y> (surface px), type <text>, submit, read (reader view), bigger / smaller (zoom),
//! select <button label>.
use crate::app::{App, Log};
use crate::md::{blocks, part_starts, Block};
use crate::pageproc::{self, Broker, PageProc, Shot};
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Instant;

pub const CSS: &str = "body{margin:0;padding:6px 10px 0 10px;background:#ffffff;color:#111111;font-family:sans-serif;font-size:15px}\
.field{display:inline-block;padding:4px 8px;border:2px solid #777777;background:#ffffff;vertical-align:middle;margin:0 6px 4px 0}\
.field.focus{border:2px solid #0047b3}\
.ph{color:#595959}\
.caret{display:inline-block;width:2px;height:16px;background:#111111;margin-left:1px;vertical-align:middle}\
.btn{display:inline-block;margin:0 4px 4px 0;padding:4px 10px;border:2px solid #777777;background:#eeeeee;vertical-align:middle}\
.btn.focus{border:2px solid #0047b3;background:#dde8ff}\
.status{color:#333333;font-size:13px;margin:0 0 2px 0}\
h1,h2,h3,h4,h5,h6{margin:14px 0 8px 0}\
.para{margin:0 0 12px 0;font-size:18px;line-height:1.5}\
ol{margin:0 0 12px 0;padding:0;list-style:none}\
li{margin:0 0 6px 0;font-size:18px}\
.n{display:inline-block;width:30px;color:#333333}\
.link{display:inline;color:#0033aa;text-decoration:underline}\
.link.focus{background:#dde8ff}\
pre.code{font-size:14px;background:#f4f4f4;padding:8px;white-space:pre-wrap}";

/// A form control on the page that is being typed into (it is edited in the chrome: the page has no editing in v1).
/// `secret` = a password control: drawn and exposed to assistive technology as dots.
pub struct Field { pub form: Option<u64>, pub name: String, pub label: String, pub value: String, pub secret: bool }

/// A tab not on screen: its page process (None = asleep: reloaded at `url`, `y` when shown again) and its history.
pub struct Parked { page: Option<PageProc>, url: String, addr: String, title_s: String, back: Vec<(String, i32)>, fwd: Vec<(String, i32)>,
    y: i32, page_h: i32, http: u16, zoom: i32, load_ms: f64 }

pub struct WebApp {
    b: Broker,
    /// every tab; the shown one's slot is None (its state lives in the fields below)
    tabs: Vec<Option<Parked>>,
    pub cur_tab: usize,
    /// bookmarks (url, title) and the pages seen (history, newest last, one row per address): both saved in the data dir
    pub bookmarks: Vec<(String, String)>,
    pub seen: Vec<(String, String)>,
    data_dir: Option<std::path::PathBuf>,
    page: Option<PageProc>,
    pub url: String,
    pub addr: String,
    pub title_s: String,
    pub back: Vec<(String, i32)>,
    pub fwd: Vec<(String, i32)>,
    pub y: i32,
    pub page_h: i32,
    view: (usize, usize),
    shot: Option<Shot>,
    pub status: String,
    focus_ord: usize,
    log: Log,
    pub find_q: String,
    pub find_boxes: Vec<[i32; 4]>,
    pub find_ix: usize,
    pub reader: bool,
    md: String,
    pub part: usize,
    rscroll: i32,
    rext: (i32, i32),
    pub zoom: i32,
    pub field: Option<Field>,
    pub load_ms: f64,
    /// page scripts (JS-0, author 10-06): run by jsi inside the page process; `js` toggles and reloads
    pub js: bool,
    /// a form POST waiting for the next navigation: (content type, body)
    post: Option<(String, Vec<u8>)>,
    /// the shown page's HTTP status
    pub http: u16,
    /// what the person typed into this page's form controls, by control name (sent together on submit)
    pub form_vals: HashMap<String, String>,
    /// the page as assistive technology reads it (pageproc `W`), fetched on demand, dropped whenever the page re-lays out
    a11y_rows: Option<std::rc::Rc<Vec<Value>>>,
    /// keyboard focus inside the page: a row of `a11y_rows` (Tab / Shift-Tab walk the chrome, then the page's links and controls)
    pub pfocus: Option<usize>,
    /// the find-in-page field (Ctrl+F) while it is open
    pub find_edit: Option<String>,
    /// Ctrl+L selected the address: the next typed character replaces it
    addr_sel: bool,
    /// the page row whose field is being edited in the chrome (Tab continues from it in page order)
    field_row: Option<usize>,
    /// page timers still waiting after the last load / tick (the page keeps running while > 0)
    pub pending: u64,
    /// the Orbit chrome (BROWSER-PRODUCT-0 C1): fold, panels, theme, motion
    pub orbit: crate::orbit::Orbit,
    /// first tree index of the reader content (Orbit draws it as a reading column under the bar)
    reader_from: std::cell::Cell<u32>,
    /// reader column markup of the last frame, and the reading column drawn once at full height: (key, w, height, pixels)
    reader_html: std::cell::RefCell<String>,
    reader_img: Option<(u64, usize, usize, Vec<u32>)>,
    reader_fonts: Option<crate::font::Fonts>,
}

impl WebApp {
    pub fn new() -> WebApp {
        WebApp { b: Broker::new(), page: None, url: String::new(), addr: String::new(), title_s: String::new(), back: vec![], fwd: vec![],
                 y: 0, page_h: 0, view: (900, 582), shot: None, status: "Type an address or search words, then press Enter.".into(), focus_ord: 0,
                 log: Log::open(), find_q: String::new(), find_boxes: vec![], find_ix: 0, reader: false, md: String::new(), part: 0,
                 rscroll: 0, rext: (0, 0), zoom: 100, field: None, load_ms: 0.0, js: std::env::var("OMNI_WEB_JS").map(|v| v != "0").unwrap_or(true),
                 post: None, http: 0, form_vals: HashMap::new(), a11y_rows: None, pfocus: None, find_edit: None, addr_sel: false, field_row: None,
                 tabs: vec![None], cur_tab: 0, bookmarks: vec![], seen: vec![], data_dir: None, pending: 0,
                 orbit: crate::orbit::Orbit::load(None), reader_from: std::cell::Cell::new(u32::MAX),
                 reader_html: std::cell::RefCell::new(String::new()), reader_img: None, reader_fonts: None }.with_jar()
    }

    /// Cookies (cookie.rs, author 10-07): first-party only, in memory; sites the person chose to keep (`keep`) survive restarts in
    /// $XDG_DATA_HOME/omni-web (tests: memory only).
    fn with_jar(mut self) -> WebApp {
        if std::env::var_os("OMNI_UI_TEST").is_none() {
            let dir = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")).join("omni-web");
            *self.b.client.jar.borrow_mut() = crate::cookie::Jar::persistent(dir.clone());
            self.bookmarks = std::fs::read_to_string(dir.join("bookmarks.tsv")).unwrap_or_default().lines()
                .filter_map(|l| l.split_once('\t')).map(|(u, t)| (u.to_string(), t.to_string())).collect();
            self.seen = std::fs::read_to_string(dir.join("history.tsv")).unwrap_or_default().lines()
                .filter_map(|l| l.split_once('\t')).map(|(u, t)| (u.to_string(), t.to_string())).collect();
            self.orbit = crate::orbit::Orbit::load(Some(dir.clone()));
            self.data_dir = Some(dir);
        }
        self
    }

    fn save_bookmarks(&self) {
        let Some(d) = &self.data_dir else { return };
        let _ = std::fs::create_dir_all(d);
        let body: String = self.bookmarks.iter().map(|(u, t)| format!("{u}\t{}\n", t.replace(['\t', '\n'], " "))).collect();
        let tmp = d.join("bookmarks.tsv.tmp");
        if std::fs::write(&tmp, body).is_ok() { let _ = std::fs::rename(&tmp, d.join("bookmarks.tsv")); }
    }

    /// history.tsv: the pages seen, oldest first (≤ 2000, one row per address), written whole through a temp file (a crash
    /// never leaves half a history).
    fn save_history(&self) {
        let Some(d) = &self.data_dir else { return };
        let _ = std::fs::create_dir_all(d);
        let body: String = self.seen.iter().map(|(u, t)| format!("{u}\t{}\n", t.replace(['\t', '\n'], " "))).collect();
        let tmp = d.join("history.tsv.tmp");
        if std::fs::write(&tmp, body).is_ok() { let _ = std::fs::rename(&tmp, d.join("history.tsv")); }
    }

    /// Take the shown tab's state out (transient page state — find, field, focus, reader text — is not kept).
    fn park(&mut self) -> Parked {
        self.shot = None; self.find_boxes.clear(); self.find_q.clear(); self.find_edit = None; self.field = None; self.pfocus = None;
        self.a11y_rows = None; self.md.clear(); self.part = 0; self.rscroll = 0; self.form_vals.clear(); self.reader = false; self.field_row = None;
        Parked { page: self.page.take(), url: std::mem::take(&mut self.url), addr: std::mem::take(&mut self.addr), title_s: std::mem::take(&mut self.title_s),
                 back: std::mem::take(&mut self.back), fwd: std::mem::take(&mut self.fwd), y: self.y, page_h: self.page_h, http: self.http, zoom: self.zoom, load_ms: self.load_ms }
    }
    fn unpark(&mut self, p: Parked) {
        self.url = p.url; self.addr = p.addr; self.title_s = p.title_s; self.back = p.back; self.fwd = p.fwd; self.y = p.y; self.page_h = p.page_h;
        self.http = p.http; self.zoom = p.zoom; self.load_ms = p.load_ms;
        self.page = p.page;
        // an asleep tab wakes: its page loads again at the place it was left
        if self.page.is_none() && !self.url.is_empty() { let (u, y) = (self.url.clone(), self.y); self.navigate(&u, y); }
        else if self.page.is_some() { self.status = self.page_status(); }
        else { self.status = "New tab: type an address or search words, then press Enter.".into(); self.focus_ord = 0; }
    }
    /// Show tab `k`: park the current one; at most 3 tabs keep a live page process (RAM), the rest sleep.
    fn switch_tab(&mut self, k: usize) {
        if k >= self.tabs.len() || k == self.cur_tab { return; }
        let cur = self.park();
        self.tabs[self.cur_tab] = Some(cur);
        let live: Vec<usize> = (0..self.tabs.len()).filter(|&i| self.tabs[i].as_ref().map(|t| t.page.is_some()).unwrap_or(false) && i != k).collect();
        if live.len() > 2 { for &i in live.iter().take(live.len() - 2) { if let Some(t) = self.tabs[i].as_mut() { t.page = None; } } }
        let p = self.tabs[k].take().expect("parked tab");
        self.cur_tab = k;
        self.unpark(p);
    }
    fn tab_cmd(&mut self, arg: &str) {
        let a = arg.trim();
        let (w, rest) = a.split_once(' ').unwrap_or((a, ""));
        match w {
            "new" | "" => {
                let cur = self.park();
                self.tabs[self.cur_tab] = Some(cur);
                self.tabs.insert(self.cur_tab + 1, Some(Parked { page: None, url: String::new(), addr: String::new(), title_s: String::new(), back: vec![], fwd: vec![], y: 0, page_h: 0, http: 0, zoom: 100, load_ms: 0.0 }));
                let k = self.cur_tab + 1;
                let p = self.tabs[k].take().unwrap();
                self.cur_tab = k;
                self.unpark(p);
                if !rest.trim().is_empty() { self.open(rest.trim()); }
            }
            "close" => {
                if let Some(p) = self.page.as_ref() { self.b.drop_page(p.id); }
                if self.tabs.len() == 1 { let _ = self.park(); self.status = "New tab: type an address or search words, then press Enter.".into(); self.page_h = 0; return; }
                let _ = self.park();
                self.tabs.remove(self.cur_tab);
                let k = self.cur_tab.min(self.tabs.len() - 1);
                let p = self.tabs[k].take().expect("parked tab");
                self.cur_tab = k;
                self.unpark(p);
            }
            "next" => { let k = (self.cur_tab + 1) % self.tabs.len(); self.switch_tab(k); }
            "prev" => { let k = (self.cur_tab + self.tabs.len() - 1) % self.tabs.len(); self.switch_tab(k); }
            n => match n.parse::<usize>() { Ok(k) if k >= 1 => self.switch_tab((k - 1).min(self.tabs.len() - 1)), _ => self.status = format!("Tabs: new, close, next, prev or 1–{}.", self.tabs.len()) },
        }
    }
    fn tab_title(&self, k: usize) -> String {
        let (t, u) = if k == self.cur_tab { (self.title_s.as_str(), self.url.as_str()) } else { self.tabs[k].as_ref().map(|p| (p.title_s.as_str(), p.url.as_str())).unwrap_or(("", "")) };
        let s = if !t.is_empty() { t } else if !u.is_empty() { u } else { "New tab" };
        let s: String = s.chars().take(28).collect();
        format!("Tab {}: {s}", k + 1)
    }

    /// Bookmark the shown page, or remove it when it is already one (Ctrl+D).
    fn bookmark(&mut self) {
        if self.url.is_empty() || self.url.starts_with("about:") { self.status = "Nothing to bookmark.".into(); return; }
        if let Some(i) = self.bookmarks.iter().position(|(u, _)| *u == self.url) { self.bookmarks.remove(i); self.status = "Bookmark removed.".into(); }
        else { self.bookmarks.push((self.url.clone(), if self.title_s.is_empty() { self.url.clone() } else { self.title_s.clone() })); self.status = format!("Bookmarked: {}", self.title_s); }
        self.save_bookmarks();
    }

    /// about:bookmarks — the bookmarks and the pages seen this session, as links (built here: no network).
    fn about_page(&self, url: &str) -> Vec<u8> {
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
        let list = |v: &[(String, String)]| -> String { if v.is_empty() { "<p>None yet.</p>".into() } else { format!("<ol>{}</ol>", v.iter().map(|(u, t)| format!("<li><a href=\"{}\">{}</a> <small>{}</small></li>", esc(u), esc(t), esc(u))).collect::<String>()) } };
        let seen: Vec<(String, String)> = self.seen.iter().rev().cloned().collect();
        format!("<!doctype html><html lang=\"en\"><head><title>Bookmarks and history</title></head><body><main><h1>Bookmarks</h1>{}<h2>History</h2>{}\
                 <p><small>{}: Ctrl+D bookmarks a page (saved in your data folder); your history is saved there too.</small></p></main></body></html>",
                list(&self.bookmarks), list(&seen), esc(url)).into_bytes()
    }

    /// The page shown when a load fails (P4: designed error states). The network layer's message decides the class; the colours
    /// follow Orbit's theme so it reads like the rest of the browser. Built here: no network, no scripts.
    fn error_page(&self, url: &str, err: &str) -> Vec<u8> {
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let e = err.to_lowercase();
        let (title, what, tries): (&str, &str, &[&str]) =
            if e.contains("not an http") { ("That is not a web address", "Omni Web opens http and https addresses, and pages of its own (about:).", &["Check the address for typos.", "Type words instead to search."]) }
            else if e.contains("no such host") { ("This site can't be found", "The name did not resolve to an address, so there is nowhere to connect.", &["Check the spelling of the address.", "Check your network connection.", "Try again in a moment."]) }
            else if e.contains("did not answer") { ("This site isn't answering", "The address was found, but the connection was refused or timed out.", &["The site may be down or busy: try again later.", "Check your network connection or firewall."]) }
            else if e.contains("certificate") || e.contains("tls") || e.contains("handshake") || e.contains("ssl") { ("This connection isn't secure", "The site's identity could not be verified, so nothing was loaded.", &["Check that the address starts with the right host.", "If it is your own site, check its certificate."]) }
            else if e.contains("no valid http") { ("The site sent something unexpected", "The server answered, but not with a valid web response (or too slowly).", &["Try again in a moment.", "The address may point at a service that is not a website."]) }
            else { ("This page didn't load", "Something went wrong while loading it.", &["Try again in a moment.", "Check the address and your network connection."]) };
        let (bg, ink, ink2, acc, line) = match self.orbit.theme {
            1 => ("#eef2fa", "#0d1324", "#3c4760", "#2563eb", "rgba(13,19,36,.14)"),
            2 => ("#000000", "#ffffff", "#ffffff", "#ffe600", "#ffffff"),
            _ => ("#05070d", "#e9eefb", "#c4ccdf", "#7cc7ff", "rgba(255,255,255,.14)"),
        };
        let list: String = tries.iter().map(|t| format!("<li>{t}</li>")).collect();
        format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title><style>\
            html,body{{margin:0;background:{bg};color:{ink};font-family:system-ui,sans-serif}}\
            main{{max-width:560px;margin:0 auto;padding:150px 28px 40px 28px}}\
            h1{{font-size:30px;line-height:38px;margin:0 0 12px 0;font-weight:600}}\
            p{{font-size:17px;line-height:26px;color:{ink2};margin:0 0 18px 0}}\
            ul{{margin:0 0 26px 0;padding:0 0 0 22px;color:{ink2};font-size:16px;line-height:26px}}\
            .addr{{display:block;padding:10px 14px;border:1px solid {line};border-radius:12px;font-size:15px;color:{ink};margin:0 0 26px 0;word-break:break-all}}\
            a.go{{display:inline-block;padding:9px 20px;border-radius:20px;background:{acc};color:{bg};font-weight:600;text-decoration:none;font-size:16px}}\
            small{{display:block;margin-top:34px;color:{ink2};font-size:13px;line-height:20px}}\
            </style></head><body><main><h1>{title}</h1><p>{what}</p><span class=\"addr\">{addr}</span><ul>{list}</ul>\
            <a class=\"go\" href=\"{href}\">Try again</a><small>{detail}</small></main></body></html>",
            addr = esc(url), href = esc(url), detail = esc(err)).into_bytes()
    }

    /// Copy the focused link's address, else the page's, to the clipboard (xclip / wl-copy).
    fn copy(&mut self) {
        let link = self.pfocus.and_then(|k| self.a11y_rows.as_ref().and_then(|r| r.get(k)).and_then(|r| r[7]["href"].as_str().map(|s| s.to_string())));
        let text = link.unwrap_or_else(|| self.url.clone());
        if text.is_empty() { self.status = "Nothing to copy.".into(); return; }
        use std::io::Write;
        let tried = [("wl-copy", vec![]), ("xclip", vec!["-selection", "clipboard"]), ("xsel", vec!["--clipboard", "--input"])];
        for (cmd, args) in tried {
            if let Ok(mut c) = std::process::Command::new(cmd).args(&args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
                if let Some(mut i) = c.stdin.take() { let _ = i.write_all(text.as_bytes()); }
                if c.wait().map(|st| st.success()).unwrap_or(false) { self.status = format!("Copied {text}"); return; }
            }
        }
        self.status = "Could not copy: no working clipboard tool (wl-copy / xclip / xsel) for this display.".into();
    }

    fn site(&self) -> String { crate::cookie::site_of(&crate::net::host_of(&self.url)) }

    /// Address bar text → URL: a scheme or a dotted host is an address; anything else is a search (DuckDuckGo's HTML page:
    /// results without JavaScript).
    pub fn to_url(s: &str) -> String {
        let s = s.trim();
        if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("about:") { return s.to_string(); }
        if !s.contains(' ') && s.contains('.') && !s.ends_with('.') { return format!("https://{s}"); }
        let q: String = s.bytes().map(|b| match b { b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => (b as char).to_string(), b' ' => "+".into(), _ => format!("%{b:02X}") }).collect();
        format!("https://html.duckduckgo.com/html/?q={q}")
    }

    fn renderer_cmd(js: bool) -> std::process::Command {
        let mut c = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "omni-ui".into()));
        c.arg("web-renderer").env("OMR_JS", if js { "1" } else { "0" });
        c
    }

    /// Show `url` at scroll `y` (no history change). A page that asks to refresh to another URL within 5 s (HTML refresh
    /// pragma: redirect pages such as a search engine's result hop) is replaced by that URL, up to 5 hops.
    fn navigate(&mut self, url: &str, y: i32) {
        let mut u = url.to_string();
        for _ in 0..5 {
            self.navigate_once(&u, y);
            let next = self.page.as_ref().and_then(|p| {
                if let Some(n) = p.meta["navigate"].as_str() { return Some(n.to_string()); } // a script set location during load
                let r = &p.meta["refresh"]; if r["delay"].as_f64()? <= 5.0 { r["url"].as_str().map(|s| s.to_string()) } else { None } });
            match next { Some(n) if n != self.url && (n.starts_with("http://") || n.starts_with("https://")) => u = n, _ => break }
        }
    }

    fn navigate_once(&mut self, url: &str, y: i32) {
        let t0 = Instant::now();
        if let Some(p) = self.page.as_ref() { self.b.drop_page(p.id); } // its WebSockets close with it
        self.page = None; // the old page process exits first: one page resident at a time
        self.shot = None;
        self.find_boxes.clear();
        self.field = None;
        self.md.clear();
        self.part = 0;
        self.rscroll = 0;
        self.form_vals.clear();
        self.a11y_rows = None; self.pfocus = None;
        let (method, body) = match self.post.take() { Some(b) => ("POST", Some(b)), None => ("GET", None) };
        let about = url.starts_with("about:").then(|| pageproc::Nav { url: url.to_string(), status: 200, doc: self.about_page(url), mime: "text/html".into() });
        // a load that fails is a designed page (error_page: what happened, what to try), not a blank window; it is not history
        let mut failed = false;
        let nav = match about.or_else(|| pageproc::navigate(&self.b, method, url, body)) {
            Some(n) => n,
            None => {
                failed = true;
                let err = self.b.client.last_error.borrow().clone();
                self.status = format!("Could not load {url}: {err}");
                pageproc::Nav { url: url.to_string(), status: 0, doc: self.error_page(url, &err), mime: "text/html".into() }
            }
        };
        self.http = nav.status;
        let (final_url, doc) = (nav.url, nav.doc);
        match PageProc::open(Self::renderer_cmd(self.js), &self.b, &final_url, doc, self.view.0, self.view.1) {
            Ok(mut p) => {
                if self.zoom != 100 { p.zoom(&self.b, self.zoom); }
                // the page's scripts run on the first tick, off the window's thread (scripts_pending): the page shows at once
                self.pending = if p.meta["scripts_pending"] == true { 1 } else { p.meta["js"]["pending"].as_u64().unwrap_or(0) };
                self.page_h = p.meta["page_h"].as_i64().unwrap_or(0) as i32;
                self.title_s = p.meta["title"].as_str().unwrap_or("").to_string();
                self.url = final_url.clone();
                self.addr = final_url;
                self.y = y.clamp(0, (self.page_h - self.view.1 as i32).max(0));
                if p.report.contains("FAIL") { self.status = format!("Renderer sandbox incomplete: {}", p.report.replace('\n', "; ")); }
                self.load_ms = t0.elapsed().as_secs_f64() * 1e3;
                if !failed && !self.url.starts_with("about:") && self.seen.last().map(|x| x.0 != self.url).unwrap_or(true) {
                    // one row per address: a revisit moves it to the end (newest) with its current title
                    self.seen.retain(|x| x.0 != self.url);
                    self.seen.push((self.url.clone(), if self.title_s.is_empty() { self.url.clone() } else { self.title_s.clone() }));
                    if self.seen.len() > 2000 { self.seen.remove(0); }
                    self.save_history();
                }
                self.page = Some(p);
                if self.reader { self.load_reader(); }
                if !self.status.starts_with("Renderer sandbox") { self.status = self.page_status(); }
            }
            Err(e) => { self.status = format!("Renderer failed: {e}"); self.url = url.to_string(); }
        }
    }

    fn page_status(&self) -> String {
        let pct = if self.page_h > self.view.1 as i32 { (self.y as i64 * 100 / (self.page_h - self.view.1 as i32).max(1) as i64) as i32 } else { 100 };
        let links = self.page.as_ref().and_then(|p| p.meta["links"].as_u64()).unwrap_or(0);
        let mut s = String::new();
        if self.http >= 400 { s += &format!("HTTP {} · ", self.http); }
        s += &format!("{} · {} links · {pct}% · {:.0} ms", if self.title_s.is_empty() { &self.url } else { &self.title_s }, links, self.load_ms);
        if self.b.download.borrow().is_some() { s += " · a file: press s to save it"; }
        let site = self.site();
        if self.b.client.jar.borrow().keep.contains(&site) { s += " · cookies kept"; }
        if self.zoom != 100 { s += &format!(" · zoom {}%", self.zoom); }
        if !self.js { s += " · JS off"; } else if let Some(j) = self.page.as_ref().map(|p| &p.meta["js"]).filter(|j| j["scripts_ran"].as_u64().unwrap_or(0) + j["scripts_failed"].as_u64().unwrap_or(0) > 0) {
            s += &format!(" · JS {}/{}", j["scripts_ran"], j["scripts_ran"].as_u64().unwrap_or(0) + j["scripts_failed"].as_u64().unwrap_or(0) + j["parse_errors"].as_u64().unwrap_or(0));
        }
        if !self.find_q.is_empty() { s += &format!(" · “{}”: {}", self.find_q, if self.find_boxes.is_empty() { "no match".to_string() } else { format!("{} of {}", self.find_ix + 1, self.find_boxes.len()) }); }
        s
    }

    fn load_reader(&mut self) {
        if let Some(p) = self.page.as_mut() { self.md = p.markdown(&self.b); }
    }

    fn open(&mut self, arg: &str) {
        let url = Self::to_url(arg);
        if url.is_empty() { return; }
        if !self.url.is_empty() { self.back.push((self.url.clone(), self.y)); }
        self.fwd.clear();
        self.navigate(&url, 0);
    }

    fn scroll_to(&mut self, y: i32) {
        if self.reader { self.rscroll = y.clamp(0, (self.rext.0 - self.rext.1).max(0)); return; }
        self.y = y.clamp(0, (self.page_h - self.view.1 as i32).max(0));
        self.status = self.page_status();
    }

    fn cur_scroll(&self) -> i32 { if self.reader { self.rscroll } else { self.y } }
    fn view_h(&self) -> i32 { if self.reader { self.rext.1.max(1) } else { self.view.1 as i32 } }

    fn find(&mut self, q: &str) {
        let q = q.trim();
        if q.is_empty() || q == self.find_q {
            if !self.find_boxes.is_empty() { self.find_ix = (self.find_ix + 1) % self.find_boxes.len(); }
        } else {
            self.find_q = q.to_string();
            self.find_ix = 0;
            self.find_boxes = self.page.as_mut().map(|p| p.find(&self.b, q)).and_then(|v| v["boxes"].as_array().cloned()).unwrap_or_default()
                .iter().filter_map(|b| { let a = b.as_array()?; Some([a[0].as_i64()? as i32, a[1].as_i64()? as i32, a[2].as_i64()? as i32, a[3].as_i64()? as i32]) }).collect();
            // start from the first match at or below the current view
            if let Some(i) = self.find_boxes.iter().position(|b| b[1] >= self.y) { self.find_ix = i; }
        }
        if let Some(b) = self.find_boxes.get(self.find_ix).copied() { self.scroll_to(b[1] - self.view.1 as i32 / 3); }
        self.status = self.page_status();
    }

    fn follow(&mut self, arg: &str) {
        let Some(p) = self.page.as_mut() else { return };
        let links = p.links(&self.b);
        let pick = match arg.trim().parse::<usize>() {
            Ok(i) => links.get(i).cloned(),
            Err(_) => { let a = arg.trim().to_lowercase(); links.iter().find(|l| l.1.to_lowercase() == a).or_else(|| links.iter().find(|l| l.1.to_lowercase().contains(&a))).cloned() }
        };
        match pick {
            Some((href, _, _)) => self.open_link(&href),
            None => self.status = format!("No link “{}” on this page ({} links).", arg.trim(), links.len()),
        }
    }

    /// A link target: same document + fragment = scroll there; otherwise navigate.
    fn open_link(&mut self, href: &str) {
        let (doc, frag) = href.split_once('#').unwrap_or((href, ""));
        let cur = self.url.split('#').next().unwrap_or("").to_string();
        if doc == cur && !frag.is_empty() {
            // in-page anchor: the renderer finds the element with that id (or <a name>) and its box
            let target = self.page.as_mut().map(|p| p.anchor(&self.b, frag)).unwrap_or(-1);
            self.back.push((self.url.clone(), self.y));
            self.fwd.clear();
            if target >= 0 { self.scroll_to(target); self.url = format!("{cur}#{frag}"); self.addr = self.url.clone(); }
            return;
        }
        if !(href.starts_with("http://") || href.starts_with("https://") || href.starts_with("about:")) { self.status = format!("Not opened (not a web link): {href}"); return; }
        self.open(href);
    }

    fn submit(&mut self) {
        let Some(f) = self.field.as_ref() else { return };
        let Some(form) = f.form else { self.status = "This field is not in a form.".into(); return };
        self.form_vals.insert(f.name.clone(), f.value.clone());
        self.send_form(form);
    }

    /// Submit form `form` with everything typed into this page (GET → a URL; POST → the body rides on the next navigation).
    fn send_form(&mut self, form: u64) {
        let vals = self.form_vals.clone();
        let r = self.page.as_mut().map(|p| p.submit(&self.b, form, &vals)).unwrap_or(json!({}));
        // the page's own scripts first (HTML §4.10.21.3 step 6: fire submit, cancelable): they see what was typed
        if self.js {
            let e = self.page.as_mut().map(|p| p.form_event(&self.b, form, &vals, true)).unwrap_or(json!({}));
            if let Some(u) = e["navigate"].as_str() { let u = u.to_string(); self.field = None; self.open(&u); return; }
            if e["prevented"] == true {
                if let Some(p) = self.page.as_ref() { self.page_h = p.meta["page_h"].as_i64().unwrap_or(self.page_h as i64) as i32; if let Some(t) = p.meta["title"].as_str() { self.title_s = t.to_string(); } }
                self.shot = None; self.a11y_rows = None; self.find_boxes.clear(); self.field = None;
                self.status = "The page handled the form.".into();
                return;
            }
        }
        let Some(u) = r["url"].as_str().map(|s| s.to_string()) else { self.status = format!("Form not sent: {}", r["error"].as_str().unwrap_or("?")); return };
        if r["method"] == "POST" {
            self.post = Some((r["ctype"].as_str().unwrap_or("application/x-www-form-urlencoded").to_string(), r["body"].as_str().unwrap_or("").as_bytes().to_vec()));
        }
        self.field = None;
        self.open(&u);
    }

    /// The page's accessibility rows (pageproc `W`), cached until the page re-lays out.
    fn rows(&mut self) -> Option<std::rc::Rc<Vec<Value>>> {
        if self.page.as_ref().map(|p| p.busy).unwrap_or(false) { return self.a11y_rows.clone(); } // never wait on a busy page
        if self.a11y_rows.is_none() { let r = self.page.as_mut()?.a11y(&self.b); self.a11y_rows = Some(std::rc::Rc::new(r)); }
        self.a11y_rows.clone()
    }
    fn row_role(&self, k: usize) -> String { self.a11y_rows.as_ref().and_then(|r| r.get(k)).and_then(|r| r[0].as_str()).unwrap_or("").to_string() }
    /// Rows the keyboard can reach (links, buttons, form controls, tabindex ≥ 0) with a box on the page, in document order.
    fn page_focusables(&mut self) -> Vec<usize> {
        let Some(rows) = self.rows() else { return vec![] };
        rows.iter().enumerate().filter(|(_, r)| r[7]["focusable"] == true && r[5].as_i64().unwrap_or(0) > 0 && r[6].as_i64().unwrap_or(0) > 0 && r[7]["disabled"] != true).map(|(k, _)| k).collect()
    }
    /// HTML §6.6.3 sequential focus navigation: the chrome's focus ring, then the page's focusable rows, around.
    fn tab(&mut self, t: &Tree, back: bool) {
        let c = t.focusables().len();
        let fs = if self.reader { vec![] } else { self.page_focusables() };
        let total = (c + fs.len()).max(1) as i32;
        let w = self.focus_widget(t);
        let in_field = self.field.as_ref().map(|f| t.label(w) == f.label).unwrap_or(false);
        // leaving a page field's editor keeps what was typed for the form's submit
        if in_field { if let Some(f) = &self.field { self.form_vals.insert(f.name.clone(), f.value.clone()); } }
        let cur = match (self.pfocus, in_field, self.field_row) {
            (Some(k), _, _) => c + fs.iter().position(|&r| r == k).unwrap_or(0),
            (None, true, Some(k)) => c + fs.iter().position(|&r| r == k).unwrap_or(0),
            _ => self.focus_ord.min(c.saturating_sub(1)),
        } as i32;
        let nx = (cur + if back { -1 } else { 1 }).rem_euclid(total) as usize;
        if nx < c { self.pfocus = None; self.focus_ord = nx; self.status = self.page_status(); } else { self.focus_row(fs[nx - c]); }
    }
    /// Give a page row keyboard focus: scroll it into view, say what it is on the status line (screen readers get focus).
    fn focus_row(&mut self, k: usize) {
        let Some(rows) = self.rows() else { return };
        let Some(r) = rows.get(k) else { return };
        self.pfocus = Some(k);
        let (y, h) = (r[4].as_i64().unwrap_or(0) as i32, r[6].as_i64().unwrap_or(0) as i32);
        let vh = self.view.1 as i32;
        if y < self.y || y + h > self.y + vh { self.scroll_to(y - vh / 3); }
        let role = r[0].as_str().unwrap_or("");
        let what = match role { "link" => "Link", "button" => "Button", "textbox" | "searchbox" => "Text field", "checkbox" => "Checkbox", "radio" => "Radio button",
            "combobox" => "Choice", "slider" => "Slider", _ => "Control" };
        let state = match r[7]["checked"].as_bool() { Some(true) => " (checked)", Some(false) => " (not checked)", None => "" };
        self.status = format!("{what}: {}{state} — Enter to {}", r[1].as_str().unwrap_or("").chars().take(120).collect::<String>(), if role == "link" { "open" } else { "use" });
    }
    /// Enter / an AT click on a page row: links open (in-page anchors scroll); controls get the click a pointer would give.
    fn activate_row(&mut self, k: usize) -> Value {
        let Some(rows) = self.rows() else { return json!({}) };
        let Some(r) = rows.get(k).cloned() else { return json!({}) };
        if r[0] == "link" { if let Some(h) = r[7]["href"].as_str() { let h = h.to_string(); self.pfocus = None; self.open_link(&h); return self.log.record("follow", &h, "key", json!({"url": self.url})); } }
        let (x, y, w, h) = (r[3].as_i64().unwrap_or(0) as i32, r[4].as_i64().unwrap_or(0) as i32, r[5].as_i64().unwrap_or(0) as i32, r[6].as_i64().unwrap_or(0) as i32);
        let (cx, cy) = (x + w / 2, y + h / 2);
        if cy < self.y || cy >= self.y + self.view.1 as i32 { self.scroll_to(cy - self.view.1 as i32 / 2); }
        let keep = self.pfocus;
        let v = self.act("click", &format!("{cx} {}", cy - self.y), "key");
        self.field_row = if self.field.is_some() { Some(k) } else { None };
        // a checkbox / radio / choice keeps focus so the person hears the new state; a field moves focus to its editor
        if self.field.is_none() && self.page.is_some() && matches!(r[0].as_str(), Some("checkbox" | "radio" | "combobox" | "button")) && self.a11y_rows.is_some() { self.pfocus = keep; }
        v
    }

    /// `save`: the file the last navigation brought (not a page) goes to the Downloads folder; the name never overwrites.
    fn save(&mut self) {
        // a page saves as its readable text (Markdown, the reader view's), named after its title
        let page_md = if self.b.download.borrow().is_none() && self.page.is_some() {
            let md = self.page.as_mut().map(|p| p.markdown(&self.b)).unwrap_or_default();
            let stem: String = self.title_s.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect::<String>().trim_matches('_').chars().take(60).collect();
            Some((format!("https://local/{}.md", if stem.is_empty() { "page".to_string() } else { stem }), "text/markdown".to_string(), format!("<!-- {} -->\n{md}", self.url).into_bytes()))
        } else { None };
        let Some((url, mime, bytes)) = page_md.or_else(|| self.b.download.borrow().clone()) else { self.status = "Nothing to save.".into(); return };
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let dir = std::env::var_os("XDG_DOWNLOAD_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| home.join("Downloads"));
        let name: String = url.split(['?', '#']).next().unwrap_or("").rsplit('/').find(|s| !s.is_empty()).unwrap_or("download")
            .chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
        let name = if name.starts_with('.') || name.is_empty() { format!("download{name}") } else { name };
        let _ = std::fs::create_dir_all(&dir);
        let (stem, ext) = match name.rsplit_once('.') { Some((a, b)) => (a.to_string(), format!(".{b}")), None => (name.clone(), String::new()) };
        let mut path = dir.join(&name);
        let mut k = 1;
        while path.exists() { path = dir.join(format!("{stem} ({k}){ext}")); k += 1; }
        self.status = match std::fs::write(&path, &bytes) { Ok(_) => format!("Saved {} ({mime}, {} bytes).", path.display(), bytes.len()), Err(e) => format!("Not saved: {e}") };
    }

    fn click(&mut self, x: i32, y: i32) {
        let py = y + self.y;
        let Some(p) = self.page.as_mut() else { return };
        // the page's own click handlers first (JS-0); its default action only when no handler prevented it
        let h = if self.js { p.click(&self.b, x, py) } else { p.hit(&self.b, x, py) };
        if h["relayout"] == true { self.page_h = p.meta["page_h"].as_i64().unwrap_or(self.page_h as i64) as i32; if let Some(t) = p.meta["title"].as_str() { self.title_s = t.to_string(); } self.shot = None; self.find_boxes.clear(); self.a11y_rows = None; self.pfocus = None; }
        if let Some(u) = h["open"].as_array().and_then(|v| v.first()).and_then(|v| v.as_str()) { let u = u.to_string(); self.tab_cmd(&format!("new {u}")); return; }
        if let Some(u) = h["navigate"].as_str() { let u = u.to_string(); self.open(&u); return; }
        if h["prevented"] == true { self.status = self.page_status(); return; }
        let Some(p) = self.page.as_mut() else { return };
        if let Some(l) = h["link"].as_str() {
            let l = l.to_string();
            // HTML §7.3.1.7: target=_blank opens a new browsing context — a new tab
            if h["target"] == "_blank" && (l.starts_with("http://") || l.starts_with("https://")) { self.tab_cmd(&format!("new {l}")); } else { self.open_link(&l); }
            return;
        }
        if h.get("input").is_some() {
            let ty = h["type"].as_str().unwrap_or("text");
            let form = h["form"].as_u64();
            if matches!(ty, "submit" | "image") || h["tag"] == "button" {
                if self.field.as_ref().map(|f| f.form == form).unwrap_or(false) { self.submit(); }
                else if let Some(fm) = form { let _ = p; self.send_form(fm); }
                return;
            }
            if matches!(ty, "checkbox") {
                let name = h["name"].as_str().unwrap_or("").to_string();
                let on = self.form_vals.get(&name).map(|v| v == "on").unwrap_or(h["checked"] == true);
                self.form_vals.insert(name.clone(), if on { "off".into() } else { "on".into() });
                self.status = format!("{} {}", if on { "Unchecked" } else { "Checked" }, if name.is_empty() { "box" } else { &name });
                return;
            }
            if ty == "select" {
                let name = h["name"].as_str().unwrap_or("").to_string();
                let opts: Vec<(String, String, bool)> = h["options"].as_array().cloned().unwrap_or_default().iter()
                    .map(|o| (o[0].as_str().unwrap_or("").to_string(), o[1].as_str().unwrap_or("").to_string(), o[2] == true)).collect();
                if opts.is_empty() { return; }
                let cur = self.form_vals.get(&name).and_then(|v| opts.iter().position(|o| &o.0 == v)).or_else(|| opts.iter().position(|o| o.2)).unwrap_or(0);
                let next = (cur + 1) % opts.len();
                self.form_vals.insert(name.clone(), opts[next].0.clone());
                self.status = format!("{name}: {} ({} of {}; click again for the next)", opts[next].1, next + 1, opts.len());
                return;
            }
            if matches!(ty, "radio") {
                let name = h["name"].as_str().unwrap_or("").to_string();
                self.form_vals.insert(name.clone(), h["value"].as_str().unwrap_or("on").to_string());
                self.status = format!("Chose {} for {name}", h["value"].as_str().unwrap_or("on"));
                return;
            }
            if matches!(ty, "text" | "search" | "email" | "url" | "tel" | "number" | "password") || h["tag"] == "textarea" { self.edit_field(&h); }
        }
    }

    /// Start editing a page control (a hit or an 'E' field record): it is typed in the chrome, sent with Enter.
    fn edit_field(&mut self, h: &Value) {
        let label = [h["placeholder"].as_str(), h["name"].as_str()].into_iter().flatten().find(|s| !s.is_empty()).unwrap_or("Field").to_string();
        // a field left with a value keeps it for this form's submit
        if let Some(f) = self.field.take() { self.form_vals.insert(f.name, f.value); }
        let name = h["name"].as_str().unwrap_or("").to_string();
        let value = self.form_vals.get(&name).cloned().unwrap_or_else(|| h["value"].as_str().unwrap_or("").to_string());
        self.field = Some(Field { form: h["form"].as_u64(), name, label, value, secret: h["type"] == "password" });
        self.focus_ord = if self.find_edit.is_some() { 2 } else { 1 }; // the field sits right after the address bar (and find) in the focus ring
        self.pfocus = None;
        self.status = "Type, then press Enter to send.".into();
    }

    /// `field <index | name>`: the page's text-entry controls, reachable without a pointer.
    fn field_cmd(&mut self, arg: &str) {
        let fs = self.page.as_mut().map(|p| p.fields(&self.b)).unwrap_or_default();
        let a = arg.trim();
        let pick = match a.parse::<usize>() { Ok(i) => fs.get(i).cloned(), Err(_) => fs.iter().find(|f| f["name"] == a || f["placeholder"].as_str().map(|p| p.eq_ignore_ascii_case(a)).unwrap_or(false)).cloned() };
        match pick {
            Some(f) => { if let Some(y) = f["box"][1].as_i64() { if (y as i32) < self.y || y as i32 > self.y + self.view.1 as i32 { self.scroll_to(y as i32 - 80); } } self.edit_field(&f); }
            None => self.status = format!("No field “{a}” on this page ({} fields).", fs.len()),
        }
    }

    fn reader_tree(&self, t: &mut Tree, win: u32) {
        let all = blocks(&self.md);
        if all.is_empty() { t.add(Kind::Paragraph, win, "(no readable text on this page)", "", ""); return; }
        let starts = part_starts(&all);
        let part = self.part.min(starts.len() - 1);
        let (from, to) = (starts[part], starts.get(part + 1).copied().unwrap_or(all.len()));
        if starts.len() > 1 { t.add(Kind::Status, win, &format!("Long page: part {} of {} (Next part: scroll past the end)", part + 1, starts.len()), "", ""); }
        let mut lists: HashMap<u32, u32> = HashMap::new();
        for b in &all[from..to] {
            match b {
                Block::Heading(l, s) => { t.add(Kind::Heading, win, &strip_links(s), &l.to_string(), ""); }
                Block::Para(s) => { let p = t.add(Kind::Paragraph, win, "", "", ""); inline_md(t, p, s); }
                Block::Code(s) => { t.add(Kind::Code, win, s, "", ""); }
                Block::Item(id, s) => {
                    let l = *lists.entry(*id).or_insert_with(|| t.add(Kind::List, win, "", "", ""));
                    let li = t.add(Kind::ListItem, l, "", "", "");
                    inline_md(t, li, s);
                }
            }
        }
    }
}

/// `[text](url)` → text.
fn strip_links(s: &str) -> String { let mut t = Tree::default(); inline_md(&mut t, 0, s); (0..t.len() as u32).map(|i| t.label(i).to_string()).collect::<Vec<_>>().join("") }

/// md::plain, keeping one space at either edge (the run sits between links: "has [three] as its").
fn keep_edges(s: &str) -> String {
    let p = crate::md::plain(s);
    let l = if s.starts_with(char::is_whitespace) { " " } else { "" };
    let r = if s.ends_with(char::is_whitespace) && !p.is_empty() { " " } else { "" };
    format!("{l}{p}{r}")
}

/// One markdown line: text runs and `[text](url)` links (agent::markdown writes links this way), in order.
fn inline_md(t: &mut Tree, parent: u32, s: &str) {
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        let after = &rest[i + 1..];
        let Some(j) = after.find("](") else { break };
        let Some(k) = after[j + 2..].find(')') else { break };
        if i > 0 { t.add(Kind::Span, parent, &keep_edges(&rest[..i]), "", ""); }
        let text = &after[..j];
        let url = &after[j + 2..j + 2 + k];
        if text.trim().is_empty() { } else { t.add(Kind::Link, parent, &crate::md::plain(text), url, "open"); }
        rest = &after[j + 3 + k..];
    }
    if !rest.trim().is_empty() { t.add(Kind::Span, parent, &keep_edges(rest), "", ""); }
}

impl App for WebApp {
    fn title(&self) -> String { if self.title_s.is_empty() { "Web — OmniUI".into() } else { format!("{} — Web", self.title_s) } }
    fn css(&self) -> String { format!("{CSS}.field.addr{{width:{}px}}", (self.view.0 as i32 - 330).max(160)) }
    fn focus_ord(&self) -> usize { self.focus_ord }
    fn set_focus_ord(&mut self, i: usize) { self.focus_ord = i; }
    fn tab_is_intent(&self) -> bool { false }
    fn fit(&self) -> Option<(i32, i32)> { if self.reader { Some((18, 40)) } else { None } }

    fn tree(&self) -> Tree {
        let mut t = Tree::default();
        let win = t.add(Kind::Window, NONE, &self.title(), "", "");
        t.add(Kind::TextInput, win, "Address or search", &self.addr, "open");
        if let Some(q) = &self.find_edit { t.add(Kind::TextInput, win, "Find in page", q, "open"); }
        if let Some(f) = &self.field { let shown = if f.secret { "•".repeat(f.value.chars().count()) } else { f.value.clone() }; t.add(Kind::TextInput, win, &f.label, &shown, "open"); }
        for l in ["Back", "Forward", "Reload", if self.reader { "Page" } else { "Reader" }] { t.add(Kind::Button, win, l, "", "select"); }
        if self.b.download.borrow().is_some() { t.add(Kind::Button, win, "Save", "", "select"); }
        let site = self.site();
        let jar = self.b.client.jar.borrow();
        if !site.is_empty() {
            // the cookie switch is ternary: block · this session · keep (the current one reads "on")
            let cur = if jar.block.contains(&site) { 0 } else if jar.keep.contains(&site) { 2 } else { 1 };
            for (k, l) in ["Cookies: blocked", "Cookies: this session", "Cookies: kept"].iter().enumerate() { t.add(Kind::Button, win, l, if k == cur { "on" } else { "" }, "select"); }
        }
        drop(jar);
        if self.tabs.len() > 1 { for k in 0..self.tabs.len() { let l = self.tab_title(k); t.add(Kind::Button, win, &if k == self.cur_tab { format!("{l} (shown)") } else { l }, "", "select"); } }
        t.add(Kind::Button, win, "New tab", "", "select");
        t.add(Kind::Status, win, &self.status, "", "");
        // Orbit (C1): the site trit, settings and the core; the open panel's choices; suggestions while the address is edited
        for l in ["Site", "Settings", "Omni core"] { t.add(Kind::Button, win, l, "", "select"); }
        use crate::orbit::Panel;
        if self.orbit.shown_panel() == Panel::Settings {
            let o = &self.orbit;
            let on = |b: bool| if b { "on" } else { "" };
            for (l, b) in [("Hide bar: off", !o.hide), ("Hide bar: on", o.hide), ("Theme: dark", o.theme == 0), ("Theme: light", o.theme == 1), ("Theme: contrast", o.theme == 2),
                ("Surfaces: glass", !o.solid), ("Surfaces: solid", o.solid), ("Motion: fluid", !o.still), ("Motion: still", o.still),
                ("Page scripts: off", !self.js), ("Page scripts: on", self.js)] { t.add(Kind::Button, win, l, on(b), "select"); }
        }
        if self.editing_addr() { for (title, url) in self.suggestions() { t.add(Kind::Link, win, &title, &url, "open"); } }
        if self.reader { self.reader_from.set(t.len() as u32); self.reader_tree(&mut t, win); }
        t
    }

    fn arg_for(&self, t: &Tree, w: u32, _verb: &str) -> String {
        match t.kind[w as usize] {
            Kind::Link => t.value(w).to_string(),
            Kind::Button => t.label(w).to_string(),
            Kind::TextInput if t.label(w) == "Find in page" => "\u{2}find".into(),
            Kind::TextInput if self.field.is_some() && t.label(w) != "Address or search" => "\u{1}field".into(),
            Kind::TextInput => self.addr.clone(),
            _ => String::new(),
        }
    }

    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value {
        // while the page's scripts run, page actions wait (navigation, tabs, the address and the chrome stay live)
        if self.page.as_ref().map(|p| p.busy).unwrap_or(false) && matches!(verb, "click" | "find" | "follow" | "field" | "submit" | "read" | "bigger" | "smaller" | "copy" | "js") {
            self.status = "The page is still running its scripts — one moment.".into();
            return self.log.record(verb, arg, source, json!({"busy": true}));
        }
        if matches!(verb, "select" | "open" | "back" | "forward" | "reload" | "tab" | "bookmark" | "bookmarks") && source != "tick" { self.orbit.touch(); }
        if matches!(verb, "open" | "reload" | "back" | "forward") { self.orbit.spin(900); }
        match verb {
            "open" if arg == "\u{1}field" => self.submit(),
            "open" if arg == "\u{2}find" => { let q = self.find_edit.clone().unwrap_or_default(); self.find(&q); }
            "open" => { let a = arg.trim().to_string(); if a.is_empty() { self.status = "Nothing to open.".into(); } else if a.contains("://") { self.open_link(&a); } else { self.open(&a); } }
            "back" => if let Some((u, y)) = self.back.pop() {
                self.fwd.push((self.url.clone(), self.y));
                let (cu, _) = self.url.split_once('#').unwrap_or((&self.url, ""));
                let (nu, _) = u.split_once('#').unwrap_or((&u, ""));
                if cu == nu && self.page.is_some() { self.url = u.clone(); self.addr = u; self.scroll_to(y); } else { self.navigate(&u, y); }
            } else { self.status = "No earlier page.".into(); },
            "forward" => if let Some((u, y)) = self.fwd.pop() {
                self.back.push((self.url.clone(), self.y));
                let (cu, _) = self.url.split_once('#').unwrap_or((&self.url, ""));
                let (nu, _) = u.split_once('#').unwrap_or((&u, ""));
                if cu == nu && self.page.is_some() { self.url = u.clone(); self.addr = u; self.scroll_to(y); } else { self.navigate(&u, y); }
            } else { self.status = "No later page.".into(); },
            "reload" => { let (u, y) = (self.url.clone(), self.y); if !u.is_empty() { self.navigate(&u, y); } }
            "scroll" => {
                let vh = self.view_h();
                let cur = self.cur_scroll();
                let ny = match arg.trim() { "page" | "+page" => cur + vh * 9 / 10, "-page" => cur - vh * 9 / 10, "top" => 0, "end" => i32::MAX / 2,
                                            a => cur + a.parse::<i32>().unwrap_or(0) };
                if self.reader && ny > (self.rext.0 - self.rext.1).max(0) && cur >= (self.rext.0 - self.rext.1).max(0) {
                    let parts = part_starts(&blocks(&self.md)).len();
                    if self.part + 1 < parts { self.part += 1; self.rscroll = 0; }
                } else { self.scroll_to(ny); }
            }
            "follow" => self.follow(arg),
            "find" => self.find(arg),
            "click" => { let mut it = arg.split_whitespace().map(|v| v.parse::<i32>().unwrap_or(0)); let (x, y) = (it.next().unwrap_or(0), it.next().unwrap_or(0)); self.click(x, y); }
            "type" => {
                let t = self.tree();
                let w = self.focus_widget(&t);
                match (&mut self.field, t.label(w)) { (Some(f), l) if l == f.label => f.value = arg.to_string(), _ => self.addr = arg.to_string() }
            }
            "submit" => self.submit(),
            "js" => { self.js = !self.js; let (u, y) = (self.url.clone(), self.y); if !u.is_empty() { self.navigate(&u, y); } self.status = self.page_status(); }
            "field" => self.field_cmd(arg),
            "read" => {
                self.reader = !self.reader;
                if self.reader && self.md.is_empty() { self.load_reader(); }
                self.status = if self.reader { format!("Reader view · {}", self.title_s) } else { self.page_status() };
            }
            "bigger" | "smaller" => {
                self.zoom = (self.zoom + if verb == "bigger" { 10 } else { -10 }).clamp(50, 300);
                let old_h = self.page_h.max(1) as i64;
                if let Some(p) = self.page.as_mut() { if let Some(m) = p.zoom(&self.b, self.zoom) { self.page_h = m["page_h"].as_i64().unwrap_or(0) as i32; } }
                self.shot = None; self.a11y_rows = None;
                self.find_boxes.clear(); self.find_q.clear();
                self.scroll_to((self.y as i64 * self.page_h as i64 / old_h) as i32);
            }
            "select" => match arg.trim().to_lowercase().as_str() {
                // Orbit (C1)
                "omni core" => { if self.orbit.folded { self.orbit.unfold(); } else { self.orbit.panel = crate::orbit::Panel::None; self.pfocus = None; self.focus_ord = 0; self.addr_sel = true; } }
                "site" => { use crate::orbit::Panel; self.orbit.panel = if self.orbit.panel == Panel::Site { Panel::None } else { Panel::Site }; }
                "settings" => { use crate::orbit::Panel; self.orbit.panel = if self.orbit.panel == Panel::Settings { Panel::None } else { Panel::Settings }; }
                "hide bar: on" => self.orbit.set_hide(true),
                "hide bar: off" => self.orbit.set_hide(false),
                a if a.starts_with("theme: ") => { self.orbit.theme = match &a[7..] { "light" => 1, "contrast" => 2, _ => 0 }; self.orbit.save(); }
                a if a.starts_with("surfaces: ") => { self.orbit.solid = a.ends_with("solid"); self.orbit.save(); }
                a if a.starts_with("motion: ") => { self.orbit.still = a.ends_with("still"); self.orbit.save(); }
                a if a.starts_with("page scripts: ") => { if self.js != a.ends_with(": on") { return self.act("js", "", source); } }
                "back" => return self.act("back", "", source),
                "forward" => return self.act("forward", "", source),
                "reload" => return self.act("reload", "", source),
                "reader" | "page" => return self.act("read", "", source),
                "save" => return self.act("save", "", source),
                "new tab" => return self.act("tab", "new", source),
                a if a.starts_with("tab ") && a.contains(':') => { let n = a[4..].split(':').next().unwrap_or("").trim().to_string(); return self.act("tab", &n, source); }
                "cookies: kept" => return self.act("keep", "keep", source),
                "cookies: this session" => return self.act("keep", "session", source),
                "cookies: blocked" => return self.act("keep", "block", source),
                _ => { let t = self.tree(); let w = self.focus_widget(&t); if let Some(v) = self.activate(&t, w, source) { return v; } }
            },
            "next" => return self.act("scroll", "page", source),
            "save" => self.save(),
            "tab" => self.tab_cmd(arg),
            "bookmark" => self.bookmark(),
            "bookmarks" | "history" => self.open("about:bookmarks"),
            "copy" => self.copy(),
            "keep" => {
                // the site's cookie state: keep (sign-ins survive a restart) · session (default) · block (none stored, none sent)
                let site = self.site();
                if site.is_empty() { self.status = "No site open.".into(); } else {
                    let a = arg.trim().to_ascii_lowercase();
                    let (kept, blocked) = { let j = self.b.client.jar.borrow(); (j.keep.contains(&site), j.block.contains(&site)) };
                    let to = match a.as_str() { "keep" | "session" | "block" => a, _ => (if kept { "session" } else { "keep" }).to_string() }; // bare "keep" verb toggles as before
                    let _ = blocked;
                    { let mut j = self.b.client.jar.borrow_mut();
                      match to.as_str() { "keep" => j.set_keep(&site, true), "block" => j.set_block(&site, true), _ => { j.set_keep(&site, false); j.set_block(&site, false); } } }
                    self.status = match to.as_str() { "keep" => format!("Cookies kept for {site} (sign-ins survive a restart)."), "block" => format!("Cookies blocked for {site}: none stored, none sent."), _ => format!("Cookies for {site}: this session only.") };
                }
            }
            "forget" => { let site = self.site(); let n = self.b.client.jar.borrow().count_site(&site); self.b.client.jar.borrow_mut().clear_site(&site); self.status = format!("Forgot {n} cookies of {site}."); }
            _ => { self.status = format!("“{verb}” is not something Web does."); }
        }
        let extra = json!({"url": self.url, "y": self.y, "page_h": self.page_h, "back": self.back.len(), "fwd": self.fwd.len(),
                           "reader": self.reader, "find": self.find_boxes.len(), "zoom": self.zoom});
        self.log.record(verb, arg, source, extra)
    }

    fn app_key(&mut self, t: &Tree, key: &str, text: Option<&str>) -> Option<Value> {
        // chrome keys bring the bar back; reading keys (scroll, find-next) leave it folded
        if key.starts_with("Ctrl+") || key.starts_with("Alt+") || key == "F6" || (self.pfocus.is_none() && self.focus_ord == 0 && text.is_some()) { self.orbit.touch(); }
        if key == "Escape" && self.orbit.panel != crate::orbit::Panel::None { self.orbit.panel = crate::orbit::Panel::None; return None; }
        // browser chords (the shell names them "Ctrl+L", "Alt+ArrowLeft", …)
        match key {
            "Ctrl+," => { self.orbit.panel = crate::orbit::Panel::Settings; return None; }
            "Ctrl+L" | "F6" | "Alt+D" => { self.pfocus = None; self.focus_ord = 0; self.addr_sel = true; self.status = "Address: type a new one, Enter to open, Escape to keep the page.".into(); return None; }
            "Ctrl+F" => { self.pfocus = None; if self.find_edit.is_none() { self.find_edit = Some(self.find_q.clone()); } self.focus_ord = 1; self.status = "Find in page: type, Enter for the next match, Escape to close.".into(); return None; }
            "Alt+ArrowLeft" => return Some(self.act("back", "", "key")),
            "Alt+ArrowRight" => return Some(self.act("forward", "", "key")),
            "F5" | "Ctrl+R" => return Some(self.act("reload", "", "key")),
            "Ctrl+S" => return Some(self.act("save", "", "key")),
            "Ctrl+T" => return Some(self.act("tab", "new", "key")),
            "Ctrl+W" => return Some(self.act("tab", "close", "key")),
            "Ctrl+Tab" => return Some(self.act("tab", "next", "key")),
            "Ctrl+Shift+Tab" => return Some(self.act("tab", "prev", "key")),
            k if k.len() == 6 && k.starts_with("Ctrl+") && k.as_bytes()[5].is_ascii_digit() && k.as_bytes()[5] != b'0' => return Some(self.act("tab", &k[5..], "key")),
            "Ctrl+D" => return Some(self.act("bookmark", "", "key")),
            "Ctrl+B" | "Ctrl+H" => return Some(self.act("bookmarks", "", "key")),
            "Ctrl+C" => return Some(self.act("copy", "", "key")),
            "Ctrl+=" | "Ctrl++" => return Some(self.act("bigger", "", "key")),
            "Ctrl+-" => return Some(self.act("smaller", "", "key")),
            k if k.starts_with("Ctrl+") || k.starts_with("Alt+") => return None,
            _ => {}
        }
        let w = self.focus_widget(t);
        let in_text = self.pfocus.is_none() && t.kind[w as usize] == Kind::TextInput;
        if in_text {
            let is_find = t.label(w) == "Find in page";
            let is_field = !is_find && self.field.as_ref().map(|f| t.label(w) == f.label).unwrap_or(false);
            let is_addr = !is_find && !is_field;
            if is_addr && self.addr_sel && matches!(text, Some(x) if !x.chars().any(|c| c.is_control())) { self.addr.clear(); }
            if is_addr && key != "" { self.addr_sel = false; } else if is_addr && text.is_some() { self.addr_sel = false; }
            let s: &mut String = if is_find { self.find_edit.as_mut().unwrap() } else if is_field { &mut self.field.as_mut().unwrap().value } else { &mut self.addr };
            match key {
                "Backspace" => { s.pop(); return None; }
                "Escape" if is_find => { self.find_edit = None; self.find_boxes.clear(); self.find_q.clear(); self.focus_ord = 0; self.status = self.page_status(); return None; }
                "Escape" => { if is_field { self.field = None; self.focus_ord = 1; } else { self.addr = self.url.clone(); self.focus_ord = 1; } return None; }
                "PageDown" | "PageUp" | "ArrowDown" | "ArrowUp" => {}
                _ => { if let Some(x) = text { if x.chars().all(|c| !c.is_control()) { s.push_str(x); } } return None; }
            }
        }
        let v = match (key, text) {
            ("PageDown", _) | (_, Some(" ")) => self.act("scroll", "page", "key"),
            ("PageUp", _) => self.act("scroll", "-page", "key"),
            ("ArrowDown", _) => self.act("scroll", "60", "key"),
            ("ArrowUp", _) => self.act("scroll", "-60", "key"),
            ("Home", _) => self.act("scroll", "top", "key"),
            ("End", _) => self.act("scroll", "end", "key"),
            ("Backspace", _) => self.act("back", "", "key"),
            ("Escape", _) => { self.find_boxes.clear(); self.find_q.clear(); self.find_edit = None; self.pfocus = None; self.status = self.page_status(); return None; }
            (_, Some("+")) => self.act("bigger", "", "key"),
            (_, Some("-")) => self.act("smaller", "", "key"),
            (_, Some("n")) => self.act("find", "", "key"),
            (_, Some("r")) => self.act("read", "", "key"),
            (_, Some("s")) if self.b.download.borrow().is_some() => self.act("save", "", "key"),
            _ => return None,
        };
        Some(v)
    }

    fn scroll(&self) -> i32 { 0 }
    // Orbit draws the reader column shifted up by the scroll, so the measured extent is the content's + the scroll
    fn set_extent(&mut self, page_h: i32, view_h: i32) { if self.reader { self.rext = (page_h + self.rscroll, view_h); } }
    fn wheel(&mut self, dy: i32) { let c = self.cur_scroll(); self.scroll_to(c + dy); }

    fn surface(&mut self, w: usize, h: usize) -> Option<Vec<u32>> {
        if self.reader { return Some(self.reader_surface(w, h)); }
        // scripts running in the page process: show the last frame (or the plain page colour) until they answer
        if self.page.as_ref().map(|p| p.busy).unwrap_or(false) {
            return Some(match &self.shot { Some(s) if s.w == w && s.h == h => s.px.iter().map(|c| c >> 8).collect(), _ => vec![0xffffff; w * h] });
        }
        if (w, h) != self.view {
            self.view = (w, h);
            if let Some(p) = self.page.as_mut() { if let Some(m) = p.resize(&self.b, w, h) { self.page_h = m["page_h"].as_i64().unwrap_or(0) as i32; } }
            self.shot = None; self.a11y_rows = None;
            self.find_boxes.clear();
            self.y = self.y.clamp(0, (self.page_h - h as i32).max(0));
        }
        let p = self.page.as_mut()?;
        if self.shot.as_ref().map(|s| s.y != self.y || s.w != w || s.h != h).unwrap_or(true) {
            self.shot = p.frame(&self.b, self.y);
            // a page created at exactly this size can answer no frame until it has been sized once: size it and ask again
            if self.shot.as_ref().map(|s| s.w != w || s.h != h).unwrap_or(true) { if let Some(m) = p.resize(&self.b, w, h) { self.page_h = m["page_h"].as_i64().unwrap_or(0) as i32; } self.shot = p.frame(&self.b, self.y); }
        }
        let s = self.shot.as_ref()?;
        let mut px: Vec<u32> = s.px.iter().map(|c| c >> 8).collect();
        // keyboard focus in the page: a 3 px ring (WCAG 2.4.7 focus visible; 2.4.11 not obscured — scrolled into view)
        if let Some(r) = self.pfocus.and_then(|k| self.a11y_rows.as_ref().and_then(|rs| rs.get(k)).cloned()) {
            let (bx, by, bw, bh) = (r[3].as_i64().unwrap_or(0) as i32 - 3, r[4].as_i64().unwrap_or(0) as i32 - s.y - 3, r[5].as_i64().unwrap_or(0) as i32 + 6, r[6].as_i64().unwrap_or(0) as i32 + 6);
            for y in by.max(0)..(by + bh).min(h as i32) {
                for x in bx.max(0)..(bx + bw).min(w as i32) {
                    let (dx, dy) = ((x - bx).min(bx + bw - 1 - x), (y - by).min(by + bh - 1 - y));
                    let d = dx.min(dy);
                    if d < 3 { px[y as usize * w + x as usize] = if d == 2 { 0xffffff } else { 0x0047b3 }; }
                }
            }
        }
        // find: every match outlined, the current one tinted
        for (i, b) in self.find_boxes.iter().enumerate() {
            let (x0, y0) = (b[0].max(0), b[1] - s.y);
            let (x1, y1) = ((b[0] + b[2]).min(w as i32), y0 + b[3]);
            if y1 < 0 || y0 >= h as i32 { continue; }
            for y in y0.max(0)..y1.min(h as i32) {
                for x in x0..x1 {
                    let edge = y - y0 < 2 || y1 - y <= 2 || x - x0 < 2 || x1 - x <= 2;
                    let o = &mut px[y as usize * w + x as usize];
                    if edge { *o = 0xe07800; } else if i == self.find_ix { *o = ((*o >> 1) & 0x7f7f7f) + 0x7f6a00; }
                }
            }
        }
        Some(px)
    }

    fn surface_click(&mut self, x: i32, y: i32) -> Option<Value> { self.pfocus = None; Some(self.act("click", &format!("{x} {y}"), "pointer")) }

    /// Middle click on a link: open it in a new tab.
    fn surface_aux_click(&mut self, x: i32, y: i32) -> Option<Value> {
        if self.page.as_ref()?.busy { return None; }
        let h = self.page.as_mut()?.hit(&self.b, x, y + self.y);
        let l = h["link"].as_str()?.to_string();
        if !(l.starts_with("http://") || l.starts_with("https://")) { return None; }
        Some(self.act("tab", &format!("new {l}"), "pointer"))
    }

    fn tick_every(&self) -> Option<std::time::Duration> {
        // 10 Hz while the shown page has timers waiting (HTML timers are clamped ≥ 4 ms; 100 ms keeps an idle tab cheap);
        // Orbit adds 16 ms only while the chrome moves, 250 ms while a hidden bar waits to fold, 500 ms while a status shows
        let busy = self.page.as_ref().map(|p| p.busy).unwrap_or(false);
        let page = (busy || (self.js && self.page.is_some() && self.pending > 0 && !self.reader)).then(|| std::time::Duration::from_millis(if busy { 50 } else { 100 }));
        let orb = self.orbit.tick_every().or_else(|| self.orbit.status_pending().then(|| std::time::Duration::from_millis(500)));
        match (page, orb) { (Some(a), Some(b)) => Some(a.min(b)), (a, b) => a.or(b) }
    }

    fn tick(&mut self, ms: u64) -> bool {
        let editing = self.editing_addr();
        let chrome = self.orbit.tick(ms, editing) || self.orbit.status_pending();
        let busy = self.page.as_ref().map(|p| p.busy).unwrap_or(false);
        if !(busy || (self.js && self.pending > 0)) { return chrome; }
        self.page_tick(ms) || chrome
    }

    fn markup(&self, t: &Tree, focus: u32, w: usize, h: usize) -> Option<String> {
        let reader = self.reader.then(|| {
            let from = self.reader_from.get();
            let roots: Vec<u32> = (0..t.len() as u32).filter(|&i| i >= from && t.parent[i as usize] == 0).collect();
            let html = t.markup_of(&roots, focus);
            *self.reader_html.borrow_mut() = html.clone();
            html
        });
        Some(self.orbit.markup(t, focus, w, h, &self.orbit_view(), &self.status, reader.as_deref(), self.rscroll))
    }
    fn overlay(&self) -> bool { true }
    fn pointer_move(&mut self, x: i32, y: i32) -> bool { self.orbit.pointer(x, y, self.view.0) }
    fn page_a11y(&mut self) -> Option<(std::rc::Rc<Vec<Value>>, i32, Option<usize>)> {
        if self.reader { return None; }
        Some((self.rows()?, self.y, self.pfocus))
    }

    fn page_action(&mut self, row: usize, action: &str, value: Option<&str>) -> Option<Value> {
        let rows = self.rows()?;
        rows.get(row)?;
        match action {
            "focus" => { self.focus_row(row); None }
            "click" => Some(self.activate_row(row)),
            "set_value" => { self.activate_row(row); if let (Some(f), Some(v)) = (self.field.as_mut(), value) { f.value = v.to_string(); } None }
            _ => None,
        }
    }

    fn focus_widget(&self, t: &Tree) -> u32 {
        // keyboard focus inside the page: no chrome widget is focused (the page draws its own ring)
        if self.pfocus.is_some() { return 0; }
        let f = t.focusables(); if f.is_empty() { 0 } else { f[self.focus_ord.min(f.len() - 1)] }
    }

    fn key(&mut self, t: &Tree, key: &str, text: Option<&str>, shift: bool) -> Option<Value> {
        match key {
            "Tab" => { self.tab(t, shift); None }
            "Enter" => { if let Some(k) = self.pfocus { return Some(self.activate_row(k)); } let w = self.focus_widget(t); self.activate(t, w, "key") }
            // Ctrl+Enter on a focused link: open it in a new tab
            "Ctrl+Enter" => { let href = self.pfocus.and_then(|k| self.a11y_rows.as_ref().and_then(|r| r.get(k)).and_then(|r| r[7]["href"].as_str().map(|s| s.to_string()))); href.map(|h| self.act("tab", &format!("new {h}"), "key")) }
            // Space on a focused page control activates it (checkbox, button); on a link it pages, like Firefox
            _ if text == Some(" ") && self.pfocus.map(|k| self.row_role(k) != "link").unwrap_or(false) => { let k = self.pfocus.unwrap(); Some(self.activate_row(k)) }
            _ => self.app_key(t, key, text),
        }
    }

    fn state(&self) -> Value {
        json!({"url": self.url, "title": self.title_s, "y": self.y, "page_h": self.page_h, "back": self.back.iter().map(|b| &b.0).collect::<Vec<_>>(),
               "fwd": self.fwd.iter().map(|b| &b.0).collect::<Vec<_>>(), "status": self.status, "reader": self.reader,
               "find": {"q": self.find_q, "hits": self.find_boxes.len(), "ix": self.find_ix}, "zoom": self.zoom, "js_on": self.js,
               "js": self.page.as_ref().map(|p| p.meta["js"].clone()).unwrap_or(Value::Null),
               "renderer_rss_kb": self.page.as_ref().map(|p| p.rss_kb()).unwrap_or(0), "load_ms": self.load_ms,
               "pfocus": self.pfocus, "find_edit": self.find_edit, "addr": self.addr, "http": self.http,
               "tabs": (0..self.tabs.len()).map(|k| self.tab_title(k)).collect::<Vec<_>>(), "cur_tab": self.cur_tab,
               "live_tabs": self.tabs.iter().filter(|t| t.as_ref().map(|p| p.page.is_some()).unwrap_or(false)).count() + self.page.is_some() as usize,
               "bookmarks": self.bookmarks.len(), "seen": self.seen.len(), "pending": self.pending})
    }
}

impl WebApp {
    /// The shown page's timers (the 'k' tick): navigate / re-layout when a script asks.
    /// The page's clock, asynchronously: a tick is started and polled on later wakeups, so a page whose scripts run for
    /// seconds never stops the window (OMNI_WEB_SYNC=1 — scripts, shots, trees — waits for it instead).
    fn page_tick(&mut self, ms: u64) -> bool {
        let Some(p) = self.page.as_mut() else { return false };
        let r = if std::env::var_os("OMNI_WEB_SYNC").is_some() { p.tick(&self.b, ms as i64) } else {
            if !p.busy && !p.tick_start(ms as i64) { return false; }
            match p.tick_poll(&self.b) { Some(r) => r, None => { if self.status.is_empty() || !self.status.starts_with("Running") { self.status = "Running page scripts…".into(); } return false } }
        };
        if r["dead"] == true { self.pending = 0; self.status = "The page process stopped.".into(); return true; }
        if r.get("js").is_some() { if let Some(p) = self.page.as_mut() { p.meta["js"] = r["js"].clone(); } self.status = self.page_status(); }
        self.pending = r["pending"].as_u64().unwrap_or(0);
        if let Some(u) = r["navigate"].as_str() { let u = u.to_string(); self.open(&u); return true; }
        if r["relayout"] == true {
            let p = self.page.as_ref().unwrap();
            self.page_h = p.meta["page_h"].as_i64().unwrap_or(self.page_h as i64) as i32;
            if let Some(t) = p.meta["title"].as_str() { self.title_s = t.to_string(); }
            self.shot = None; self.a11y_rows = None; self.find_boxes.clear();
            if self.pfocus.is_some() { self.pfocus = None; }
            return true;
        }
        false
    }

    /// The address is being typed into (focus on it, a different text than the shown page's address).
    fn editing_addr(&self) -> bool { self.focus_ord == 0 && self.pfocus.is_none() && !self.reader && (self.addr != self.url || self.addr_sel || self.url.is_empty()) }

    /// Open tabs, bookmarks and pages seen this session whose title or address holds what is typed (≤ 7, no repeats).
    fn suggestions(&self) -> Vec<(String, String)> {
        let q = self.addr.trim().to_lowercase();
        if q.is_empty() || q.contains("://") && q == self.url.to_lowercase() { return vec![]; }
        let mut out: Vec<(String, String)> = Vec::new();
        let tabs: Vec<(String, String)> = (0..self.tabs.len()).filter(|&k| k != self.cur_tab).filter_map(|k| self.tabs[k].as_ref().map(|p| (p.title_s.clone(), p.url.clone()))).collect();
        for (t, u) in tabs.iter().chain(self.bookmarks.iter()).chain(self.seen.iter().rev()) {
            if u.is_empty() || out.iter().any(|x| x.1 == *u) { continue; }
            if t.to_lowercase().contains(&q) || u.to_lowercase().contains(&q) { out.push((if t.is_empty() { u.clone() } else { t.clone() }, u.clone())); }
            if out.len() >= 7 { break; }
        }
        out
    }

    /// What Orbit shows besides the tree: host / path and the site trit (connection · cookies · third parties).
    fn orbit_view(&self) -> crate::orbit::View {
        let u = self.url.as_str();
        let rest = u.split_once("://").map(|x| x.1).unwrap_or(u);
        let (host, path) = match rest.find('/') { Some(i) => (rest[..i].to_string(), rest[i..].to_string()), None => (rest.to_string(), String::new()) };
        let path = if path == "/" { String::new() } else { path };
        let conn: i8 = if u.is_empty() || u.starts_with("about:") { 0 } else if self.http >= 400 { -1 } else if u.starts_with("https://") { 1 } else { 0 };
        let site = self.site();
        let jar = self.b.client.jar.borrow();
        let (kept, n, blocked) = (jar.keep.contains(&site), jar.count_site(&site), jar.block.contains(&site));
        drop(jar);
        let cookies: i8 = if blocked { -1 } else if kept { 1 } else if n > 0 { 0 } else { 1 };
        let ct = if blocked { "blocked: none stored, none sent".to_string() } else if kept { "kept for this site (survive restarts)".to_string() } else if n > 0 { format!("{n} for this session only") } else { "none stored".into() };
        let conn_t = if u.starts_with("https://") { if self.http >= 400 { format!("TLS · the server answered HTTP {}", self.http) } else { "TLS · certificate checked".into() } }
            else if u.starts_with("http://") { "not encrypted (http)".into() } else { "—".into() };
        let host = if host.is_empty() { host } else { host };
        crate::orbit::View { site: site.clone(), host, path, trit: [conn, cookies, 1], trit_text: [conn_t, ct, "never sent our cookies (first-party only)".into()],
            loading: false, editing: self.editing_addr(), reader: self.reader }
    }

    /// Reader view's pixels: the reading column drawn ONCE at full height (re-drawn when its text, focus, theme or width
    /// change), then sliced at the scroll like a page — scrolling the reader costs a copy, not a layout + raster.
    fn reader_surface(&mut self, w: usize, h: usize) -> Vec<u32> {
        use std::hash::{Hash, Hasher};
        let html = self.reader_html.borrow().clone();
        let css = self.orbit.css();
        let mut hs = std::collections::hash_map::DefaultHasher::new();
        (&html, &css, w).hash(&mut hs);
        let key = hs.finish();
        if self.reader_img.as_ref().map(|r| r.0 != key || r.1 != w).unwrap_or(true) {
            let cw = (w as i32 - 160).clamp(280, 720);
            let doc = format!("<!doctype html><html><head><style>{css}</style></head><body style=\"background:var(--void)\"><div class=reader style=\"left:{}px;top:92px;width:{cw}px\">{html}</div></body></html>", (w as i32 - cw) / 2);
            if let Ok(p) = std::env::var("OMNI_WEB_READER_DUMP") { let _ = std::fs::write(p, &doc); }
            let fonts = self.reader_fonts.get_or_insert_with(crate::font::Fonts::new);
            let probe = crate::frame::render(&doc, fonts, w, h.max(1), 0);
            let full = (probe.page_h as usize + 120).clamp(h, 40_000);
            let f = if full > h { crate::frame::render(&doc, fonts, w, full, 0) } else { probe };
            self.reader_img = Some((key, w, full, f.pixels));
        }
        let (_, _, ih, img) = self.reader_img.as_ref().unwrap();
        let void = img.first().copied().unwrap_or(0);
        let y0 = self.rscroll.max(0) as usize;
        let mut out = vec![void; w * h];
        for y in 0..h { let sy = y0 + y; if sy >= *ih { break; } out[y * w..(y + 1) * w].copy_from_slice(&img[sy * w..(sy + 1) * w]); }
        out
    }
}
