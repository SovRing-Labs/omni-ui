// SPDX-License-Identifier: MIT OR Apache-2.0
//! The widget tree as an AccessKit tree -> AT-SPI (accesskit_unix) -> orca and every other assistive technology.
//! Node id = widget index + 1. Roles, names, values and bounds come from the same packed tree the engine draws.
//! A page under the surface (the Web app) joins as one Document node: its rows (pageproc `W`: HTML-AAM roles, accname,
//! states, page boxes) get ids PAGE + 1 + row, bounds moved into window coordinates (surface top, page scroll).
use crate::frame::Frame;
use crate::widgets::{Kind, Tree};
use accesskit::{Action, Live, Node, NodeId, Rect, Role, Toggled, TreeId, TreeInfo, TreeUpdate};
use serde_json::Value;

pub fn id(w: u32) -> NodeId { NodeId(w as u64 + 1) }
pub fn widget(id: NodeId) -> u32 { (id.0 - 1) as u32 }
/// The page's Document node; page row k is PAGE + 1 + k.
pub const PAGE: u64 = 1 << 40;
pub fn page_row(id: NodeId) -> Option<usize> { if id.0 > PAGE { Some((id.0 - PAGE - 1) as usize) } else { None } }

pub fn role(k: Kind) -> Role {
    match k {
        Kind::Window => Role::Window,
        Kind::Heading => Role::Heading,
        Kind::Label | Kind::Status => Role::Label,
        Kind::TextInput => Role::SearchInput,
        Kind::Button => Role::Button,
        Kind::List => Role::List,
        Kind::ListItem => Role::ListItem,
        Kind::Link => Role::Link,
        Kind::Paragraph => Role::Paragraph,
        Kind::Span => Role::Label,
        Kind::Code => Role::Code,
    }
}

/// WAI-ARIA role names (pageproc a11y_tree) → AccessKit roles (Core-AAM mappings).
fn page_role(r: &str, extra: &Value) -> Role {
    match r {
        "link" => Role::Link, "heading" => Role::Heading, "paragraph" => Role::Paragraph, "list" => Role::List, "listitem" => Role::ListItem,
        "image" => Role::Image, "button" => Role::Button, "checkbox" => Role::CheckBox, "radio" => Role::RadioButton, "combobox" => Role::ComboBox,
        "textbox" if extra["protected"] == true => Role::PasswordInput, "textbox" if extra["multiline"] == true => Role::MultilineTextInput,
        "textbox" => Role::TextInput, "searchbox" => Role::SearchInput, "slider" => Role::Slider,
        "navigation" => Role::Navigation, "main" => Role::Main, "banner" => Role::Banner, "contentinfo" => Role::ContentInfo,
        "complementary" => Role::Complementary, "search" => Role::Search, "form" => Role::Form, "region" => Role::Region, "article" => Role::Article,
        "table" => Role::Table, "row" => Role::Row, "cell" => Role::Cell, "columnheader" => Role::ColumnHeader, "rowheader" => Role::RowHeader,
        "caption" => Role::Caption, "figure" => Role::Figure, "blockquote" => Role::Blockquote, "code" => Role::Code, "dialog" => Role::Dialog,
        "group" => Role::Group, "separator" => Role::Splitter, "tab" => Role::Tab, "tablist" => Role::TabList, "tabpanel" => Role::TabPanel,
        "menu" => Role::Menu, "menuitem" => Role::MenuItem, "status" => Role::Status, "label" => Role::Label, _ => Role::Label,
    }
}

/// The page part of an update: (rows, page scroll px, focused row).
pub type Page<'a> = (&'a [Value], i32, Option<usize>);

