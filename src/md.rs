// SPDX-License-Identifier: MIT OR Apache-2.0
//! Markdown → blocks, shared by every OmniUI app that shows written text (Notes today; Library / Docs next) — one parser,
//! not one per app (the launcher grew two reader codebases, 11,573 + 423 lines). Presentation-only: never writes.

/// Inline markup stripped for drawing and reading (**bold**, *em*, `code`, ==mark==, ~~strike~~).
pub fn plain(s: &str) -> String {
    s.replace("**", "").replace("__", "").replace("==", "").replace("~~", "").replace('`', "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Markdown blocks of one note (parsed once per view; a part of them is drawn).
pub enum Block { Heading(u8, String), Para(String), Item(u32, String), Code(String) }

/// A part ends after PART blocks or PART_CHARS of text: a 4.7 MB note (one 2.6 MB paragraph) was 65,955 widgets and 4.6–7.7 s
/// per frame drawn whole. Paragraphs longer than PARA_CHARS are cut at line ends.
pub const PART: usize = 400;
pub const PART_CHARS: usize = 40_000;
const PARA_CHARS: usize = 4_000;

pub fn block_len(b: &Block) -> usize { match b { Block::Heading(_, s) | Block::Para(s) | Block::Item(_, s) | Block::Code(s) => s.len() } }

/// Start index of every part.
pub fn part_starts(all: &[Block]) -> Vec<usize> {
    let (mut starts, mut n, mut chars) = (vec![0], 0usize, 0usize);
    for (i, b) in all.iter().enumerate() {
        if n > 0 && (n >= PART || chars + block_len(b) > PART_CHARS) { starts.push(i); n = 0; chars = 0; }
        n += 1; chars += block_len(b);
    }
    starts
}

pub fn blocks(text: &str) -> Vec<Block> {
    let mut out = vec![];
    let mut lines = text.lines().peekable();
    if text.starts_with("---") { lines.next(); while let Some(l) = lines.next() { if l.trim() == "---" { break; } } } // properties: not drawn in v0
    let (mut list, mut lists) = (None::<u32>, 0u32);
    let mut code: Option<String> = None;
    let mut para = String::new();
    let flush = |out: &mut Vec<Block>, para: &mut String| { if !para.trim().is_empty() { out.push(Block::Para(para.trim().to_string())); } para.clear(); };
    for line in lines {
        let s = line.trim_start();
        if let Some(c) = code.as_mut() {
            if s.starts_with("```") || s.starts_with("~~~") { out.push(Block::Code(c.trim_end().to_string())); code = None; } else { c.push_str(line); c.push('\n'); }
            continue;
        }
        if s.starts_with("```") || s.starts_with("~~~") { flush(&mut out, &mut para); list = None; code = Some(String::new()); continue; }
        if s.is_empty() { flush(&mut out, &mut para); list = None; continue; }
        // a thematic break (---, ***, ___) ends the paragraph and draws nothing (spacing comes from the blocks around it)
        let t = s.trim_end();
        if t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_')) { flush(&mut out, &mut para); list = None; continue; }
        let hashes = s.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && s[hashes..].starts_with(' ') {
            flush(&mut out, &mut para); list = None;
            let txt = s[hashes..].trim().replace("[[", "").replace("]]", "");
            out.push(Block::Heading(hashes as u8, plain(txt.split('|').last().unwrap_or(&txt))));
            continue;
        }
        // the item keeps its own marker: • for bullets, the written number for ordered lists
        let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
        let item = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")).or_else(|| s.strip_prefix("+ ")).map(|b| ("•".to_string(), b))
            .or_else(|| (digits > 0 && s[digits..].starts_with(". ")).then(|| (format!("{}.", &s[..digits]), &s[digits + 2..])));
        if let Some((marker, body)) = item {
            flush(&mut out, &mut para);
            let l = *list.get_or_insert_with(|| { lists += 1; lists });
            let body = body.strip_prefix("[ ] ").map(|b| format!("☐ {b}")).or_else(|| body.strip_prefix("[x] ").map(|b| format!("☑ {b}"))).unwrap_or_else(|| format!("{marker} {body}"));
            out.push(Block::Item(l, body));
            continue;
        }
        list = None;
        let s = s.strip_prefix("> ").unwrap_or(s);
        if para.len() + s.len() > PARA_CHARS { flush(&mut out, &mut para); } // a giant paragraph becomes several, cut at line ends
        if !para.is_empty() { para.push(' '); }
        para.push_str(s);
    }
    if let Some(c) = code { out.push(Block::Code(c.trim_end().to_string())); }
    flush(&mut out, &mut para);
    out
}


// ── Essence: the launcher's extractEssence (opencode-launcher ui/src/lib/markdownRenderer.js:99-145), ported to the byte so
// the desktop Library, the mobile Reader and OmniUI show the same essence. JS semantics kept: \s / trim() = WhiteSpace ∪
// LineTerminator (incl. U+FEFF, excl. U+0085); `.` stops at \n \r U+2028 U+2029; /^#{1,4}\s+(.*)/ (5+ hashes never match).
fn js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
fn js_trim(s: &str) -> &str { s.trim_matches(js_space) }

/// /^#{1,4}\s+(.*)/ → the captured title, or None.
fn js_header(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if !(1..=4).contains(&hashes) { return None; }
    let rest = &line[hashes..];
    let ws: usize = rest.chars().take_while(|&c| js_space(c)).map(char::len_utf8).sum();
    if ws == 0 { return None; }
    let body = &rest[ws..];
    let end = body.find(['\n', '\r', '\u{2028}', '\u{2029}']).unwrap_or(body.len());
    Some(&body[..end])
}

pub fn essence(raw: &str) -> String {
    if raw.is_empty() { return String::new(); }
    let lines: Vec<&str> = raw.split('\n').collect();
    let (mut sec, mut p, mut v, mut c) = (0u8, Vec::<&str>::new(), Vec::<&str>::new(), Vec::<&str>::new());
    for &line in &lines {
        if let Some(h) = js_header(line) {
            let t: String = h.to_lowercase().chars().filter(|&ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || js_space(ch)).collect();
            let t = js_trim(&t);
            if t.contains("problem") || t.contains("where we are") || t.contains("baseline") { sec = 1; p.push(line); continue; }
            else if t.contains("verdict") || t.contains("decision") || t.contains("recommendation") || t.contains("converge") { sec = 2; v.push(line); continue; }
            else if t.contains("critical path") || t.contains("next steps") || t.contains("immediate action") || t.contains("execution plan") || t.contains("action items") { sec = 3; c.push(line); continue; }
            else { sec = 0; }
        }
        match sec { 1 => p.push(line), 2 => v.push(line), 3 => c.push(line), _ => {} }
    }
    if !p.is_empty() || !v.is_empty() || !c.is_empty() {
        let parts = ["> **ESSENCE MODE ACTIVE** — Isolating Problem, Oracle Verdict & Critical Path\n".to_string(),
                     js_trim(&p.join("\n")).to_string(), js_trim(&v.join("\n")).to_string(), js_trim(&c.join("\n")).to_string()];
        return parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
    }
    format!("> **ESSENCE MODE ACTIVE** — Summary Slice\n\n{}", js_trim(&lines[..lines.len().min(30)].join("\n")))
}
