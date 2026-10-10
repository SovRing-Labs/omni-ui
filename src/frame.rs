// SPDX-License-Identifier: MIT OR Apache-2.0
//! One frame on the engine: markup -> packed DOM + cascade (page::load) -> integer layout -> 2-bit glyph tiles (raster)
//! -> framebuffer; plus the residue-grid hit test and each widget's box. Nothing here re-implements the engine.
use crate::{dom, font, hit, layout, page, raster, style};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

struct MemSource { html: String }
impl page::Source for MemSource {
    fn url(&self) -> String { "omni-ui://app/".into() }
    fn html(&self) -> Vec<u8> { self.html.as_bytes().to_vec() }
    fn get(&self, _url: &str) -> Option<Vec<u8>> { None }
}

/// The app's markup for a window `w` px wide, with soft breaks for over-long tokens when the app reflows text.
pub fn app_markup(app: &dyn crate::app::App, tree: &crate::widgets::Tree, focus: u32, fonts: &font::Fonts, w: usize) -> String {
    match app.fit() {
        Some((px, pad)) => {
            use crate::widgets::Kind;
            let max = w as i32 - pad;
            let b = |t: &str, kind: Kind, level: u8| match kind {
                Kind::Code => t.split('\n').map(|l| crate::fit::break_long_tokens(fonts, "monospace", 400, l, 17, max - 16)).collect::<Vec<_>>().join("\n"),
                Kind::Heading => crate::fit::break_long_tokens(fonts, "sans-serif", 700, t, px * [15, 13, 11, 10, 10, 10][(level.max(1) - 1) as usize] / 10, max),
                Kind::Status => crate::fit::break_long_tokens(fonts, "sans-serif", 400, t, 17, max),
                _ => crate::fit::break_long_tokens(fonts, "sans-serif", 400, t, px, max),
            };
            tree.markup(focus, &app.css(), &app.title(), Some(&b))
        }
        None => tree.markup(focus, &app.css(), &app.title(), None),
    }
}

pub struct Frame {
    pub w: usize,
    pub h: usize,
    /// 0x00RRGGBB, w*h (softbuffer's format)
    pub pixels: Vec<u32>,
    hit: hit::HitGrid,
    /// widget -> its box in px (x, y, w, h)
    pub boxes: HashMap<u32, [i32; 4]>,
    node_widget: Vec<u32>,
    parent: Vec<u32>,
    pub ms_style: f64,
    pub ms_layout: f64,
    pub ms_raster: f64,
    pub tiles_unique: usize,
    pub tiles_total: usize,
    pub dom_nodes: usize,
    /// vertical scroll (px) this frame was rastered at; page height and widest box edge from layout (px)
    pub scroll: i32,
    pub page_h: i32,
    pub max_right: i32,
    /// first window row of the app's pixel surface (0 = no surface)
    pub surface_top: i32,
    /// the surface fills the window under the widgets (App::overlay): a press that hits no widget goes to the surface
    pub overlay: bool,
}

/// Lay the page out at the window width, raster rows [scroll, scroll + h): the raster takes any y0, so scrolling is y0.
pub fn render(html: &str, fonts: &font::Fonts, w: usize, h: usize, scroll: i32) -> Frame { render_on(html, fonts, w, h, scroll, None) }