/// Bounds are window coordinates: page boxes minus the scroll offset.
pub fn tree_update(t: &Tree, f: &Frame, focus: u32, page: Option<Page>) -> TreeUpdate {
    let mut nodes = Vec::with_capacity(t.len() + page.map(|p| p.0.len() + 1).unwrap_or(0));
    for i in 0..t.len() as u32 {
        let k = t.kind[i as usize];
        let mut n = Node::new(role(k));
        let label = t.label(i);
        // static text (Role::Label) takes its AT-SPI name from the value (accesskit label_comes_from_value)
        if !label.is_empty() { if matches!(k, Kind::Label | Kind::Status | Kind::Span) { n.set_value(label); } else { n.set_label(label); } }
        match k {
            Kind::Heading => n.set_level(t.value(i).parse::<usize>().unwrap_or(1).clamp(1, 6)),
            Kind::TextInput => { n.set_value(t.value(i)); n.add_action(Action::Focus); n.add_action(Action::SetValue); }
            Kind::Button | Kind::Link => { n.add_action(Action::Click); n.add_action(Action::Focus); }
            // the status line is a polite live region: page loads, find counts and errors are announced (ARIA role=status)
            Kind::Status => n.set_live(Live::Polite),
            _ => {}
        }
        if let Some(b) = f.boxes.get(&i) {
            let y = (b[1] - f.scroll) as f64;
            n.set_bounds(Rect { x0: b[0] as f64, y0: y, x1: (b[0] + b[2]) as f64, y1: y + b[3] as f64 });
        }
        let mut kids: Vec<NodeId> = t.children(i).map(id).collect();
        if i == 0 && page.is_some() { kids.push(NodeId(PAGE)); }
        if !kids.is_empty() { n.set_children(kids); }
        nodes.push((id(i), n));
    }
    let mut focus_id = id(focus);
    if let Some((rows, scroll, pfocus)) = page {
        let top = f.surface_top as f64;
        let mut kids: Vec<Vec<NodeId>> = vec![vec![]; rows.len()];
        let mut doc_kids = vec![];
        for (k, r) in rows.iter().enumerate() {
            let p = r[2].as_i64().unwrap_or(-1);
            let me = NodeId(PAGE + 1 + k as u64);
            if p >= 0 && (p as usize) < rows.len() { kids[p as usize].push(me); } else { doc_kids.push(me); }
        }
        let mut doc = Node::new(Role::Document);
        doc.set_label(t.label(0));
        doc.set_bounds(Rect { x0: 0.0, y0: top, x1: f.w as f64, y1: f.h as f64 });
        doc.set_children(doc_kids);
        nodes.push((NodeId(PAGE), doc));
        for (k, r) in rows.iter().enumerate() {
            let x = &r[7];
            let rl = r[0].as_str().unwrap_or("text");
            let mut n = Node::new(page_role(rl, x));
            let name = r[1].as_str().unwrap_or("");
            if rl == "text" { n.set_value(name); } else if !name.is_empty() { n.set_label(name); }
            if let Some(l) = x["level"].as_u64() { n.set_level(l as usize); }
            if let Some(h) = x["href"].as_str() { n.set_url(h); }
            if let Some(v) = x["value"].as_str() { n.set_value(v); }
            if let Some(p) = x["placeholder"].as_str() { n.set_placeholder(p); }
            if let Some(c) = x["checked"].as_bool() { n.set_toggled(if c { Toggled::True } else { Toggled::False }); }
            if let Some(e) = x["expanded"].as_bool() { n.set_expanded(e); }
            if x["disabled"] == true { n.set_disabled(); }
            if x["required"] == true { n.set_required(); }
            if x["focusable"] == true { n.add_action(Action::Focus); n.add_action(Action::Click); }
            if matches!(rl, "textbox" | "searchbox") { n.add_action(Action::SetValue); }
            let (bx, by, bw, bh) = (r[3].as_f64().unwrap_or(0.0), r[4].as_f64().unwrap_or(0.0), r[5].as_f64().unwrap_or(0.0), r[6].as_f64().unwrap_or(0.0));
            let y0 = by - scroll as f64 + top;
            n.set_bounds(Rect { x0: bx, y0, x1: bx + bw, y1: y0 + bh });
            if !kids[k].is_empty() { n.set_children(std::mem::take(&mut kids[k])); }
            nodes.push((NodeId(PAGE + 1 + k as u64), n));
        }
        if let Some(k) = pfocus { if k < rows.len() { focus_id = NodeId(PAGE + 1 + k as u64); } }
    }
    let mut info = TreeInfo::new(id(0));

    info.toolkit_name = Some("OmniUI".into());
    info.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
    TreeUpdate { nodes, tree: Some(info), tree_id: TreeId::ROOT, focus: focus_id }
}
