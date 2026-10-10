// SPDX-License-Identifier: MIT OR Apache-2.0
//! OmniUI — the NeuralOS app toolkit (NEURAL-APPS-0 step 2): packed widget tree -> the OMNI-RENDER engine (packed DOM,
//! integer layout, 2-bit glyph tiles, residue-grid hit test) -> window; the same tree -> AT-SPI; intents in from the
//! access.bus. Apps: Find (step 4), Reader (step 5). Plan: an unpublished plan Manual: docs/systems/omni-ui/MANUAL.md.
#![allow(dead_code, unused_imports, unused_variables, unused_mut, unused_assignments, unused_parens, unreachable_patterns)]

// The engine, compiled from .engine/src = a committed omni-render revision (tools/pin-engine.sh): that repo belongs to the engine's owner and is never edited
// from here; OmniUI only calls its public functions.
#[path = "../.engine/src/agent.rs"] mod agent;
#[path = "../.engine/src/css.rs"] mod css;
#[path = "../.engine/src/dom.rs"] mod dom;
#[path = "../.engine/src/font.rs"] mod font;
#[path = "../.engine/src/hit.rs"] mod hit;
#[path = "../.engine/src/img.rs"] mod img;
#[path = "../.engine/src/layout.rs"] mod layout;
#[path = "../.engine/src/net.rs"] mod net;
#[path = "../.engine/src/cookie.rs"] mod cookie;
#[path = "../.engine/src/page.rs"] mod page;
#[path = "../.engine/src/raster.rs"] mod raster;
#[path = "../.engine/src/paint.rs"] mod paint;
#[path = "../.engine/src/sandbox.rs"] mod sandbox;
#[path = "../.engine/src/style.rs"] mod style;
#[path = "../.engine/src/svg.rs"] mod svg;
#[path = "../.engine/src/woff.rs"] mod woff;
#[path = "../.engine/src/scroll.rs"] mod scroll;
#[path = "../.engine/src/pageproc.rs"] mod pageproc;
#[path = "../.engine/src/jsi.rs"] mod jsi;
#[path = "../.engine/src/jsi_ast.rs"] mod jsi_ast;
#[path = "../.engine/src/jsc.rs"] mod jsc;
#[path = "../.engine/src/jscomp.rs"] mod jscomp;
#[path = "../.engine/src/jsi_lib.rs"] mod jsi_lib;
#[path = "../.engine/src/jsi_re.rs"] mod jsi_re;
#[path = "../.engine/src/jsi_date.rs"] mod jsi_date;
#[path = "../.engine/src/jsparse.rs"] mod jsparse;
#[path = "../.engine/src/jsdom.rs"] mod jsdom;
#[path = "../.engine/src/why.rs"] mod why;
#[path = "../.engine/src/jit/x86.rs"] mod jit_x86;
#[path = "../.engine/src/jit/sift.rs"] mod jit_sift;
#[path = "../.engine/src/jit/ir.rs"] mod jit_ir;
#[cfg(feature = "t25")] #[path = "../.engine/src/jit/t25.rs"] mod jit_t25;
#[cfg(not(feature = "t25"))] #[path = "../.engine/src/jit/t25_stub.rs"] mod jit_t25; // MIT OR Apache-2.0 no-op hooks
pub(crate) use jit_x86 as x86; // jit_sift / jit_t25 read `super::x86`
pub(crate) use jit_sift as sift; // jit_ir reads `super::sift`
pub(crate) use jit_ir as ir; // jit_t25 reads `super::ir` (engine 188e178: Layout::run fires the T25 doctor core)
/// jit_sift's bench helper reads `super::time_ns` (median ns of `reps` timed runs, at least `min_ms` each).
#[allow(dead_code)]
fn time_ns(reps: usize, min_ms: f64, mut f: impl FnMut()) -> f64 {
    let mut v = vec![];
    for _ in 0..reps { let t = std::time::Instant::now(); let mut n = 0u64; while t.elapsed().as_secs_f64() * 1e3 < min_ms { f(); n += 1; } v.push(t.elapsed().as_secs_f64() * 1e9 / n.max(1) as f64); }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2]
}
mod jit { pub(crate) use crate::jit_x86 as x86; pub(crate) use crate::jit_sift as sift; pub(crate) use crate::jit_t25 as t25; }

