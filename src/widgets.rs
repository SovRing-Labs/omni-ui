// SPDX-License-Identifier: MIT OR Apache-2.0
//! Packed widget tree — the app's UI as flat columns (struct of arrays), the same shape as the engine's packed DOM.
//! Each widget is kind (1 B) + verb (1 B) + parent (4 B) + label span (8 B) + value span (8 B) = 22 B; every string lives in
//! one arena. The tree is the single source for both outputs: markup for the engine (what is drawn) and the AT-SPI tree
//! (what a screen reader reads) — so the two can never disagree about what is on screen.

pub const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Kind { Window, Heading, Label, Status, TextInput, Button, List, ListItem, Link, Paragraph, Span, Code }

impl Kind {
    pub fn interactive(self) -> bool { matches!(self, Kind::TextInput | Kind::Button | Kind::Link) }
}

/// The verbs a widget can carry = access.bus VERBS (projects/access-pack/access/bus.py), so a widget activation and a
/// spoken / typed / switched intent are the same Intent.
pub const VERBS: [&str; 21] = ["find", "read", "speak", "hear", "type", "keep", "count", "translate", "stop", "repeat", "louder",
    "quieter", "bigger", "smaller", "next", "back", "select", "accept", "revert", "dismiss", "open"];
pub const NO_VERB: u8 = 255;

pub fn verb_id(v: &str) -> u8 { VERBS.iter().position(|x| *x == v).map(|i| i as u8).unwrap_or(NO_VERB) }

#[derive(Default)]
pub struct Tree {
    pub kind: Vec<Kind>,
    pub verb: Vec<u8>,
    pub parent: Vec<u32>,
    pub label: Vec<(u32, u32)>,
    pub value: Vec<(u32, u32)>,
    pub arena: String,
}

impl Tree {
    fn put(&mut self, s: &str) -> (u32, u32) { let o = self.arena.len() as u32; self.arena.push_str(s); (o, s.len() as u32) }

    pub fn add(&mut self, kind: Kind, parent: u32, label: &str, value: &str, verb: &str) -> u32 {
        let (l, v) = (self.put(label), self.put(value));
        self.kind.push(kind); self.verb.push(verb_id(verb)); self.parent.push(parent); self.label.push(l); self.value.push(v);
        (self.kind.len() - 1) as u32
    }
    pub fn len(&self) -> usize { self.kind.len() }
    pub fn label(&self, i: u32) -> &str { let (o, n) = self.label[i as usize]; &self.arena[o as usize..(o + n) as usize] }
    pub fn value(&self, i: u32) -> &str { let (o, n) = self.value[i as usize]; &self.arena[o as usize..(o + n) as usize] }
    pub fn verb(&self, i: u32) -> Option<&'static str> { VERBS.get(self.verb[i as usize] as usize).copied() }
    pub fn children(&self, i: u32) -> impl Iterator<Item = u32> + '_ { (0..self.len() as u32).filter(move |&c| self.parent[c as usize] == i) }
    /// Interactive widgets in document order: the Tab ring.
    pub fn focusables(&self) -> Vec<u32> { (0..self.len() as u32).filter(|&i| self.kind[i as usize].interactive()).collect() }
    pub fn bytes(&self) -> usize { self.len() * 22 + self.arena.len() }

    /// Markup for the engine: every widget becomes one element `id="w<i>"` (the frame maps engine boxes back by that id).
    pub fn markup(&self, focus: u32, css: &str, title: &str, brk: Option<&dyn Fn(&str, Kind, u8) -> String>) -> String {
        let mut s = format!("<!doctype html><html><head><title>{}</title><style>{}</style></head>", esc(title), css);
        self.emit(0, focus, brk, &mut s);
        s.push_str("</html>");
        s
    }

    /// Markup of some widgets (each with its subtree) — an app placing part of its tree inside its own layout (Orbit's reader).
    pub fn markup_of(&self, ids: &[u32], focus: u32) -> String { let mut s = String::new(); for &i in ids { self.emit(i, focus, None, &mut s); } s }

    fn emit(&self, i: u32, focus: u32, brk: Option<&dyn Fn(&str, Kind, u8) -> String>, s: &mut String) {
        let f = if i == focus { " focus" } else { "" };
        let kind = self.kind[i as usize];
        let level = if kind == Kind::Heading { self.value(i).parse::<u8>().unwrap_or(1).clamp(1, 6) } else { 0 };
        // drawing text: presentation soft breaks (fit.rs) when the app reflows; the tree / AT keep the original
        let txt = |i: u32| -> String { match brk { Some(b) => b(self.label(i), kind, level), None => self.label(i).to_string() } };
        let (open, close) = match self.kind[i as usize] {
            Kind::Window => (format!("<body id=w{i}>"), "</body>"),
            // heading level 1-6 rides in the value span (empty = 1)
            Kind::Heading => { return self.wrap(i, focus, brk, s, &format!("<h{level} id=w{i}>{}", esc(&txt(i))), &format!("</h{level}>")); }
            Kind::Span => (format!("<span id=w{i}>{}", esc(&txt(i))), "</span>"),
            Kind::Code => (format!("<pre id=w{i} class=code>{}", esc(&txt(i))), "</pre>"),
            Kind::Label => (format!("<p id=w{i}>{}", esc(&txt(i))), "</p>"),
            Kind::Status => (format!("<p id=w{i} class=status>{}", esc(&txt(i))), "</p>"),
            Kind::Paragraph => (format!("<p id=w{i} class=para>{}", esc(&txt(i))), "</p>"),
            Kind::TextInput => {
                let v = self.value(i);
                let body = if v.is_empty() { format!("<span class=ph>{}</span>", esc(self.label(i))) } else { esc(v) };
                (format!("<div id=w{i} class=\"field{f}\">{body}{}", if f.is_empty() { "" } else { "<span class=caret></span>" }), "</div>")
            }
            Kind::Button => (format!("<div id=w{i} class=\"btn{f}\">{}", esc(self.label(i))), "</div>"),
            Kind::List => (format!("<ol id=w{i}>"), "</ol>"),
            // the number is drawn by OmniUI: the engine's ol markers have no counter yet (layout.rs marker() writes "1.")
            Kind::ListItem => {
                let k = self.children(self.parent[i as usize]).position(|c| c == i).unwrap_or(0) + 1;
                (format!("<li id=w{i}><span class=n>{k}.</span>"), "</li>")
            }
            Kind::Link => (format!("<span id=w{i} class=\"link{f}\">{}", esc(&txt(i))), "</span>"), // inline: a <div> would close an enclosing <p>
        };
        s.push_str(&open);
        for c in self.children(i) { self.emit(c, focus, brk, s); }
        s.push_str(close);
    }

    fn wrap(&self, i: u32, focus: u32, brk: Option<&dyn Fn(&str, Kind, u8) -> String>, s: &mut String, open: &str, close: &str) {
        s.push_str(open);
        for c in self.children(i) { self.emit(c, focus, brk, s); }
        s.push_str(close);
    }
}

pub fn esc(t: &str) -> String { t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }
