// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vault — read-forward index of an Obsidian vault (NEURAL-APPS-0 step 8). Never writes: the author's scope for vault apps is "Obsidian
//! stays the brain; never silently corrupt a real note" (an unpublished review).
//! Links: [[x]] [[x|shown]] [[x#heading]] [[#heading]] ![[embed]] and [text](path.md) (%-encoded); none inside code fences or
//! inline code. Resolution (Obsidian help / forum: names match case-insensitively; a unique name links by name, a duplicate by
//! its path from the vault root; attachments by full file name): ./ ../ relative → vault-root path → path relative to the source
//! folder → path suffix → unique name → ambiguous name (same folder as the source, then shortest path; flagged).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Wiki, Embed, Md }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Res { Note(u32), File(u32), SameNote, Unresolved }

#[derive(Clone, Debug)]
pub struct Link { pub kind: Kind, pub target: String, pub sub: Option<String>, pub shown: String, pub res: Res, pub ambiguous: bool, pub heading_found: Option<bool> }

pub struct Note { pub rel: String, pub headings: Vec<String>, pub links: Vec<Link> }

pub struct Vault {
    pub root: PathBuf,
    pub notes: Vec<Note>,
    pub files: Vec<String>,
    note_path: HashMap<String, u32>,
    note_name: HashMap<String, Vec<u32>>,
    file_path: HashMap<String, u32>,
    file_name: HashMap<String, Vec<u32>>,
    pub backlinks: Vec<Vec<u32>>,
    pub index_ms: f64,
}

/// Symlinked folders are followed (Obsidian follows them); each real folder is visited once, so a link loop cannot hang the index.
fn walk(dir: &Path, root: &Path, out: &mut Vec<String>, seen: &mut std::collections::HashSet<PathBuf>) {
    if let Ok(c) = std::fs::canonicalize(dir) { if !seen.insert(c) { return; } }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; } // .obsidian, .trash, .git
        if p.is_dir() { walk(&p, root, out, seen); } else if let Ok(r) = p.strip_prefix(root) { out.push(r.to_string_lossy().replace('\\', "/")); }
    }
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'%')); i += 3; continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn folder(rel: &str) -> &str { rel.rfind('/').map(|i| &rel[..i]).unwrap_or("") }
fn strip_md(s: &str) -> &str { if s.to_ascii_lowercase().ends_with(".md") { &s[..s.len() - 3] } else { s } }
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for p in path.split('/') { match p { "" | "." => {} ".." => { parts.pop(); } x => parts.push(x) } }
    parts.join("/")
}
fn heading_key(h: &str) -> String { h.trim().trim_start_matches('#').trim().to_lowercase() }

/// Text with fenced code blocks and inline code spans blanked (same length per line, so nothing inside code looks like a link).
fn without_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        let marker = if t.starts_with("```") { Some("```") } else if t.starts_with("~~~") { Some("~~~") } else { None };
        if let Some(f) = fence {
            if t.starts_with(f) { fence = None; }
            out.push_str(&" ".repeat(line.trim_end_matches('\n').len())); if line.ends_with('\n') { out.push('\n'); }
            continue;
        }
        if let Some(m) = marker { fence = Some(m); out.push_str(&" ".repeat(line.trim_end_matches('\n').len())); if line.ends_with('\n') { out.push('\n'); } continue; }
        let mut in_code = false;
        for c in line.chars() {
            if c == '`' { in_code = !in_code; out.push(' '); } else if in_code && c != '\n' { out.push(' '); } else { out.push(c); }
        }
    }
    out
}

