// SPDX-License-Identifier: MIT OR Apache-2.0
//! iCalendar (RFC 5545) subset → residue-clock rules (recur.rs). Supported: VEVENT with SUMMARY, DTSTART (DATE or local
//! DATE-TIME; a trailing Z / TZID is read as wall-clock time — no zone maths yet), DTEND or DURATION, EXDATE, and RRULE with
//! FREQ = DAILY | WEEKLY | MONTHLY | YEARLY, INTERVAL, COUNT, UNTIL, BYDAY (plain weekdays, WEEKLY), BYMONTHDAY (positive,
//! MONTHLY), BYDAY with an ordinal (2TU, -1FR: n-th weekday, MONTHLY), BYMONTH (= the DTSTART month, YEARLY). Anything else
//! is reported in `warnings` and the event gets NO rules — never a guess (a guessed rule shows wrong days).
use crate::recur::{days_from_civil, Ring, Rule};

pub struct Event { pub summary: String, pub rules: Vec<Rule>, pub warnings: Vec<String> }

const WD: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
/// 0 = Monday (1970-01-01 was a Thursday = 3)
pub fn weekday(day: i64) -> i64 { (day + 3).rem_euclid(7) }

fn parse_dt(v: &str) -> Option<(i64, Option<i64>)> {
    let v = v.trim().trim_end_matches('Z');
    let (d, t) = v.split_once('T').map(|(a, b)| (a, Some(b))).unwrap_or((v, None));
    if d.len() != 8 { return None; }
    let (y, m, dd) = (d[0..4].parse().ok()?, d[4..6].parse().ok()?, d[6..8].parse().ok()?);
    let mins = match t { Some(t) if t.len() >= 4 => Some(t[0..2].parse::<i64>().ok()? * 60 + t[2..4].parse::<i64>().ok()?), _ => None };
    Some((days_from_civil(y, m, dd), mins))
}

fn parse_duration(v: &str) -> Option<i64> {
    // P[nD][T[nH][nM]] and PnW, in minutes
    let v = v.trim().trim_start_matches('+');
    let mut rest = v.strip_prefix('P')?;
    let mut mins = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        match c {
            '0'..='9' => num.push(c),
            'T' => in_time = true,
            'W' => { mins += num.parse::<i64>().ok()? * 7 * 1440; num.clear(); }
            'D' => { mins += num.parse::<i64>().ok()? * 1440; num.clear(); }
            'H' if in_time => { mins += num.parse::<i64>().ok()? * 60; num.clear(); }
            'M' if in_time => { mins += num.parse::<i64>().ok()?; num.clear(); }
            'S' if in_time => { num.clear(); }
            _ => return None,
        }
    }
    Some(mins)
}

fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in text.split('\n') {
        let l = raw.trim_end_matches('\r');
        if (l.starts_with(' ') || l.starts_with('\t')) && !out.is_empty() { out.last_mut().unwrap().push_str(&l[1..]); } else { out.push(l.to_string()); }
    }
    out
}

pub fn parse(text: &str) -> Vec<Event> {
    let mut events = vec![];
    let mut cur: Option<Vec<(String, String)>> = None;
    for line in unfold(text) {
        if line == "BEGIN:VEVENT" { cur = Some(vec![]); continue; }
        if line == "END:VEVENT" { if let Some(p) = cur.take() { events.push(event(&p)); } continue; }
        if let (Some(p), Some((k, v))) = (cur.as_mut(), line.split_once(':')) {
            let name = k.split(';').next().unwrap_or(k).to_ascii_uppercase();
            p.push((name, v.to_string()));
        }
    }
    events
}