/// `render` over pixels already on screen (RGBA, w×h): the overlay chrome draws on the page and its glass blurs it.
pub fn render_on(html: &str, fonts: &font::Fonts, w: usize, h: usize, scroll: i32, base: Option<&[u32]>) -> Frame {
    let t0 = Instant::now();
    let l = page::load(&MemSource { html: html.to_string() }, (w as f32, h as f32));
    let t1 = Instant::now();
    let sizes = |n: u32| -> Option<(u32, u32)> {
        if l.dom.tag(n) != "svg" { return None; }
        crate::svg::intrinsic(l.dom.outer_xml(n).as_bytes()).map(|(w, h, _)| (w.round() as u32, h.round() as u32))
    };
    let mut lay = layout::Layout::new(&l.dom, &l.styled, fonts, &sizes, w as i32, h as i32);
    lay.run();
    let out = std::mem::take(&mut lay.out);
    drop(lay);
    let t2 = Instant::now();
    let page_h = out.boxes.iter().map(|(_, b)| (b[1] + b[3]) / style::FX).max().unwrap_or(0);
    // widest INK, not widest box: text that overflows its box (an unbreakable word) is only visible in the display list
    let max_right = out.items.iter().map(|it| { let (x, _, w, _) = it.bounds(); (x + w) / style::FX }).max().unwrap_or(0);
    let scroll = scroll.clamp(0, (page_h - h as i32).max(0));
    // inline <svg> (the chrome's icons and the core) drawn from its markup with the element's CSS fill / stroke / colour
    let imgf = |n: u32, m: u8, iw: i32, ih: i32| -> Option<Rc<(u32, u32, Vec<u32>)>> {
        if m != 0 || l.dom.tag(n) != "svg" { return None; }
        let st = &l.styled.table[l.styled.ids[n as usize] as usize];
        let css = crate::svg::Css { fill: st.svg_fill, stroke: st.svg_stroke, sw: st.svg_sw.map(|x| x as f32 / style::FX as f32), color: st.color };
        crate::svg::render_css(l.dom.outer_xml(n).as_bytes(), iw.max(1) as u32, ih.max(1) as u32, css).map(Rc::new)
    };
    let mut store = raster::Store::default();
    // overlay (the base is the page under the chrome): paint only where the chrome is, straight into the frame
    let (pixels, tiles_unique, tiles_total) = if let Some(bpx) = base {
        let mut o = bpx.to_vec();
        raster::paint_over(&out.items, fonts, &imgf, w, h, bpx, &mut o);
        (o.iter().map(|c| c >> 8).collect::<Vec<u32>>(), 0, 0)
    } else {
        let page = raster::render(&out.items, fonts, &imgf, w, scroll as usize, scroll as usize + h, &mut store);
        let rgba = raster::compose(&page, &store, w);
        (rgba[..w * h].iter().map(|c| c >> 8).collect(), store.tiles.len(), page.tw * page.th)
    };
    let t3 = Instant::now();

    let n = l.dom.nodes.len();
    let mut node_widget = vec![dom::NONE; n];
    for i in 0..n as u32 {
        if l.dom.nodes[i as usize].kind != dom::Kind::Element { continue; }
        if let Some(id) = l.dom.attr(i, "id") { if let Some(k) = id.strip_prefix('w').and_then(|d| d.parse::<u32>().ok()) { node_widget[i as usize] = k; } }
    }
    let mut boxes = HashMap::new();
    for (nd, b) in &out.boxes {
        let wdg = node_widget[*nd as usize];
        if wdg != dom::NONE { boxes.entry(wdg).or_insert([b[0] / style::FX, b[1] / style::FX, b[2] / style::FX, b[3] / style::FX]); }
    }
    let parent = l.dom.nodes.iter().map(|x| x.parent).collect();
    let hit = hit::HitGrid::build(&out.boxes, w as i32 * style::FX, (page_h.max(h as i32) + 1) * style::FX);
    Frame { w, h, pixels, hit, boxes, node_widget, parent, ms_style: (t1 - t0).as_secs_f64() * 1e3, ms_layout: (t2 - t1).as_secs_f64() * 1e3,
            ms_raster: (t3 - t2).as_secs_f64() * 1e3, tiles_unique, tiles_total, dom_nodes: n,
            scroll, page_h, max_right, surface_top: 0, overlay: false }
}

/// The whole window: the app's widgets, then (when the app has one) its pixel surface in the rows under the last widget.
pub fn render_app(app: &mut dyn crate::app::App, tree: &crate::widgets::Tree, focus: u32, fonts: &font::Fonts, w: usize, h: usize) -> Frame {
    let html = app.markup(tree, focus, w, h).unwrap_or_else(|| app_markup(app, tree, focus, fonts, w));
    if app.overlay() {
        let t_s = Instant::now();
        let base: Vec<u32> = match app.surface(w, h) { Some(px) if px.len() >= w * h => px[..w * h].iter().map(|c| c << 8 | 0xff).collect(), _ => vec![0x0b0e14ff; w * h] };
        let ms_surface = t_s.elapsed().as_secs_f64() * 1e3;
        let mut f = render_on(&html, fonts, w, h, 0, Some(&base));
        f.overlay = true;
        if std::env::var_os("OMNI_UI_TRACE").is_some() { eprintln!("overlay frame {w}x{h}: surface {ms_surface:.1} ms · style {:.1} · layout {:.1} · raster {:.1} ms", f.ms_style, f.ms_layout, f.ms_raster); }
        return f;
    }
    let mut f = render(&html, fonts, w, h, app.scroll());
    let top = f.boxes.iter().filter(|(k, _)| **k != 0).map(|(_, b)| b[1] + b[3]).max().unwrap_or(0) - f.scroll + 6;
    let top = top.clamp(0, h as i32 - 32);
    if let Some(px) = app.surface(w, h - top as usize) {
        let n = px.len().min(w * (h - top as usize));
        f.pixels[top as usize * w..top as usize * w + n].copy_from_slice(&px[..n]);
        f.surface_top = top;
    }
    f
}

impl Frame {
    /// The widget under (x, y) px: deepest engine box (residue-grid hit test), then up the DOM to the nearest widget.
    pub fn widget_at(&self, x: i32, y: i32) -> Option<u32> {
        let mut nd = self.hit.hit(x * style::FX, (y + self.scroll) * style::FX)?;
        loop {
            let wdg = self.node_widget[nd as usize];
            if wdg != dom::NONE { return Some(wdg); }
            nd = self.parent[nd as usize];
            if nd == dom::NONE || nd as usize >= self.parent.len() { return None; }
        }
    }

    pub fn png(&self, path: &str) -> std::io::Result<()> {
        let f = std::fs::File::create(path)?;
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), self.w as u32, self.h as u32);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().map_err(std::io::Error::other)?;
        let data: Vec<u8> = self.pixels.iter().flat_map(|c| [(c >> 16) as u8, (c >> 8) as u8, *c as u8]).collect();
        wr.write_image_data(&data).map_err(std::io::Error::other)
    }
}
