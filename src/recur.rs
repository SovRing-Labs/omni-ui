// SPDX-License-Identifier: MIT OR Apache-2.0
//! Recurrence on residue clocks — NEURAL-APPS-0 step 7a (Calendar). Lineage: bin/reason-gen calendar.crt (generalised CRT,
//! "never" is a real answer, independent day-walk verifier); LAWS R14 (address by residue algebra, never scan); MATH index #1,
//! #10 (two cycles meet iff gcd(p, q) divides the offset gap).
//!
//! A rule lives on one ring: the DAY ring (RFC 5545 DAILY, WEEKLY on one weekday, every-n-days: x = start + k·period days) or
//! the MONTH ring (MONTHLY / YEARLY by date: month index m = 12·year + month − 1, m = start + k·period, the date exists only
//! when the month has that day — RFC 5545 skips invalid dates and does not count them). Exact integers throughout.
//! O(1): membership (day ring; month ring when dom ≤ 28), next occurrence (day ring), first coincidence of two rules on the
//! same ring (generalised CRT). A day-ring rule against a month-ring rule has no single CRT: the month rule's occurrences are
//! walked with the O(1) day-ring membership test (bounded by the horizon).

pub const HORIZON_DAYS: i64 = 146_097; // one Gregorian cycle (400 years): month lengths repeat after it