pub fn parse_links(text: &str) -> Vec<(Kind, String, Option<String>, String)> {
    let t = without_code(text);
    let mut out = vec![];
    let b = t.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'[' && b[i + 1] == b'[' {
            if let Some(end) = t[i + 2..].find("]]") {
                let inner = &t[i + 2..i + 2 + end];
                if !inner.contains('\n') && !inner.is_empty() {
                    let embed = i > 0 && b[i - 1] == b'!';
                    let inner = inner.replace("\\|", "|");
                    let (tp, shown) = match inner.split_once('|') { Some((a, s)) => (a.to_string(), s.to_string()), None => (inner.clone(), inner.clone()) };
                    let (target, sub) = match tp.split_once('#') { Some((a, h)) => (a.trim().to_string(), Some(h.to_string())), None => (tp.trim().to_string(), None) };
                    out.push((if embed { Kind::Embed } else { Kind::Wiki }, target, sub, shown));
                    i += 2 + end + 2;
                    continue;
                }
            }
        }
        if b[i] == b']' && b[i + 1] == b'(' {
            // [shown](target) — find the opening '[' on the same line
            let line_start = t[..i].rfind('\n').map(|x| x + 1).unwrap_or(0);
            if let (Some(open), Some(close)) = (t[line_start..i].rfind('['), t[i + 2..].find(')')) {
                let shown = t[line_start + open + 1..i].to_string();
                let mut url = t[i + 2..i + 2 + close].trim().to_string();
                if url.starts_with('<') { url = url.trim_matches(|c| c == '<' || c == '>').to_string(); }
                if let Some(sp) = url.find(" \"") { url.truncate(sp); }
                let lower = url.to_lowercase();
                let external = lower.contains("://") || lower.starts_with("mailto:") || lower.starts_with("obsidian:") || lower.starts_with("data:");
                if !external && !url.is_empty() {
                    let dec = pct_decode(&url);
                    let (target, sub) = match dec.split_once('#') { Some((a, h)) => (a.to_string(), Some(h.to_string())), None => (dec.clone(), None) };
                    out.push((Kind::Md, target, sub, shown));
                }
                i += 2 + close + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

impl Vault {
    pub fn open(root: &Path) -> Vault {
        let t0 = std::time::Instant::now();
        let mut all = vec![];
        walk(root, root, &mut all, &mut std::collections::HashSet::new());
        all.sort();
        let (mut notes, mut files) = (vec![], vec![]);
        for r in all { if r.to_ascii_lowercase().ends_with(".md") { notes.push(r) } else { files.push(r) } }
        let mut v = Vault { root: root.to_path_buf(), notes: vec![], files: files.clone(), note_path: HashMap::new(), note_name: HashMap::new(),
                            file_path: HashMap::new(), file_name: HashMap::new(), backlinks: vec![], index_ms: 0.0 };
        for (i, r) in notes.iter().enumerate() {
            let key = strip_md(r).to_lowercase();
            v.note_path.insert(key.clone(), i as u32);
            v.note_name.entry(key.rsplit('/').next().unwrap_or(&key).to_string()).or_default().push(i as u32);
        }
        for (i, r) in files.iter().enumerate() {
            let key = r.to_lowercase();
            v.file_path.insert(key.clone(), i as u32);
            v.file_name.entry(key.rsplit('/').next().unwrap_or(&key).to_string()).or_default().push(i as u32);
        }
        let mut parsed = vec![];
        for r in &notes {
            let text = std::fs::read_to_string(root.join(r)).unwrap_or_default();
            let headings: Vec<String> = without_code(&text).lines().filter(|l| { let t = l.trim_start(); t.starts_with('#') && t.trim_start_matches('#').starts_with(' ') }).map(heading_key).collect();
            parsed.push((r.clone(), headings, parse_links(&text)));
        }
        v.notes = parsed.iter().map(|(r, h, _)| Note { rel: r.clone(), headings: h.clone(), links: vec![] }).collect();
        v.backlinks = vec![vec![]; v.notes.len()];
        for (i, (r, _, links)) in parsed.into_iter().enumerate() {
            let mut out = vec![];
            for (kind, target, sub, shown) in links {
                let (res, ambiguous) = v.resolve(&target, &r, kind);
                let heading_found = match (res, &sub) {
                    (Res::Note(n), Some(h)) if !h.starts_with('^') => Some(v.notes[n as usize].headings.contains(&heading_key(h))),
                    (Res::SameNote, Some(h)) if !h.starts_with('^') => Some(v.notes[i].headings.contains(&heading_key(h))),
                    _ => None,
                };
                if let Res::Note(n) = res { if n as usize != i && !v.backlinks[n as usize].contains(&(i as u32)) { v.backlinks[n as usize].push(i as u32); } }
                out.push(Link { kind, target, sub, shown, res, ambiguous, heading_found });
            }
            v.notes[i].links = out;
        }
        v.index_ms = t0.elapsed().as_secs_f64() * 1e3;
        v
    }

    fn pick(&self, cands: &[u32], src_folder: &str, note: bool) -> u32 {
        let path = |i: u32| if note { self.notes[i as usize].rel.as_str() } else { self.files[i as usize].as_str() };
        if let Some(&c) = cands.iter().find(|&&c| folder(path(c)).eq_ignore_ascii_case(src_folder)) { return c; }
        *cands.iter().min_by_key(|&&c| (path(c).len(), path(c).to_string())).unwrap()
    }

    /// (resolution, ambiguous?)
    pub fn resolve(&self, target: &str, source: &str, kind: Kind) -> (Res, bool) {
        let t = target.trim();
        if t.is_empty() { return (Res::SameNote, false); }
        let src_folder = folder(source);
        let rel = if t.starts_with("./") || t.starts_with("../") || (kind == Kind::Md && !t.starts_with('/')) {
            normalize(&format!("{src_folder}/{t}"))
        } else { normalize(t) };
        let lc = rel.to_lowercase();
        let root_lc = normalize(t).to_lowercase();
        // exact path (relative-resolved first for ./ ../ and markdown links, then from the vault root), note then attachment
        for key in [lc.as_str(), root_lc.as_str()] {
            if let Some(&n) = self.note_path.get(strip_md(key)) { return (Res::Note(n), false); }
            if let Some(&f) = self.file_path.get(key) { return (Res::File(f), false); }
        }
        // path relative to the source folder (wikilinks written relative without ./)
        let sf = normalize(&format!("{src_folder}/{t}")).to_lowercase();
        if let Some(&n) = self.note_path.get(strip_md(&sf)) { return (Res::Note(n), false); }
        if let Some(&f) = self.file_path.get(&sf) { return (Res::File(f), false); }
        let name = root_lc.rsplit('/').next().unwrap_or(&root_lc).to_string();
        if root_lc.contains('/') {
            // a partial path: notes whose path ends with it
            let suffix = format!("/{}", strip_md(&root_lc));
            let c: Vec<u32> = self.note_name.get(strip_md(&name)).map(|v| v.iter().copied().filter(|&n| format!("/{}", strip_md(&self.notes[n as usize].rel.to_lowercase())).ends_with(&suffix)).collect()).unwrap_or_default();
            if !c.is_empty() { return (Res::Note(self.pick(&c, src_folder, true)), c.len() > 1); }
        }
        if let Some(c) = self.note_name.get(strip_md(&name)) { return (Res::Note(self.pick(c, src_folder, true)), c.len() > 1); }
        if let Some(c) = self.file_name.get(&name) { return (Res::File(self.pick(c, src_folder, false)), c.len() > 1); }
        (Res::Unresolved, false)
    }

    pub fn find(&self, rel: &str) -> Option<u32> { self.note_path.get(&strip_md(rel).to_lowercase()).copied() }

    pub fn stats(&self) -> serde_json::Value {
        let links: Vec<&Link> = self.notes.iter().flat_map(|n| n.links.iter()).collect();
        let count = |f: &dyn Fn(&&Link) -> bool| links.iter().filter(|l| f(l)).count();
        serde_json::json!({
            "notes": self.notes.len(), "attachments": self.files.len(), "links": links.len(),
            "to_notes": count(&|l| matches!(l.res, Res::Note(_))), "to_attachments": count(&|l| matches!(l.res, Res::File(_))),
            "same_note": count(&|l| l.res == Res::SameNote), "unresolved": count(&|l| l.res == Res::Unresolved),
            "ambiguous": count(&|l| l.ambiguous), "heading_missing": count(&|l| l.heading_found == Some(false)),
            "by_kind": {"wiki": count(&|l| l.kind == Kind::Wiki), "embed": count(&|l| l.kind == Kind::Embed), "md": count(&|l| l.kind == Kind::Md)},
            "notes_with_backlinks": self.backlinks.iter().filter(|b| !b.is_empty()).count(),
            "index_ms": (self.index_ms * 10.0).round() / 10.0,
        })
    }

    pub fn target_name(&self, r: Res) -> String {
        match r { Res::Note(n) => self.notes[n as usize].rel.clone(), Res::File(f) => self.files[f as usize].clone(), Res::SameNote => "SAME".into(), Res::Unresolved => "UNRESOLVED".into() }
    }
}
