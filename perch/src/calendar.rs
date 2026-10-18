//! 日历订阅(ICS):`config.toml [calendar] ics = ["本地文件.ics", "https://…/basic.ics"]`。
//! 网址用系统自带的 curl.exe 在后台线程里每 30 分钟拉一次(HUD 自己不带 TLS 库);本地文件直接读。
//! 解析 VEVENT(UTC / 本地 / 全天),支持常见的 RRULE(DAILY / WEEKLY(BYDAY)/ MONTHLY / YEARLY,INTERVAL / COUNT / UNTIL)和 EXDATE,
//! 只展开从今天起 14 天内的事件。事件开始前 N 分钟(默认 10)发一次通知。

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub start: DateTime<Local>,
    pub end: Option<DateTime<Local>>,
    pub title: String,
    pub all_day: bool,
}

static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static STARTED: AtomicBool = AtomicBool::new(false);
static NOTIFIED: Mutex<Vec<String>> = Mutex::new(Vec::new());

const WINDOW_DAYS: i64 = 14;

/// 后台线程:启动时和之后每 30 分钟刷新一次;没配置日历源时什么都不做(线程仍每 30 分钟醒一次检查配置)
pub fn start_background() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| loop {
        refresh();
        let mins = crate::config::get().intervals.ics_min.max(1);
        std::thread::sleep(std::time::Duration::from_secs(mins * 60));
    });
}

fn load_source(src: &str) -> Option<String> {
    if src.starts_with("http://") || src.starts_with("https://") {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("curl.exe").args(["-sL", "--max-time", "25", src]).creation_flags(0x0800_0000).output().ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        std::fs::read_to_string(crate::config::resolve_target(src)).ok()
    }
}

pub fn refresh() {
    let sources = crate::config::get().calendar.ics;
    let from = Local::now().date_naive().and_hms_opt(0, 0, 0).and_then(|d| Local.from_local_datetime(&d).single()).unwrap_or_else(Local::now);
    let to = from + Duration::days(WINDOW_DAYS);
    let mut all: Vec<Event> = Vec::new();
    for s in &sources {
        if let Some(text) = load_source(s) {
            all.extend(parse_ics(&text, from, to));
        } else {
            crate::log::log(&format!("calendar: failed to load a source ({} chars)", s.len()));
        }
    }
    all.sort_by_key(|e| e.start);
    if let Ok(mut e) = EVENTS.lock() {
        *e = all;
    }
}

/// 还没结束的事件(按开始时间排序)
pub fn upcoming() -> Vec<Event> {
    let now = Local::now();
    EVENTS.lock().map(|e| e.iter().filter(|x| x.end.unwrap_or(x.start + Duration::hours(1)) >= now || x.all_day && x.start.date_naive() >= now.date_naive()).cloned().collect()).unwrap_or_default()
}

/// 本月有事件的日子(日历格子上画小点)
pub fn days_with_events(year: i32, month: u32) -> HashSet<u32> {
    EVENTS.lock().map(|e| e.iter().filter(|x| x.start.year() == year && x.start.month() == month).map(|x| x.start.day()).collect()).unwrap_or_default()
}

/// 即将开始、还没提醒过的事件(开始前 notify_min 分钟内)
pub fn due_notifications() -> Vec<Event> {
    let mins = crate::config::get().calendar.notify_min as i64;
    if mins == 0 {
        return Vec::new();
    }
    let now = Local::now();
    let events = EVENTS.lock().map(|e| e.clone()).unwrap_or_default();
    let mut out = Vec::new();
    if let Ok(mut done) = NOTIFIED.lock() {
        for e in events.into_iter().filter(|e| !e.all_day && e.start > now && e.start <= now + Duration::minutes(mins)) {
            let key = format!("{}|{}", e.start.timestamp(), e.title);
            if !done.contains(&key) {
                done.push(key);
                out.push(e);
            }
        }
        if done.len() > 200 {
            done.drain(0..100);
        }
    }
    out
}

// ---------------- ICS 解析 ----------------

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&raw[1..]);
        } else {
            lines.push(raw.to_string());
        }
    }
    lines
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

/// 返回 (时刻, 是否全天)
fn parse_dt(value: &str) -> Option<(DateTime<Local>, bool)> {
    let v = value.trim();
    if v.len() == 8 && v.chars().all(|c| c.is_ascii_digit()) {
        let d = NaiveDate::parse_from_str(v, "%Y%m%d").ok()?;
        return Some((Local.from_local_datetime(&d.and_time(NaiveTime::MIN)).single()?, true));
    }
    if let Some(z) = v.strip_suffix('Z') {
        let n = NaiveDateTime::parse_from_str(z, "%Y%m%dT%H%M%S").ok()?;
        return Some((Utc.from_utc_datetime(&n).with_timezone(&Local), false));
    }
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    Some((Local.from_local_datetime(&n).single()?, false))
}

