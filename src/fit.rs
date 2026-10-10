// SPDX-License-Identifier: MIT OR Apache-2.0
//! Presentation-only soft breaks: the engine breaks lines only at ASCII whitespace (a logged finding), so a token wider than
//! the line (a URL, a hash, a path) would run off the window. Split only such tokens, by the engine's own glyph advances, into
//! pieces that each fit one line. The widget tree keeps the original text — AT-SPI reads the real token; only the drawing
//! gets break points.
use crate::font::Fonts;

/// `weight`: 700 for headings — they draw bold, and bold glyphs are wider (a 2,485-char heading overflowed by 16 px measured at 400).
pub fn break_long_tokens(fonts: &Fonts, family: &str, weight: u16, text: &str, px: i32, max_w_px: i32) -> String {
    let id = fonts.resolve(family, weight, false);
    let px64 = px * 64;
    let max = max_w_px * 64;
    let mut out: Vec<String> = Vec::new();
    for tok in text.split(' ') {
        let w: i32 = tok.chars().map(|c| fonts.advance(id, c, px64).2).sum();
        if w <= max { out.push(tok.to_string()); continue; }
        let (mut piece, mut pw) = (String::new(), 0);
        for c in tok.chars() {
            let a = fonts.advance(id, c, px64).2;
            if pw + a > max && !piece.is_empty() { out.push(std::mem::take(&mut piece)); pw = 0; }
            piece.push(c); pw += a;
        }
        if !piece.is_empty() { out.push(piece); }
    }
    out.join(" ")
}
