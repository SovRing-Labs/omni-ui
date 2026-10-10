// SPDX-License-Identifier: LicenseRef-PolyForm-Small-Business-1.0.0 OR LicenseRef-PolyForm-Noncommercial-1.0.0
//! T25 — the 25-ring system rendered into JIT as a live core, with the T25 omniring on top (author 2026-10-06: "the 25T system rendered
//! into JIT — this is the core … on top of the 25T rendered core is a 25T ring system"). Vocabulary (VOCAB.md): render = code
//! JIT-generated into memory; the core = the state space + its motions + its codebooks + its source loop, firing. Lineage:
//! CORTEX-2 (the rendered core: program → JIT sift → fires every tick), the design review §1 (T⁵ rendered core inside the T¹⁰
//! ring system), omniring (residue HDC: enc(x) over coprime rings, tick = every ring steps, per-ring sift + CRT), 15t.txt (the
//! rendered core = the browser render loop).
//!
//! The RING SYSTEM (omniring on T25): 25 coprime rings 3…101 (P25 ≈ 1.16·10³⁸, one u128). Each ring m has a random rotation row
//! r_m ∈ {0…m−1}^D; its codebook = m phase rows, codeword j = 2π·j·r_m/m quantised to 16 phases (Σm = 1,060 rows, the lattice).
//! A state x is ONE phase vector: the bundle of its 25 ring codewords (each ring at x mod m).
//! The CORE (rendered, firing every tick inside this window's render loop):
//!   TICK   — rendered x86 (AVX2): all 25 rings step at once (residues + step mod m, the moduli in a constant row)
//!   BUNDLE — the state vector from the 25 codewords
//!   SIFT   — rendered x86 (our emitter, JIT-2 form): the vector against all 1,060 codewords, one pass
//!   DECODE — per ring the best codeword → 25 residues; margin per ring
//!   CRT    — Garner over all 25 rings → the u128 state
//!   CHECK  — decoded state == the state the core holds (exact)
use crate::app::{App, Log};
use crate::jit::{sift, x86};
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::time::Instant;

pub const MODS: [u8; 25] = [3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97, 101];
const T5: [u8; 5] = [3, 5, 7, 13, 17];
const D: usize = 2048;
const OPS: [&str; 6] = ["TICK", "BUNDLE", "SIFT", "DECODE", "CRT", "CHECK"];

fn splitmix(s: &mut u64) -> u64 { *s = s.wrapping_add(0x9E3779B97F4A7C15); let mut z = *s; z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9); z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB); z ^ (z >> 31) }
fn inv(a: u128, m: u128) -> u128 { (1..m).find(|&x| a * x % m == 1).unwrap_or(0) }
/// Which tier a ring first belongs to: T5 ⊂ T10 ⊂ T15 ⊂ T20 ⊂ T25 (the nesting of our ring systems).
fn tier(m: u8) -> usize { if T5.contains(&m) { 0 } else if m <= 31 { 1 } else if m <= 53 { 2 } else if m <= 73 { 3 } else { 4 } }
const COL: [(u32, u32, u32); 5] = [(245, 166, 35), (46, 196, 214), (167, 139, 250), (74, 222, 128), (244, 114, 182)];

pub struct T25App {
    lat: Vec<u8>,                // 1,060 × D phase indices (the codebooks)
    row0: Vec<usize>,            // first lattice row of each ring
    cos16: [f32; 16], sin16: [f32; 16],
    sift: sift::Sift,            // the rendered sift
    tick: x86::Exec, tick_code: Vec<u8>,
    consts: [u8; 64],            // step residues | moduli (the tick kernel's constant rows)
    res: [u8; 32],               // the state, as 25 residues
    x: u128, step: u128, p25: u128,
    scores: Vec<i32>, qr: Vec<i8>, qi: Vec<i8>, dec: [u8; 25], margin: [f32; 25],
    ticks: u64, exact: u64, frames: u64, tps: f64, sift_us: f64, tick_ns: f64, last_x: u128, last_dec: u128,
    focus: usize, log: Log, paused: bool,
    tick_bad: u64, dec_bad: [u64; 25],
}