#[derive(Default, Debug)]
struct Rule {
    freq: String,
    interval: i64,
    count: Option<usize>,
    until: Option<DateTime<Local>>,
    byday: Vec<Weekday>,
}

fn parse_rrule(v: &str) -> Rule {
    let mut r = Rule { interval: 1, ..Default::default() };
    for part in v.split(';') {
        let Some((k, val)) = part.split_once('=') else { continue };
        match k.to_uppercase().as_str() {
            "FREQ" => r.freq = val.to_uppercase(),
            "INTERVAL" => r.interval = val.parse().unwrap_or(1).max(1),
            "COUNT" => r.count = val.parse().ok(),
            "UNTIL" => r.until = parse_dt(val).map(|x| x.0),
            "BYDAY" => {
                r.byday = val
                    .split(',')
                    .filter_map(|d| match d.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == '+') {
                        "MO" => Some(Weekday::Mon),
                        "TU" => Some(Weekday::Tue),
                        "WE" => Some(Weekday::Wed),
                        "TH" => Some(Weekday::Thu),
                        "FR" => Some(Weekday::Fri),
                        "SA" => Some(Weekday::Sat),
                        "SU" => Some(Weekday::Sun),
                        _ => None,
                    })
                    .collect()
            }
            _ => {}
        }
    }
    r
}

/// 展开重复规则里落在 [from, to) 内的开始时刻
fn expand(start: DateTime<Local>, rule: &Rule, from: DateTime<Local>, to: DateTime<Local>) -> Vec<DateTime<Local>> {
    let mut out = Vec::new();
    let mut produced = 0usize;
    let mut push = |t: DateTime<Local>, out: &mut Vec<DateTime<Local>>, produced: &mut usize| -> bool {
        if rule.until.map_or(false, |u| t > u) || rule.count.map_or(false, |c| *produced >= c) {
            return false;
        }
        *produced += 1;
        if t >= from && t < to {
            out.push(t);
        }
        true
    };
    let mut step = 0i64;
    for _ in 0..3000 {
        let base = match rule.freq.as_str() {
            "DAILY" => Some(start + Duration::days(step * rule.interval)),
            "WEEKLY" => Some(start + Duration::weeks(step * rule.interval)),
            "MONTHLY" => {
                let m0 = start.month0() as i64 + step * rule.interval;
                let (y, m) = (start.year() + (m0 / 12) as i32, (m0 % 12) as u32 + 1);
                NaiveDate::from_ymd_opt(y, m, start.day()).and_then(|d| Local.from_local_datetime(&d.and_time(start.time())).single())
            }
            "YEARLY" => NaiveDate::from_ymd_opt(start.year() + (step * rule.interval) as i32, start.month(), start.day()).and_then(|d| Local.from_local_datetime(&d.and_time(start.time())).single()),
            _ => return out,
        };
        step += 1;
        let Some(base) = base else { continue };
        if base >= to + Duration::days(7) {
            break;
        }
        if rule.freq == "WEEKLY" && !rule.byday.is_empty() {
            // 这一周里 BYDAY 指定的每一天(以 base 所在周的周一为起点)
            let monday = base - Duration::days(base.weekday().num_days_from_monday() as i64);
            let mut days: Vec<DateTime<Local>> = rule.byday.iter().map(|w| monday + Duration::days(w.num_days_from_monday() as i64)).filter(|d| *d >= start).collect();
            days.sort();
            for d in days {
                if !push(d, &mut out, &mut produced) {
                    return out;
                }
            }
        } else if !push(base, &mut out, &mut produced) {
            return out;
        }
    }
    out
}

/// 解析 ICS 文本,返回 [from, to) 内的事件(含重复展开)
pub fn parse_ics(text: &str, from: DateTime<Local>, to: DateTime<Local>) -> Vec<Event> {
    let mut events = Vec::new();
    let mut cur: Option<Vec<(String, String, String)>> = None; // (属性名, 参数, 值)
    for line in unfold(text) {
        let up = line.to_uppercase();
        if up == "BEGIN:VEVENT" {
            cur = Some(Vec::new());
        } else if up == "END:VEVENT" {
            if let Some(props) = cur.take() {
                events.extend(build_events(&props, from, to));
            }
        } else if let Some(props) = cur.as_mut() {
            if let Some((left, value)) = line.split_once(':') {
                let (name, params) = left.split_once(';').unwrap_or((left, ""));
                props.push((name.to_uppercase(), params.to_uppercase(), value.to_string()));
            }
        }
    }
    events.sort_by_key(|e| e.start);
    events
}