mod a11y;
mod app;
mod cal_app;
mod ics;
mod library_app;
mod mail_app;
mod md;
mod notes_app;
mod find_app;
mod fit;
mod frame;
mod reader_app;
mod recur;
mod shell;
#[cfg(feature = "t25")] mod t25_app;
mod vault;
mod web_app;
#[path = "repair_shim.rs"] mod repair;
mod orbit;
mod widgets;

use app::App;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};

const W: usize = 900;
const H: usize = 640;
/// headless window size (the `script` command's `size W H` changes it: resize without a window)
static DIMS: std::sync::Mutex<(usize, usize)> = std::sync::Mutex::new((W, H));

/// `find [query]` | `reader <file> [--px N] [--next K]` | `<query>` (Find, the v0 form). Returns the app and its socket name.
fn flag_s(args: &[String], k: &str) -> Option<String> { args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned() }

fn make_app(args: &[String]) -> (Box<dyn App>, &'static str) {
    let flag = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<i32>().ok());
    match args.first().map(|s| s.as_str()) {
        Some("calendar") => {
            let today = flag_s(args, "--from").and_then(|s| cal_app::parse_iso(&s)).unwrap_or_else(|| {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64 / 86_400).unwrap_or(0)
            });
            (Box::new(cal_app::CalApp::open(std::path::Path::new(args.get(1).map(|s| s.as_str()).unwrap_or("calendar.ics")), today)), "calendar")
        }
        #[cfg(feature = "t25")]
        Some("t25") => (Box::new(t25_app::T25App::new()), "t25"),
        Some("library") => {
            let mut l = library_app::LibraryApp::new();
            if let Some(p) = flag_s(args, "--open") { l.act("open", &p, "cli"); }
            if args.iter().any(|a| a == "--full") { l.full = true; }
            (Box::new(l), "library")
        }
        Some("notes") => {
            let home = std::env::var("HOME").unwrap_or_default();
            let root = args.get(1).filter(|p| !p.starts_with("--")).cloned().unwrap_or(format!("{home}/Documents/Obsidian"));
            let mut n = notes_app::NotesApp::open(std::path::Path::new(&root));
            if let Some(rel) = flag_s(args, "--open") { n.act("open", &rel, "cli"); }
            (Box::new(n), "notes")
        }
        Some("mail") => {
            let mut m = mail_app::MailApp::open(std::path::Path::new(args.get(1).map(|s| s.as_str()).unwrap_or("mbox")));
            if let Some(i) = flag_s(args, "--open").and_then(|v| v.parse::<usize>().ok()) { m.open = Some(i); }
            (Box::new(m), "mail")
        }
        Some("reader") => {
            let mut r = reader_app::ReaderApp::new();
            if let Some(p) = args.get(1).filter(|p| !p.starts_with("--")) { r.open(std::path::Path::new(p)); }
            if let Some(px) = flag("--px") { if let Some(i) = reader_app::SIZES.iter().position(|&s| s == px) { r.size_ix = i; } }
            (Box::new(r), "reader")
        }
        Some("web") => {
            let mut w = web_app::WebApp::new();
            if let Some(u) = args.get(1).filter(|p| !p.starts_with("--")) { w.act("open", u, "cli"); }
            if let Some(q) = flag_s(args, "--find") { w.act("find", &q, "cli"); }
            if let Some(n) = flag_s(args, "--scroll") { w.act("scroll", &n, "cli"); }
            if args.iter().any(|a| a == "--reader") { w.act("read", "", "cli"); }
            if let Some(z) = flag("--zoom") { while w.zoom < z { w.act("bigger", "", "cli"); } while w.zoom > z { w.act("smaller", "", "cli"); } }
            (Box::new(w), "web")
        }
        first => {
            let mut f = find_app::FindApp::new();
            let q = if first == Some("find") { args.get(1) } else { args.first() };
            if let Some(q) = q { f.quiet = true; f.act("find", q, "cli"); }
            (Box::new(f), "find")
        }
    }
}