// ── civil dates (Hinnant's days_from_civil / civil_from_days; day 0 = 1970-01-01) ──
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}
pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m { 4 | 6 | 9 | 11 => 30, 2 => if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 }, _ => 31 }
}
fn month_day(mi: i64, dom: i64) -> Option<i64> {
    let (y, m) = (mi.div_euclid(12), mi.rem_euclid(12) + 1);
    (dom <= days_in_month(y, m)).then(|| days_from_civil(y, m, dom))
}
fn month_index(day: i64) -> (i64, i64) { let (y, m, d) = civil_from_days(day); (12 * y + m - 1, d) }
/// n-th weekday `wd` (0 = Monday) of month index `mi`; n < 0 counts from the end. None when the month has no such day.
pub fn nth_weekday(mi: i64, n: i64, wd: i64) -> Option<i64> {
    let (y, m) = (mi.div_euclid(12), mi.rem_euclid(12) + 1);
    let (first, len) = (days_from_civil(y, m, 1), days_in_month(y, m));
    let d = if n > 0 {
        first + (wd - (first + 3).rem_euclid(7)).rem_euclid(7) + 7 * (n - 1)
    } else {
        let last = first + len - 1;
        last - ((last + 3).rem_euclid(7) - wd).rem_euclid(7) - 7 * (-n - 1)
    };
    (d >= first && d < first + len).then_some(d)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring { Day, Month }

#[derive(Clone, Debug)]
pub struct Rule {
    pub ring: Ring,
    /// Day ring: first day number. Month ring: first month index (12·y + m − 1).
    pub start: i64,
    /// Month ring: day of month (1..=31) — unless `nth` is set.
    pub dom: i64,
    /// Month ring: the n-th weekday of the month instead of a date (n = 1..5 from the start, -1..-5 from the end; weekday 0 = Monday).
    pub nth: Option<(i64, i64)>,
    /// Ring units between occurrences (days or months), ≥ 1.
    pub period: i64,
    pub count: Option<i64>,
    pub until: Option<i64>,
    /// Day numbers removed (RFC 5545 EXDATE), sorted.
    pub exdates: Vec<i64>,
    /// Time of day [t0, t1) in minutes.
    pub t0: i64,
    pub t1: i64,
}

fn egcd(a: i64, b: i64) -> (i64, i64, i64) { if b == 0 { (a, 1, 0) } else { let (g, x, y) = egcd(b, a % b); (g, y, x - (a / b) * y) } }

/// Smallest x ≥ lo with x ≡ r1 (mod m1) and x ≡ r2 (mod m2), and the joint period lcm; None when gcd ∤ (r2 − r1) ("never").
pub fn crt(r1: i64, m1: i64, r2: i64, m2: i64, lo: i64) -> Option<(i64, i64)> {
    let (g, s, _) = egcd(m1, m2);
    if (r2 - r1).rem_euclid(g) != 0 { return None; }
    let l = m1 / g * m2;
    let k = ((((r2 - r1) / g) as i128 * s as i128).rem_euclid((m2 / g) as i128)) as i64;
    let x0 = (r1 as i128 + m1 as i128 * k as i128).rem_euclid(l as i128) as i64;
    let x = if x0 >= lo { x0 - (x0 - lo) / l * l } else { x0 + (lo - x0 + l - 1) / l * l };
    Some((x, l))
}

impl Rule {
    /// The occurrence date in month `mi` on the month ring (a date, or the n-th weekday).
    fn month_date(&self, mi: i64) -> Option<i64> {
        match self.nth { Some((n, wd)) => nth_weekday(mi, n, wd), None => month_day(mi, self.dom) }
    }
    /// Every month of the ring has a date (dom <= 28, or n-th weekday with |n| <= 4): ordinals are O(1).
    fn always_valid(&self) -> bool { self.nth.map_or(self.dom <= 28, |(n, _)| (1..=4).contains(&n.abs())) }

    fn excluded(&self, day: i64) -> bool { self.exdates.binary_search(&day).is_ok() }
    fn within_until(&self, day: i64) -> bool { self.until.map_or(true, |u| day <= u) }

    /// Index k of an existing occurrence on its ring (month ring: invalid months skipped, as COUNT counts).
    fn ordinal(&self, k_ring: i64) -> i64 {
        if self.ring == Ring::Day || self.always_valid() { return k_ring; }
        (0..k_ring).filter(|&j| self.month_date(self.start + j * self.period).is_some()).count() as i64
    }

    /// Does the rule occur on `day`? O(1) on the day ring and for dom ≤ 28.
    pub fn occurs_on(&self, day: i64) -> bool {
        let k = match self.ring {
            Ring::Day => { let off = day - self.start; if off < 0 || off % self.period != 0 { return false; } off / self.period }
            Ring::Month => {
                let (mi, _) = month_index(day);
                let off = mi - self.start;
                if off < 0 || off % self.period != 0 || self.month_date(mi) != Some(day) { return false; }
                off / self.period
            }
        };
        self.within_until(day) && !self.excluded(day) && self.count.map_or(true, |c| self.ordinal(k) < c)
    }

    /// First occurrence on or after `day`.
    pub fn next_on_or_after(&self, day: i64) -> Option<i64> {
        match self.ring {
            Ring::Day => {
                let mut k = ((day - self.start).max(0) + self.period - 1) / self.period;
                loop {
                    let x = self.start + k * self.period;
                    if !self.within_until(x) || self.count.is_some_and(|c| k >= c) { return None; }
                    if !self.excluded(x) { return Some(x); }
                    k += 1; // exdates are finite: at most exdates.len() extra steps
                }
            }
            Ring::Month => {
                let (mi0, _) = month_index(day);
                let mut k = ((mi0 - self.start).max(0) + self.period - 1) / self.period;
                let mut ord = None;
                for _ in 0..(4_800 / self.period + 2 + self.exdates.len() as i64) * 2 {
                    let mi = self.start + k * self.period;
                    if let Some(x) = self.month_date(mi) {
                        if x >= day {
                            if !self.within_until(x) { return None; }
                            let o = *ord.get_or_insert_with(|| self.ordinal(k));
                            if self.count.is_some_and(|c| o >= c) { return None; }
                            if !self.excluded(x) { return Some(x); }
                        }
                        if let Some(o) = ord.as_mut() { *o += 1; }
                    }
                    k += 1;
                }
                None
            }
        }
    }

    fn last_possible(&self) -> i64 {
        let by_count = self.count.map(|c| match self.ring {
            Ring::Day => self.start + (c - 1).max(0) * self.period,
            Ring::Month => i64::MAX,
        });
        [self.until, by_count].into_iter().flatten().min().unwrap_or(i64::MAX)
    }
}

/// Exact quick reject for two month-ring selectors: can they ever name the same day of a month?
/// n-th weekday n > 0 falls on days 7(n-1)+1 ..= 7n; from the end (k = -n) on 22-7(k-1) ..= 31-7(k-1) (months are 28-31 days).
fn selectors_can_meet(a: &Rule, b: &Rule) -> bool {
    let range = |n: i64| if n > 0 { (7 * (n - 1) + 1, 7 * n) } else { let k = -n; (22 - 7 * (k - 1), 31 - 7 * (k - 1)) };
    match (a.nth, b.nth) {
        (None, None) => a.dom == b.dom,
        (Some((n1, w1)), Some((n2, w2))) => {
            if w1 != w2 { return false; }                                   // one day has one weekday
            if n1.signum() == n2.signum() { return n1 == n2; }              // same counting direction: same n or never
            let ((l1, h1), (l2, h2)) = (range(n1), range(n2));
            l1 <= h2 && l2 <= h1
        }
        (Some((n, _)), None) => { let (l, h) = range(n); (l..=h).contains(&b.dom) }
        (None, Some((n, _))) => { let (l, h) = range(n); (l..=h).contains(&a.dom) }
    }
}

/// First day ≥ `from` on which both rules occur (any time of day), within the horizon. Exact; None = never.
pub fn first_coincidence(a: &Rule, b: &Rule, from: i64) -> Option<i64> {
    let end = a.last_possible().min(b.last_possible()).min(from.saturating_add(HORIZON_DAYS));
    match (a.ring, b.ring) {
        (Ring::Day, Ring::Day) => {
            let (mut x, l) = crt(a.start, a.period, b.start, b.period, from.max(a.start).max(b.start))?;
            // every coincidence is x + j·l; only exdates or COUNT / UNTIL can remove one, and exdates are finite
            while x <= end {
                if a.occurs_on(x) && b.occurs_on(x) { return Some(x); }
                if (a.count.is_some() && !a.occurs_on(x) && !a.excluded(x)) || (b.count.is_some() && !b.occurs_on(x) && !b.excluded(x)) { return None; }
                x += l;
            }
            None
        }
        (Ring::Month, Ring::Month) => {
            if !selectors_can_meet(a, b) { return None; }
            let (m0, _) = month_index(from);
            // months where both recur = one CRT on the month ring; in each, both selectors must name the same day
            let (mut mi, l) = crt(a.start, a.period, b.start, b.period, m0.max(a.start).max(b.start))?;
            loop {
                let first = days_from_civil(mi.div_euclid(12), mi.rem_euclid(12) + 1, 1);
                if first > end { return None; }
                if let (Some(x), Some(y)) = (a.month_date(mi), b.month_date(mi)) {
                    if x == y && x >= from && x <= end && a.occurs_on(x) && b.occurs_on(x) { return Some(x); }
                }
                mi += l;
            }
        }
        _ => {
            // day ring × month ring: walk the month rule, test the day rule in O(1)
            let (m, d) = if a.ring == Ring::Month { (a, b) } else { (b, a) };
            // exact reject: a day rule whose period is a multiple of 7 keeps one weekday; an n-th-weekday rule keeps another
            if let Some((_, w)) = m.nth { if d.period % 7 == 0 && (d.start + 3).rem_euclid(7) != w { return None; } }
            let mut x = m.next_on_or_after(from)?;
            while x <= end {
                if d.occurs_on(x) { return Some(x); }
                x = m.next_on_or_after(x + 1)?;
            }
            None
        }
    }
}

/// First day both occur with overlapping times of day; None when the times never overlap or the days never meet.
pub fn first_conflict(a: &Rule, b: &Rule, from: i64) -> Option<i64> {
    if a.t0 >= b.t1 || b.t0 >= a.t1 { return None; }
    first_coincidence(a, b, from)
}

// ── reference: enumerate each rule's occurrences directly (no residue algebra) and intersect ──
pub fn occurrences_ref(r: &Rule, from: i64, to: i64) -> std::collections::BTreeSet<i64> {
    let mut s = std::collections::BTreeSet::new();
    let mut n = 0i64;
    let mut k = 0i64;
    loop {
        let x = match r.ring {
            Ring::Day => Some(r.start + k * r.period),
            Ring::Month => {
                let mi = r.start + k * r.period;
                match r.nth {
                    None => month_day(mi, r.dom),
                    Some((n, wd)) => {
                        let (y, m) = (mi.div_euclid(12), mi.rem_euclid(12) + 1);
                        let ds: Vec<i64> = (1..=days_in_month(y, m)).map(|d| days_from_civil(y, m, d)).filter(|&d| (d + 3).rem_euclid(7) == wd).collect();
                        if n > 0 { ds.get((n - 1) as usize).copied() } else { ds.len().checked_sub((-n) as usize).map(|i| ds[i]) }
                    }
                }
            }
        };
        k += 1;
        if let Some(x) = x {
            if x > to || r.until.is_some_and(|u| x > u) || r.count.is_some_and(|c| n >= c) { break; }
            n += 1;
            if x >= from && !r.exdates.contains(&x) { s.insert(x); }
        } else if r.start + k * r.period > 12 * 10_000 { break; }
        if r.ring == Ring::Month && month_day(r.start + k * r.period, 1).map_or(false, |d| d > to) { break; }
    }
    s
}

pub fn first_conflict_ref(a: &Rule, b: &Rule, from: i64, to: i64) -> Option<i64> {
    if a.t0 >= b.t1 || b.t0 >= a.t1 { return None; }
    let sa = occurrences_ref(a, from, to);
    occurrences_ref(b, from, to).into_iter().find(|x| sa.contains(x))
}

/// `omni-ui recur-check N`: N random rule pairs; residue answer vs the enumeration reference.
pub fn check(n: usize, seed: u64) -> serde_json::Value {
    let mut s = seed;
    let mut rnd = move |m: i64| -> i64 { s ^= s << 13; s ^= s >> 7; s ^= s << 17; (s % m as u64) as i64 };
    let base = days_from_civil(2026, 1, 1);
    let mut rules = |rnd: &mut dyn FnMut(i64) -> i64| -> Rule {
        let month = rnd(4) == 0;
        let ring = if month { Ring::Month } else { Ring::Day };
        let period = if month { [1, 1, 2, 3, 6, 12, 12, 24][rnd(8) as usize] } else { [1, 2, 3, 7, 7, 7, 14, 14, 21, 28, 5, 10, 30, 13, 17, 60][rnd(16) as usize] };
        let start = if month { 12 * 2026 + rnd(36) } else { base + rnd(400) };
        let dom = if month { [1, 5, 15, 28, 29, 30, 31, 31][rnd(8) as usize] } else { 1 };
        let nth = (month && rnd(3) == 0).then(|| ([1, 2, 3, 4, 5, -1, -2][rnd(7) as usize], rnd(7)));
        let mut r = Rule { ring, start, dom, nth, period, count: None, until: None, exdates: vec![], t0: 0, t1: 0 };
        if rnd(3) == 0 { r.count = Some(1 + rnd(60)); }
        if rnd(3) == 0 { r.until = Some(base + 200 + rnd(4000)); }
        let t0 = 60 * (7 + rnd(12)); r.t0 = t0; r.t1 = t0 + 30 * (1 + rnd(6));
        let occ: Vec<i64> = occurrences_ref(&r, base - 1000, base + 6000).into_iter().collect();
        for _ in 0..rnd(4) { if !occ.is_empty() { r.exdates.push(occ[rnd(occ.len().min(40) as i64) as usize]); } }
        r.exdates.sort(); r.exdates.dedup();
        r
    };
    let horizon = base + 12_000; // ~33 years: the reference enumerates up to here
    let (mut same, mut diff, mut found, mut never, mut fast_ns, mut ref_ns) = (0, 0, 0, 0, 0u128, 0u128);
    let mut kinds = std::collections::BTreeMap::<&str, (usize, usize, u128)>::new();
    let mut crt_pairs = vec![];
    let mut first_bad = vec![];
    for _ in 0..n {
        let a = rules(&mut rnd);
        let mut b = rules(&mut rnd);
        if rnd(2) == 0 { b.t0 = a.t0; b.t1 = a.t1; } // half the pairs share a time slot, so the DAY math decides
        let from = base + rnd(300);
        let t = std::time::Instant::now();
        let got = first_conflict(&a, &b, from).filter(|&x| x <= horizon);
        let dt = t.elapsed().as_nanos();
        fast_ns += dt;
        let t = std::time::Instant::now();
        let want = first_conflict_ref(&a, &b, from, horizon);
        ref_ns += t.elapsed().as_nanos();
        let kind = match (a.ring, b.ring) {
            (Ring::Day, Ring::Day) => "day×day",
            (Ring::Month, Ring::Month) => if a.nth.is_some() || b.nth.is_some() { "month×month (nth weekday)" } else { "month×month" },
            _ => if a.nth.is_some() || b.nth.is_some() { "day×month (nth weekday)" } else { "day×month" },
        };
        let e = kinds.entry(kind).or_default();
        e.0 += 1;
        e.2 += dt;
        if got == want { same += 1; } else { diff += 1; e.1 += 1; if first_bad.len() < 5 { first_bad.push(format!("{a:?} | {b:?} | from {from} | got {got:?} want {want:?}")); } }
        if want.is_some() { found += 1 } else { never += 1 }
        if a.ring == Ring::Day && b.ring == Ring::Day && a.count.is_none() && b.count.is_none() && a.until.is_none() && b.until.is_none()
            && a.exdates.is_empty() && b.exdates.is_empty() && crt_pairs.len() < 2000 {
            crt_pairs.push(serde_json::json!({"p": a.period, "a": a.start - base, "q": b.period, "b": b.start - base,
                                               "x": first_coincidence(&a, &b, a.start.max(b.start)).map(|x| x - base)}));
        }
    }
    serde_json::json!({"pairs": n, "equal": same, "mismatches": diff, "with_conflict": found, "never_or_beyond": never,
                       "by_kind": kinds.iter().map(|(k, v)| (k.to_string(), serde_json::json!({"pairs": v.0, "mismatches": v.1, "residue_us": (v.2 as f64 / v.0.max(1) as f64 / 1e3 * 100.0).round() / 100.0}))).collect::<serde_json::Map<_, _>>(),
                       "residue_us_per_pair": (fast_ns as f64 / n as f64 / 1e3 * 100.0).round() / 100.0,
                       "reference_us_per_pair": (ref_ns as f64 / n as f64 / 1e3 * 10.0).round() / 10.0,
                       "first_mismatches": first_bad, "crt_pairs": crt_pairs})
}