impl T25App {
    pub fn new() -> T25App {
        let mut s = 0x7525_u64;
        let rows: usize = MODS.iter().map(|&m| m as usize).sum();
        let mut lat = vec![0u8; rows * D];
        let mut row0 = vec![];
        let mut r = 0;
        for &m in &MODS {
            row0.push(r);
            // codeword j of ring m, per dimension: base phase b (random, so residue 0 is not a constant vector that every ring at 0
            // would leak into) + j·r/m of a turn, r ∈ 1…m−1 (no dimension wasted on r = 0); 16 phase steps
            let rot: Vec<u64> = (0..D).map(|_| 1 + splitmix(&mut s) % (m as u64 - 1)).collect();
            let base: Vec<u64> = (0..D).map(|_| splitmix(&mut s) % 16).collect();
            for j in 0..m as u64 { for d in 0..D { lat[(r + j as usize) * D + d] = ((base[d] + ((((j * rot[d]) % m as u64) * 16 + m as u64 / 2) / m as u64)) % 16) as u8; } }
            r += m as usize;
        }
        let (mut cos16, mut sin16) = ([0f32; 16], [0f32; 16]);
        for k in 0..16 { let a = std::f32::consts::TAU * k as f32 / 16.0; cos16[k] = a.cos(); sin16[k] = a.sin(); }
        // render the TICK kernel: res = res + step − m; where negative, + m (all 25 rings in one AVX2 row)
        use x86::*;
        let mut a = Asm::default();
        a.vmovdqu_ld(0, RSI, 0); a.vmovdqu_ld(1, RDI, 0); a.vmovdqu_ld(2, RDI, 32);
        // byte-lane AVX2 (VEX.256.66.0F FC/F8/64 — the pinned engine's emitter predates these encodings)
        let v66 = |a: &mut Asm, op: u8, d: u8, s: u8, m: u8| { a.b.push(0xC5); a.b.push((((d >> 3) ^ 1) << 7) | ((!s & 0xF) << 3) | (1 << 2) | 1); a.b.push(op); a.b.push(0xC0 | ((d & 7) << 3) | (m & 7)); };
        v66(&mut a, 0xFC, 0, 0, 1); v66(&mut a, 0xF8, 0, 0, 2); a.vpxor(3, 3, 3); v66(&mut a, 0x64, 4, 3, 0); a.vpand(4, 4, 2); v66(&mut a, 0xFC, 0, 0, 4);
        a.vmovdqu_st(RSI, 0, 0); a.vzeroupper(); a.ret();
        let tick = Exec::new(&a.b);
        let p25: u128 = MODS.iter().map(|&m| m as u128).product();
        let step: u128 = 0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C834 % p25; // golden-ratio stride: every ring turns at its own rate
        let mut consts = [0u8; 64];
        for (k, &m) in MODS.iter().enumerate() { consts[k] = (step % m as u128) as u8; consts[32 + k] = m; }
        let mut app = T25App { lat, row0, cos16, sin16, sift: sift::render(D), tick, tick_code: a.b, consts, res: [0; 32], x: 0, step, p25,
            scores: vec![0; rows], qr: vec![0; D], qi: vec![0; D], dec: [0; 25], margin: [0.0; 25], ticks: 0, exact: 0, frames: 0, tps: 0.0, sift_us: 0.0, tick_ns: 0.0,
            last_x: 0, last_dec: 0, focus: 0, log: Log::open(), paused: false, tick_bad: 0, dec_bad: [0; 25] };
        app.x = 1_234_567_890_123_456_789_012_345_678_901 % p25;
        for (k, &m) in MODS.iter().enumerate() { app.res[k] = (app.x % m as u128) as u8; }
        app
    }