fn event(props: &[(String, String)]) -> Event {
    let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let mut warnings = vec![];
    let summary = get("SUMMARY").unwrap_or("(no title)").replace("\\,", ",").replace("\\;", ";").replace("\\n", " ");
    let Some((day, tmin)) = get("DTSTART").and_then(parse_dt) else {
        return Event { summary, rules: vec![], warnings: vec!["no readable DTSTART".into()] };
    };
    let (t0, t1) = match tmin {
        None => (0, 1440),
        Some(t0) => {
            let end = get("DTEND").and_then(parse_dt).map(|(d, t)| (d - day) * 1440 + t.unwrap_or(0))
                .or_else(|| get("DURATION").and_then(parse_duration).map(|m| t0 + m)).unwrap_or(t0);
            (t0, end.max(t0 + 1).min(1440))
        }
    };
    let mut exdates: Vec<i64> = props.iter().filter(|(n, _)| n == "EXDATE").flat_map(|(_, v)| v.split(',').filter_map(parse_dt).map(|(d, _)| d).collect::<Vec<_>>()).collect();
    exdates.sort(); exdates.dedup();
    let base = Rule { ring: Ring::Day, start: day, dom: 1, nth: None, period: 1, count: Some(1), until: None, exdates: exdates.clone(), t0, t1 };
    let Some(rr) = get("RRULE") else { return Event { summary, rules: vec![base], warnings } };
    let parts: Vec<(String, String)> = rr.split(';').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_ascii_uppercase(), v.to_string())).collect();
    let part = |k: &str| parts.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    for (k, _) in &parts {
        if !["FREQ", "INTERVAL", "COUNT", "UNTIL", "BYDAY", "BYMONTHDAY", "BYMONTH", "WKST"].contains(&k.as_str()) { warnings.push(format!("RRULE {k} not supported")); }
    }
    let interval: i64 = part("INTERVAL").and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
    let count: Option<i64> = part("COUNT").and_then(|v| v.parse().ok());
    // UNTIL is inclusive; a DATE-TIME before the event's start time on that day excludes that day
    let until = part("UNTIL").and_then(parse_dt).map(|(d, t)| if t.is_some_and(|t| tmin.is_some_and(|s| t < s)) { d - 1 } else { d });
    let rule = |ring: Ring, start: i64, dom: i64, period: i64| Rule { ring, start, dom, nth: None, period, count, until, exdates: exdates.clone(), t0, t1 };
    let (y, m, dd) = crate::recur::civil_from_days(day);
    let mi = 12 * y + m - 1;
    let rules = match part("FREQ").unwrap_or("") {
        "DAILY" => vec![rule(Ring::Day, day, 1, interval)],
        "WEEKLY" => {
            let days: Vec<i64> = part("BYDAY").map(|v| v.split(',').filter_map(|d| WD.iter().position(|w| *w == d.trim()).map(|i| i as i64)).collect())
                .unwrap_or_else(|| vec![weekday(day)]);
            if part("BYDAY").is_some_and(|v| v.split(',').count() != days.len()) { warnings.push("BYDAY with ordinals not supported".into()); }
            if days.len() > 1 && count.is_some() { warnings.push("COUNT over several BYDAY weekdays not supported".into()); return Event { summary, rules: vec![], warnings }; }
            let monday = day - weekday(day);
            days.iter().map(|&wd| {
                let mut s = monday + wd;
                if s < day { s += 7 * interval; } // that weekday already passed in DTSTART's week: next week in the INTERVAL sequence
                rule(Ring::Day, s, 1, 7 * interval)
            }).collect()
        }
        "MONTHLY" if part("BYDAY").is_some() => {
            // n-th weekday of the month: "2TU", "-1FR" (an ordinal is required: a bare weekday means every such day)
            let mut out = vec![];
            let items: Vec<&str> = part("BYDAY").unwrap().split(',').map(|s| s.trim()).collect();
            if items.len() > 1 && count.is_some() { warnings.push("COUNT over several BYDAY not supported".into()); }
            if part("BYMONTHDAY").is_some() { warnings.push("BYDAY with BYMONTHDAY not supported".into()); }
            for it in items {
                let (num, wd) = it.split_at(it.len().saturating_sub(2));
                match (num.parse::<i64>(), WD.iter().position(|w| *w == wd)) {
                    (Ok(n), Some(w)) if n != 0 && n.abs() <= 5 => {
                        let mut r = rule(Ring::Month, mi, 1, interval);
                        r.nth = Some((n, w as i64));
                        // this month's n-th weekday falls before DTSTART: the ring starts one INTERVAL later (as for BYMONTHDAY)
                        if crate::recur::nth_weekday(mi, n, w as i64).is_some_and(|x| x < day) { r.start = mi + interval; }
                        out.push(r);
                    }
                    _ => warnings.push(format!("MONTHLY BYDAY {it} not supported (needs an ordinal, e.g. 2TU)")),
                }
            }
            out
        }
        "MONTHLY" => {
            let doms: Vec<i64> = part("BYMONTHDAY").map(|v| v.split(',').filter_map(|d| d.trim().parse().ok()).collect()).unwrap_or_else(|| vec![dd]);
            if doms.iter().any(|&d| d < 1) { warnings.push("negative BYMONTHDAY not supported".into()); }
            if doms.len() > 1 && count.is_some() { warnings.push("COUNT over several BYMONTHDAY not supported".into()); return Event { summary, rules: vec![], warnings }; }
            doms.iter().filter(|&&d| d >= 1).map(|&d| rule(Ring::Month, if d < dd { mi + interval } else { mi }, d, interval)).collect()
        }
        "YEARLY" => {
            if part("BYMONTH").is_some_and(|v| v.parse::<i64>().ok() != Some(m)) { warnings.push("YEARLY BYMONTH other than the start month not supported".into()); }
            if part("BYDAY").is_some() { warnings.push("YEARLY BYDAY not supported".into()); }
            vec![rule(Ring::Month, mi, dd, 12 * interval)]
        }
        f => { warnings.push(format!("FREQ {f} not supported")); vec![] }
    };
    let rules = if warnings.is_empty() { rules } else { vec![] }; // any unsupported part: report, never guess
    Event { summary, rules, warnings }
}