/// Render once; when the app scrolls (--next K pages), render again so the page height is known first.
fn headless(app: &mut dyn App, fonts: &font::Fonts, nexts: i32) -> (widgets::Tree, frame::Frame, std::time::Duration) {
    let t0 = std::time::Instant::now();
    let mut out = None;
    for pass in 0..=nexts {
        let tree = app.tree();
        let focus = app.focus_widget(&tree);
        let (w, h) = *DIMS.lock().unwrap();
        let f = frame::render_app(app, &tree, focus, fonts, w, h);
        app.set_extent(f.page_h, h as i32);
        out = Some((tree, f));
        if pass < nexts { app.act("next", "", "cli"); }
    }
    let (t, f) = out.unwrap();
    (t, f, t0.elapsed())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rest = |i: usize| a.get(i..).map(|s| s.to_vec()).unwrap_or_default();
    let nexts = a.iter().position(|x| x == "--next").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);
    match a.get(1).map(|s| s.as_str()) {
        // the engine revision this binary was built from (build.rs bakes .engine/REV in)
        Some("--engine-rev") => println!("{}", env!("OMNI_ENGINE_REV")),
        Some("web-renderer") => pageproc::serve(),
        Some("script") => {
            // headless runs wait for a page's scripts (the window runs them asynchronously)
            unsafe { std::env::set_var("OMNI_WEB_SYNC", "1"); }
            // headless intent script: `script <app args…> --script FILE` — one "verb arg" per line (# comments), state after each
            let i = a.iter().position(|x| x == "--script").expect("--script FILE");
            let (mut app, _) = make_app(&a[2..i].to_vec());
            let fonts = font::Fonts::new();
            for line in std::fs::read_to_string(&a[i + 1]).expect("script file").lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') { continue; }
                let (verb, arg) = line.split_once(' ').unwrap_or((line, ""));
                if verb == "size" { let mut it = arg.split_whitespace().map(|v| v.parse::<usize>().unwrap_or(0)); *DIMS.lock().unwrap() = (it.next().unwrap_or(W).max(200), it.next().unwrap_or(H).max(200));
                    let t0 = std::time::Instant::now(); let (_, f, _) = headless(app.as_mut(), &fonts, 0);
                    println!("{}", json!({"do": line, "ms": (t0.elapsed().as_secs_f64() * 1e4).round() / 10.0, "w": f.w, "h": f.h, "state": app.state()})); continue; }
                if verb == "hold" { std::thread::sleep(std::time::Duration::from_millis(arg.parse().unwrap_or(1000))); continue; }
                // `hover X Y`: the pointer moves there (hover-driven chrome: Orbit's folded bar springs back when touched)
                if verb == "hover" { let mut it = arg.split_whitespace().map(|v| v.parse::<i32>().unwrap_or(0)); let (x, y) = (it.next().unwrap_or(0), it.next().unwrap_or(0)); app.pointer_move(x, y); continue; }
                if verb == "tick" {
                    // real time passing for the app's clock: `tick MS [N]` = N ticks of MS ms
                    let mut it = arg.split_whitespace().map(|v| v.parse::<u64>().unwrap_or(0));
                    let (ms, n) = (it.next().unwrap_or(100), it.next().unwrap_or(1));
                    let mut redraws = 0;
                    for _ in 0..n { if app.tick(ms) { redraws += 1; } }
                    let (_, _f, _) = headless(app.as_mut(), &fonts, 0);
                    println!("{}", json!({"do": line, "redraws": redraws, "state": app.state()}));
                    continue;
                }
                if verb == "key" || verb == "shift-key" {
                    // a key as the window would deliver it: a named key / chord ("Tab", "Ctrl+F") or one typed character
                    let t = app.tree();
                    let one = arg.chars().count() == 1;
                    let rec = app.key(&t, if one { "" } else { arg }, if one { Some(arg) } else { None }, verb == "shift-key");
                    let (_, _f, _) = headless(app.as_mut(), &fonts, 0);
                    println!("{}", json!({"do": line, "action": rec, "focus_ord": app.focus_ord(), "state": app.state()}));
                    continue;
                }
                if verb == "shot" { let (_, f, _) = headless(app.as_mut(), &fonts, 0); f.png(arg).expect("png"); println!("{}", json!({"shot": arg})); continue; }
                let t0 = std::time::Instant::now();
                app.act(verb, arg, "script");
                let (_, _f, _) = headless(app.as_mut(), &fonts, 0);
                println!("{}", json!({"do": line, "ms": (t0.elapsed().as_secs_f64() * 1e4).round() / 10.0, "state": app.state()}));
            }
        }
        Some("run") => { let (app, name) = make_app(&rest(2)); shell::run(app, name) }
        Some("tree") => {
            // headless runs wait for a page's scripts (the window runs them asynchronously)
            unsafe { std::env::set_var("OMNI_WEB_SYNC", "1"); }
            // the accessibility tree a screen reader receives, without a window
            let (mut app, _) = make_app(&rest(2));
            let fonts = font::Fonts::new();
            let (t, f, el) = headless(app.as_mut(), &fonts, nexts);
            let page = app.page_a11y();
            let up = a11y::tree_update(&t, &f, app.focus_widget(&t), page.as_ref().map(|(r, y, k)| (&r[..], *y, *k)));
            for (id, n) in &up.nodes {
                let b = n.bounds().map(|r| format!("[{:.0},{:.0} {:.0}x{:.0}]", r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0)).unwrap_or_default();
                let l: String = n.label().or(n.value()).unwrap_or("").chars().take(90).collect();
                println!("{:?} {:?} {:?} {}", id, n.role(), l, b);
            }
            println!("widgets {} ({} B packed) · DOM nodes {} · frame {:.1} ms (style {:.1}, layout {:.1}, raster {:.1}) · tiles {} unique of {} · page {} px, scroll {}",
                     t.len(), t.bytes(), f.dom_nodes, el.as_secs_f64() * 1e3, f.ms_style, f.ms_layout, f.ms_raster, f.tiles_unique, f.tiles_total, f.page_h, f.scroll);
        }
        Some("shot") => {
            // headless runs wait for a page's scripts (the window runs them asynchronously)
            unsafe { std::env::set_var("OMNI_WEB_SYNC", "1"); }
            let (mut app, _) = make_app(&rest(3));
            let fonts = font::Fonts::new();
            let (_, f, el) = headless(app.as_mut(), &fonts, nexts);
            f.png(&a[2]).expect("png");
            println!("{} ({}x{}, scroll {} of {} px, frame {:.1} ms)", a[2], f.w, f.h, f.scroll, f.page_h, el.as_secs_f64() * 1e3);
        }
        Some("measure") => {
            // Reader reflow facts at every text size, as JSON (tests/verify_reader.py)
            let path = a.get(3).expect("measure reader <file>");
            let fonts = font::Fonts::new();
            let mut r = reader_app::ReaderApp::new();
            r.open(std::path::Path::new(path));
            let text: Vec<String> = r.paras.clone();
            let mut sizes = vec![];
            for i in 0..reader_app::SIZES.len() {
                r.size_ix = i; r.scroll = 0;
                let (t, f, el) = headless(&mut r, &fonts, 0);
                let tree_text: Vec<String> = (0..t.len() as u32).filter(|&w| t.kind[w as usize] == widgets::Kind::Paragraph).map(|w| t.label(w).to_string()).collect();
                let top = f.pixels.iter().fold(0u64, |h, &p| h.wrapping_mul(1_099_511_628_211).wrapping_add(p as u64));
                r.act("next", "", "cli");
                let (_, f2, _) = headless(&mut r, &fonts, 0);
                let moved = f2.pixels.iter().fold(0u64, |h, &p| h.wrapping_mul(1_099_511_628_211).wrapping_add(p as u64)) != top;
                sizes.push(json!({"px": r.px(), "page_h": f.page_h, "max_right": f.max_right, "width": W, "view_h": H,
                                  "frame_ms": (el.as_secs_f64() * 1e3 * 10.0).round() / 10.0, "paragraphs": tree_text.len(),
                                  "tree_text_equal": tree_text == text, "next_scroll": f2.scroll, "next_frame_differs": moved,
                                  "tiles_unique": f.tiles_unique, "tiles_total": f.tiles_total}));
            }
            println!("{}", json!({"file": path, "status": r.status, "words": r.words().len(), "text": text.join("\n\n"), "sizes": sizes}));
        }
        Some("essence-dump") => {
            // L1 check: md::essence for every document of the Library corpus (JSON {path: essence})
            let docs = library_app::corpus();
            let m: serde_json::Map<String, Value> = docs.iter().map(|d| (d.path.to_string_lossy().to_string(), json!(md::essence(&String::from_utf8_lossy(&std::fs::read(&d.path).unwrap_or_default()))))).collect();
            println!("{}", Value::Object(m));
        }
        Some("inkcheck") if a.get(2).map(|s| s.as_str()) == Some("library") => {
            // every document, essence and full text, every part: drawn ink inside the window
            let fonts = font::Fonts::new();
            let mut l = library_app::LibraryApp::new();
            let (mut rows, t0) = (vec![], std::time::Instant::now());
            for i in 0..l.docs.len() {
                for full in [false, true] {
                    l.open = Some(i); l.full = full; l.scroll = 0; l.part = 0;
                    let parts = l.parts();
                    let (mut right, mut ms) = (0, 0f64);
                    for p in 0..parts { l.part = p; let (_, f, el) = headless(&mut l, &fonts, 0); right = right.max(f.max_right); ms = ms.max(el.as_secs_f64() * 1e3); }
                    rows.push(json!({"full": full, "max_right": right, "parts": parts, "ms": (ms * 10.0).round() / 10.0}));
                }
            }
            println!("{}", json!({"width": W, "docs": l.docs.len(), "total_s": (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0, "rows": rows}));
        }
        Some("inkcheck") if a.get(2).map(|s| s.as_str()) == Some("notes") => {
            // Notes: render every note once (robustness on real markdown + drawn ink inside the window)
            let fonts = font::Fonts::new();
            let mut n = notes_app::NotesApp::open(std::path::Path::new(&a[3]));
            let (mut rows, t0) = (vec![], std::time::Instant::now());
            for k in 0..n.vault.notes.len() as u32 {
                n.current = Some(k); n.scroll = 0; n.part = 0;
                let parts = n.parts();
                let (mut right, mut ms, mut widgets, mut page_h) = (0, 0f64, 0, 0);
                for p in 0..parts { // every part of a long note
                    n.part = p;
                    let (t, f, el) = headless(&mut n, &fonts, 0);
                    right = right.max(f.max_right); ms = ms.max(el.as_secs_f64() * 1e3); widgets = widgets.max(t.len()); page_h = page_h.max(f.page_h);
                }
                rows.push(json!({"max_right": right, "page_h": page_h, "widgets": widgets, "parts": parts, "ms": (ms * 10.0).round() / 10.0}));
            }
            println!("{}", json!({"width": W, "notes": rows.len(), "total_s": (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0, "rows": rows}));
        }
        Some("inkcheck") => {
            // Mail: render every conversation once; widest drawn ink and page height per conversation (JSON)
            let fonts = font::Fonts::new();
            let mut m = mail_app::MailApp::open(std::path::Path::new(&a[3]));
            let mut rows = vec![];
            for k in 0..m.threads.len() {
                m.open = Some(k); m.scroll = 0;
                let (_, f, el) = headless(&mut m, &fonts, 0);
                rows.push(json!({"k": k, "max_right": f.max_right, "page_h": f.page_h, "ms": (el.as_secs_f64() * 1e4).round() / 10.0}));
            }
            println!("{}", json!({"width": W, "threads": m.threads.len(), "rows": rows}));
        }
        Some("vault-check") => {
            // N1: index a vault, resolve every link; with --expect, compare against expected targets (tests/fixtures/vault-expect.json)
            let v = vault::Vault::open(std::path::Path::new(&a[2]));
            let mut out = v.stats();
            let key = |l: &vault::Link| {
                let mut k = l.target.clone();
                if let Some(s) = &l.sub { k = format!("{k}#{s}"); }
                if l.kind != vault::Kind::Md && l.shown != k { k = format!("{k}|{}", l.shown); }
                k
            };
            let got = |l: &vault::Link| {
                let base = v.target_name(l.res);
                if l.ambiguous { return format!("AMBIGUOUS:{base}"); }
                match l.heading_found { Some(true) => format!("{base}#found"), Some(false) => format!("{base}#missing"), None => base }
            };
            if let Some(ex) = flag_s(&a, "--expect") {
                let exp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(ex).unwrap()).unwrap();
                let mut rows = vec![];
                let mut ok = 0;
                for e in exp.as_array().unwrap() {
                    let (src, raw, want) = (e[0].as_str().unwrap(), e[1].as_str().unwrap(), e[2].as_str().unwrap());
                    let n = v.find(src);
                    let g = n.and_then(|n| v.notes[n as usize].links.iter().find(|l| key(l) == raw).map(|l| got(l))).unwrap_or_else(|| "NOT PARSED".into());
                    let pass = if let Some(c) = want.strip_prefix("AMBIGUOUS:") { g.strip_prefix("AMBIGUOUS:").is_some_and(|x| c.split('|').any(|y| y == x)) } else { g == want };
                    ok += pass as usize;
                    rows.push(serde_json::json!({"source": src, "link": raw, "want": want, "got": g, "pass": pass, "rule": e[3]}));
                }
                // links that must NOT exist: inside code and in skipped folders
                let all: Vec<String> = v.notes.iter().flat_map(|n| n.links.iter().map(|l| key(l))).collect();
                let leaked: Vec<&String> = all.iter().filter(|k| k.as_str() == "In Code" || k.as_str() == "Fenced").collect();
                let skipped = v.notes.iter().any(|n| n.rel.starts_with(".trash") || n.rel.starts_with(".obsidian"));
                out["expect"] = serde_json::json!({"total": rows.len(), "pass": ok, "code_links_leaked": leaked.len(), "hidden_folders_indexed": skipped, "rows": rows});
            }
            if a.iter().any(|x| x == "--dump") {
                out["links_dump"] = serde_json::json!(v.notes.iter().flat_map(|n| n.links.iter().map(move |l| (n.rel.clone(), l))).map(|(s, l)| serde_json::json!([s, format!("{:?}", l.kind), l.target, v.target_name(l.res), l.ambiguous])).collect::<Vec<_>>());
            }
            println!("{}", out);
        }
        Some("agenda") => {
            // occurrences + first clashes as JSON: `agenda <ics> --from YYYY-MM-DD --days N`
            let from = flag_s(&a, "--from").and_then(|s| cal_app::parse_iso(&s)).expect("--from YYYY-MM-DD");
            let days = flag_s(&a, "--days").and_then(|s| s.parse().ok()).unwrap_or(56);
            println!("{}", cal_app::agenda_json(std::path::Path::new(&a[2]), from, days));
        }
        Some("recur-check") => {
            // NEURAL-APPS-0 C1: residue-clock recurrence vs direct enumeration on N random rule pairs
            let n = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(10_000);
            let r = recur::check(n, 0x9E37_79B9_7F4A_7C15);
            std::fs::create_dir_all("results").ok();
            std::fs::write("results/recur_check.json", serde_json::to_string_pretty(&r).unwrap()).unwrap();
            let mut s = r.clone(); s.as_object_mut().unwrap().remove("crt_pairs");
            println!("{}", serde_json::to_string_pretty(&s).unwrap());
        }
        Some("send") => {
            // one JSON line to a running window's intent socket; prints the reply. `send [find|reader] '<json>'`
            let (name, msg) = if a.len() > 3 { (a[2].as_str(), &a[3]) } else { ("find", &a[2]) };
            let mut s = std::os::unix::net::UnixStream::connect(shell::sock_path(name)).expect("omni-ui is not running (omni-ui run)");
            writeln!(s, "{msg}").unwrap();
            let mut line = String::new();
            BufReader::new(&s).read_line(&mut line).unwrap();
            print!("{line}");
        }
        _ => eprintln!("usage: omni-ui run|tree|shot <out.png> [find [query] | reader <file> [--px N]] [--next K] | measure reader <file> | send [app] '<json>'"),
    }
}