    /// One firing of the core: TICK → BUNDLE → SIFT → DECODE → CRT → CHECK.
    fn fire(&mut self) {
        let t0 = Instant::now();
        (self.tick.f2())(self.consts.as_ptr(), self.res.as_mut_ptr());                     // TICK (rendered)
        self.x = (self.x + self.step) % self.p25;
        let tk = t0.elapsed().as_secs_f64();
        let (mut qr, mut qi) = (vec![0i8; D], vec![0i8; D]);                              // BUNDLE
        for d in 0..D {
            let (mut sr, mut si) = (0f32, 0f32);
            for k in 0..25 { let p = self.lat[(self.row0[k] + self.res[k] as usize) * D + d] as usize; sr += self.cos16[p]; si += self.sin16[p]; }
            // keep the bundle's AMPLITUDE (not only its phase): each ring's codeword then correlates at D against the other 24
            // rings' cross terms (≈ √(12·D)) — the per-ring margin a phase-only query lost (7 of 20 exact at D 8192)
            qr[d] = (sr * 16.0).round().clamp(-127.0, 127.0) as i8; qi[d] = (si * 16.0).round().clamp(-127.0, 127.0) as i8;
        }
        let ts = Instant::now();
        (self.sift.exec.f6())(self.lat.as_ptr(), self.scores.len(), qr.as_ptr(), qi.as_ptr(), self.scores.as_mut_ptr(), self.sift.consts.b.as_ptr()); // SIFT (rendered)
        self.sift_us = self.sift_us * 0.95 + ts.elapsed().as_secs_f64() * 1e6 * 0.05;
        if std::env::var("T25_DEBUG").is_ok() && self.ticks < 6 {
            let c8: Vec<i32> = (0..16).map(|k| (127.0 * self.cos16[k]).round() as i32).collect();
            let s8: Vec<i32> = (0..16).map(|k| (127.0 * self.sin16[k]).round() as i32).collect();
            let mut bad = 0; let mut ex = vec![];
            for r in 0..self.scores.len() { let mut v = 0i32; for d in 0..D { let p = self.lat[r * D + d] as usize; v += c8[p] * qr[d] as i32 + s8[p] * qi[d] as i32; } if v != self.scores[r] { bad += 1; if ex.len() < 3 { ex.push((r, v, self.scores[r])); } } }
            let tr = self.row0[0] + self.res[0] as usize;
            eprintln!("tick {} kernel-vs-scalar mismatches {bad} {:?} · ring3 true row {} score {} · row scores {:?}", self.ticks, ex, tr, self.scores[tr], &self.scores[0..3]);
        }
        for (k, &m) in MODS.iter().enumerate() {                                            // DECODE
            let s = &self.scores[self.row0[k]..self.row0[k] + m as usize];
            let (mut b, mut b2, mut bi) = (i32::MIN, i32::MIN, 0);
            for (j, &v) in s.iter().enumerate() { if v > b { b2 = b; b = v; bi = j; } else if v > b2 { b2 = v; } }
            self.dec[k] = bi as u8; self.margin[k] = if b > 0 { (b - b2) as f32 / b as f32 } else { 0.0 };
        }
        let (mut xd, mut big) = (0u128, 1u128);                                              // CRT (Garner, u128)
        for (k, &m) in MODS.iter().enumerate() { let m = m as u128; let t = ((self.dec[k] as u128 + m - xd % m) % m) * inv(big % m, m) % m; xd += big * t; big *= m; }
        self.qr = qr; self.qi = qi;
        for (k, &m) in MODS.iter().enumerate() { if self.res[k] as u128 != self.x % m as u128 { self.tick_bad += 1; } if self.dec[k] != self.res[k] { self.dec_bad[k] += 1; } }
        self.ticks += 1;                                                                     // CHECK
        if xd == self.x { self.exact += 1; }
        self.last_x = self.x; self.last_dec = xd;
        self.tick_ns = self.tick_ns * 0.95 + tk * 1e9 * 0.05;
    }