fn build_events(props: &[(String, String, String)], from: DateTime<Local>, to: DateTime<Local>) -> Vec<Event> {
    let get = |n: &str| props.iter().find(|p| p.0 == n).map(|p| p.2.clone());
    let Some((start, all_day)) = get("DTSTART").and_then(|v| parse_dt(&v)) else { return Vec::new() };
    if props.iter().any(|p| p.0 == "STATUS" && p.2.eq_ignore_ascii_case("CANCELLED")) {
        return Vec::new();
    }
    let end = get("DTEND").and_then(|v| parse_dt(&v)).map(|x| x.0);
    let dur = end.map(|e| e - start);
    let title = unescape(&get("SUMMARY").unwrap_or_else(|| perch_data::t("misc.untitled")));
    let ex: HashSet<String> = props.iter().filter(|p| p.0 == "EXDATE").flat_map(|p| p.2.split(',').filter_map(|v| parse_dt(v).map(|x| x.0.format("%Y%m%d").to_string())).collect::<Vec<_>>()).collect();
    let starts = match get("RRULE") {
        Some(r) => expand(start, &parse_rrule(&r), from, to),
        None => {
            // 单次事件:落在窗口里,或者已经开始但还没结束(跨天)
            let e = end.unwrap_or(start);
            if (start >= from && start < to) || (start < from && e > from) {
                vec![start]
            } else {
                Vec::new()
            }
        }
    };
    starts.into_iter().filter(|s| !ex.contains(&s.format("%Y%m%d").to_string())).map(|s| Event { start: s, end: dur.map(|d| s + d), title: title.clone(), all_day }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> (DateTime<Local>, DateTime<Local>) {
        let from = Local.with_ymd_and_hms(2026, 10, 17, 0, 0, 0).unwrap();
        (from, from + Duration::days(14))
    }

    #[test]
    fn single_utc_and_allday_and_folding() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261017T060000Z\r\nDTEND:20261017T070000Z\r\nSUMMARY:项目评审\r\n 会议\\, 三楼\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261020\r\nSUMMARY:全天事件\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART:20270101T000000Z\r\nSUMMARY:太远了\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (f, t) = window();
        let ev = parse_ics(ics, f, t);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].title, "项目评审会议, 三楼");
        assert_eq!(ev[0].start.with_timezone(&Utc).format("%H:%M").to_string(), "06:00");
        assert!(!ev[0].all_day && ev[1].all_day);
        assert_eq!(ev[1].start.day(), 20);
    }

    #[test]
    fn weekly_byday_count_and_exdate() {
        // 每周一三,共 6 次,第一个周三被排除
        let ics = "BEGIN:VEVENT\nDTSTART:20261012T090000\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6\nEXDATE:20261014T090000\nSUMMARY:站会\nEND:VEVENT\n";
        let (f, t) = window();
        let ev = parse_ics(ics, f, t);
        let days: Vec<String> = ev.iter().map(|e| e.start.format("%m-%d").to_string()).collect();
        // COUNT=6 从 10-12 起:12(一) 14(三,排除) 19(一) 21(三) 26(一) 28(三);窗口从 10-17 起
        assert_eq!(days, vec!["10-19", "10-21", "10-26", "10-28"]);
    }

    #[test]
    fn daily_until_and_monthly() {
        let ics = "BEGIN:VEVENT\nDTSTART:20261016T100000\nRRULE:FREQ=DAILY;INTERVAL=2;UNTIL=20261022T235959\nSUMMARY:隔天\nEND:VEVENT\nBEGIN:VEVENT\nDTSTART:20260920T120000\nRRULE:FREQ=MONTHLY\nSUMMARY:月度\nEND:VEVENT\n";
        let (f, t) = window();
        let ev = parse_ics(ics, f, t);
        let a: Vec<String> = ev.iter().filter(|e| e.title == "隔天").map(|e| e.start.format("%m-%d").to_string()).collect();
        assert_eq!(a, vec!["10-18", "10-20", "10-22"]);
        let m: Vec<String> = ev.iter().filter(|e| e.title == "月度").map(|e| e.start.format("%m-%d").to_string()).collect();
        assert_eq!(m, vec!["10-20"]);
    }

    #[test]
    fn cancelled_is_skipped() {
        let ics = "BEGIN:VEVENT\nDTSTART:20261018T100000\nSTATUS:CANCELLED\nSUMMARY:取消\nEND:VEVENT\n";
        let (f, t) = window();
        assert!(parse_ics(ics, f, t).is_empty());
    }
}