    fn put(px: &mut [u32], w: usize, h: usize, x: i32, y: i32, s: i32, c: u32) {
        for yy in y.max(0)..(y + s).min(h as i32) { for xx in x.max(0)..(x + s).min(w as i32) { px[yy as usize * w + xx as usize] = c; } }
    }
    fn rgb(c: (u32, u32, u32), f: f32) -> u32 { let f = f.clamp(0.0, 1.0); let g = |v: u32| ((11.0 + (v as f32 - 11.0) * f) as u32).min(255); (g(c.0) << 16) | (g(c.1) << 8) | g(c.2) }
}

impl App for T25App {
    fn title(&self) -> String { "T25 — rendered core + ring system (live)".into() }
    fn css(&self) -> String {
        "body{margin:0;padding:10px 16px 4px 16px;background:#0b0b10;color:#d8d8e0;font-family:DejaVu Sans Mono,monospace;font-size:14px}\
         h1{font-size:19px;margin:0 0 4px 0;color:#ffffff}.status{margin:2px 0;color:#d8d8e0}".into()
    }
    fn focus_ord(&self) -> usize { self.focus }
    fn set_focus_ord(&mut self, i: usize) { self.focus = i; }
    fn tree(&self) -> Tree {
        let mut t = Tree::default();
        let win = t.add(Kind::Window, NONE, &self.title(), "", "");
        t.add(Kind::Heading, win, "T²⁵ rendered core (JIT, firing in this render loop) · T²⁵ omniring on top", "", "");
        let acc = if self.ticks > 0 { self.exact as f64 / self.ticks as f64 * 100.0 } else { 0.0 };
        t.add(Kind::Status, win, &format!("fired {} ticks · {:.0} ticks/s · SIFT (rendered x86, {} B) {:.0} µs over {} codewords × D {} · TICK (rendered x86, {} B) {:.0} ns · exact {:.2} %",
            self.ticks, self.tps, self.sift.exec.code_bytes, self.sift_us, self.scores.len(), D, self.tick_code.len(), self.tick_ns, acc), "", "");
        t.add(Kind::Status, win, &format!("state x  = {:>39}   (P25 = {})", self.last_x, self.p25), "", "");
        t.add(Kind::Status, win, &format!("decoded  = {:>39}   {}", self.last_dec, if self.last_dec == self.last_x { "= x ✓" } else { "≠ x" }), "", "");
        t.add(Kind::Status, win, &format!("program: {} · each tick every ring steps by x+{} (mod m) · space = pause", OPS.join(" → "), self.step % 1_000_000), "", "");
        t
    }
    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value {
        if verb == "pause" { self.paused = !self.paused; }
        self.log.record(verb, arg, source, json!({}))
    }
    fn app_key(&mut self, _t: &Tree, key: &str, _text: Option<&str>) -> Option<Value> { if key == "Space" || key == " " { Some(self.act("pause", "", "key")) } else { None } }
    fn state(&self) -> Value { json!({"ticks": self.ticks, "exact": self.exact, "tps": self.tps, "sift_us": self.sift_us, "x": self.last_x.to_string(), "decoded": self.last_dec.to_string(), "tick_bad": self.tick_bad, "dec_bad": self.dec_bad.to_vec()}) }
    fn live(&self) -> bool { !self.paused }
    fn surface(&mut self, w: usize, h: usize) -> Option<Vec<u32>> {
        // fire the core for this frame's budget, then draw the ring system from what it just computed
        let t0 = Instant::now(); let n0 = self.ticks;
        if !self.paused { while t0.elapsed().as_secs_f64() < 0.010 { self.fire(); } }
        let dt = t0.elapsed().as_secs_f64();
        if dt > 0.0 && self.ticks > n0 { self.tps = self.tps * 0.8 + (self.ticks - n0) as f64 / dt * 0.2; }
        self.frames += 1;
        let mut px = vec![0x000b0b10u32; w * h];
        let (cx, cy) = ((w as f32 * 0.5) as i32, (h as f32 * 0.5) as i32);
        let rmax = (h.min(w) as f32 * 0.5 - 8.0).max(60.0);
        let r0 = rmax * 0.30; let dr = (rmax - r0) / 25.0;
        // rings ordered by the nesting T5 ⊂ T10 ⊂ T15 ⊂ T20 ⊂ T25, inner to outer
        let mut order: Vec<usize> = (0..25).collect(); order.sort_by_key(|&k| (tier(MODS[k]), MODS[k]));
        for (ri, &k) in order.iter().enumerate() {
            let m = MODS[k] as usize; let rad = r0 + dr * (ri as f32 + 0.5); let c = COL[tier(MODS[k])];
            let s = &self.scores[self.row0[k]..self.row0[k] + m];
            let (lo, hi) = (*s.iter().min().unwrap() as f32, *s.iter().max().unwrap() as f32);
            for j in 0..m {
                let a = std::f32::consts::TAU * j as f32 / m as f32 - std::f32::consts::FRAC_PI_2;
                let (x, y) = (cx + (rad * a.cos()) as i32, cy + (rad * a.sin()) as i32);
                let v = if hi > lo { (s[j] as f32 - lo) / (hi - lo) } else { 0.0 };
                if j == self.dec[k] as usize { Self::put(&mut px, w, h, x - 3, y - 3, 7, 0xffffff); Self::put(&mut px, w, h, x - 2, y - 2, 5, Self::rgb(c, 1.0)); }
                else { Self::put(&mut px, w, h, x - 1, y - 1, 3, Self::rgb(c, 0.15 + 0.85 * v * v)); }
            }
        }
        // the core: its rendered machine code, byte by byte (SIFT kernel then TICK kernel), the stage that fires lit in turn
        let code: Vec<u8> = unsafe { std::slice::from_raw_parts(self.sift.exec.ptr(), self.sift.exec.code_bytes) }.iter().chain(self.tick_code.iter()).copied().collect();
        let cell = 3i32; let side = ((r0 * 1.35) as i32 / cell).max(8);
        let mut i = 0usize;
        'grid: for gy in 0..side { for gx in 0..side {
            let (x, y) = (cx - side * cell / 2 + gx * cell, cy - side * cell / 2 + gy * cell);
            let (dx, dy) = ((x - cx) as f32, (y - cy) as f32);
            if (dx * dx + dy * dy).sqrt() > r0 * 0.92 { continue; }
            if i >= code.len() { break 'grid; }
            let b = code[i];
            let is_tick = i >= self.sift.exec.code_bytes;
            let base = if is_tick { COL[4] } else { COL[1] };
            Self::put(&mut px, w, h, x, y, cell - 1, Self::rgb(base, 0.2 + 0.8 * (b as f32 / 255.0)));
            i += 1;
        } }
        // the state vector itself (this tick's BUNDLE: D phases, the query the rendered SIFT just fired), on the core's rim
        for d in 0..D {
            let a = std::f32::consts::TAU * d as f32 / D as f32 - std::f32::consts::FRAC_PI_2;
            let ph = (self.qi[d] as f32).atan2(self.qr[d] as f32);
            let c = ((128.0 + 127.0 * ph.cos()) as u32, (128.0 + 127.0 * (ph + 2.09).cos()) as u32, (128.0 + 127.0 * (ph + 4.19).cos()) as u32);
            let rr = r0 * 0.97;
            Self::put(&mut px, w, h, cx + (rr * a.cos()) as i32, cy + (rr * a.sin()) as i32, 2, Self::rgb(c, 1.0));
        }
        Some(px)
    }
}
