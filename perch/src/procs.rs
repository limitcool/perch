//! 进程占用面板:点系统卡片弹出,按 CPU 或内存列出占用最高的程序(同名进程合并,如 Chrome 的几十个进程),可结束。
//! 只在面板打开时每 1.5 秒采样一次(进程快照 + GetProcessTimes + GetProcessMemoryInfo),关闭后不做任何事。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::Result;
use windows::Win32::Foundation::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE};

use crate::cards::{self, clip, View, C, L, P, R};
use crate::state::AppState;
use crate::window::RectBox;

const W: f32 = 328.0;
const H: f32 = 350.0;
const ROWS: usize = 8;

#[derive(Clone, Default)]
pub struct Row {
    pub name: String,
    pub count: usize,
    pub cpu: f32,
    pub mem_mb: f32,
    pub pids: Vec<u32>,
}

struct Sampler {
    prev: HashMap<u32, u64>,
    at: Instant,
    rows: Vec<Row>,
    sampled: bool,
}

static STATE: Mutex<Option<Sampler>> = Mutex::new(None);
/// 0 = 按 CPU,1 = 按内存
static BY_MEM: AtomicU8 = AtomicU8::new(0);

const KILL_ID: [&str; ROWS] = ["pr_x0", "pr_x1", "pr_x2", "pr_x3", "pr_x4", "pr_x5", "pr_x6", "pr_x7"];

/// 不允许结束的系统关键进程(以及 HUD 自己)
const PROTECTED: [&str; 14] = ["system", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe", "svchost.exe", "dwm.exe", "fontdrvhost.exe", "explorer.exe", "perch.exe", "system idle process"];

pub fn rect() -> (f32, f32, f32, f32) {
    let s = cards::system_rect();
    (s.0 - W - 8.0, s.1, W, H)
}

pub fn region() -> (f32, f32, f32, f32, f32) {
    let r = rect();
    (r.0, r.1, r.2, r.3, 12.0)
}

fn abs(x: f32, y: f32, w: f32, h: f32, id: &'static str) -> RectBox {
    let r = rect();
    RectBox { id, x: r.0 + x, y: r.1 + y, w, h }
}

pub fn boxes() -> Vec<RectBox> {
    let mut v = vec![abs(W - 36.0, 8.0, 28.0, 26.0, "pr_close"), abs(12.0, 40.0, 70.0, 24.0, "pr_cpu"), abs(88.0, 40.0, 70.0, 24.0, "pr_mem")];
    for k in 0..ROWS {
        v.push(abs(W - 40.0, 72.0 + 31.0 * k as f32, 28.0, 26.0, KILL_ID[k]));
    }
    v.push(abs(0.0, 0.0, W, H, "pr_panel"));
    v
}

fn filetime(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

fn sample() {
    let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
    let mut g = STATE.lock().unwrap();
    let st = g.get_or_insert_with(|| Sampler { prev: HashMap::new(), at: crate::log::ago(10), rows: Vec::new(), sampled: false });
    let dt = st.at.elapsed().as_secs_f32().max(0.1);
    let mut groups: HashMap<String, Row> = HashMap::new();
    let mut now_times: HashMap<u32, u64> = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return };
        let mut pe = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut pe).is_ok();
        while ok {
            let pid = pe.th32ProcessID;
            let len = pe.szExeFile.iter().position(|c| *c == 0).unwrap_or(pe.szExeFile.len());
            let name = String::from_utf16_lossy(&pe.szExeFile[..len]);
            if pid != 0 {
                if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
                    let mut cpu_pct = 0.0f32;
                    if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() {
                        let total = filetime(k) + filetime(u);
                        now_times.insert(pid, total);
                        if let Some(prev) = st.prev.get(&pid) {
                            // 100ns 单位 → 秒,再按核数归一
                            cpu_pct = (total.saturating_sub(*prev) as f32 / 1e7) / dt / ncpu * 100.0;
                        }
                    }
                    let mut mc = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
                    let mem = if GetProcessMemoryInfo(h, &mut mc, mc.cb).is_ok() { mc.WorkingSetSize as f32 / 1048576.0 } else { 0.0 };
                    let _ = CloseHandle(h);
                    let r = groups.entry(name.to_lowercase()).or_insert_with(|| Row { name: name.clone(), ..Default::default() });
                    r.count += 1;
                    r.cpu += cpu_pct;
                    r.mem_mb += mem;
                    r.pids.push(pid);
                }
            }
            ok = Process32NextW(snap, &mut pe).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    st.sampled = !st.prev.is_empty();
    st.prev = now_times;
    st.at = Instant::now();
    st.rows = groups.into_values().collect();
}

fn sorted(by_mem: bool) -> Vec<Row> {
    let g = STATE.lock().unwrap();
    let mut rows = g.as_ref().map(|s| s.rows.clone()).unwrap_or_default();
    if by_mem {
        rows.sort_by(|a, b| b.mem_mb.partial_cmp(&a.mem_mb).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        rows.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
    }
    rows.truncate(ROWS);
    rows
}

/// 面板打开期间由主循环调用;到点就重新采样,返回 true 表示需要重绘
pub fn tick() -> bool {
    let due = STATE.lock().unwrap().as_ref().map_or(true, |s| s.at.elapsed() >= Duration::from_millis(1500));
    if due {
        sample();
    }
    due
}

/// 面板关闭:丢掉采样状态
pub fn reset() {
    *STATE.lock().unwrap() = None;
}

fn is_protected(name: &str) -> bool {
    PROTECTED.contains(&name.to_lowercase().as_str())
}

fn kill_group(row: &Row) {
    for pid in &row.pids {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, *pid) {
                let _ = TerminateProcess(h, 1);
                let _ = CloseHandle(h);
            }
        }
    }
}

pub fn draw(p: &mut P, _v: &View) -> Result<()> {
    p.at(rect());
    let th = p.th;
    let (white, muted) = (p.col(&th.white), p.col(&th.muted));
    let faint = windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 35.0 / 255.0 };
    p.rect(0.0, 0.0, W, H, 12.0, th.bg.with_alpha(250), p.col(&th.stroke), 1.5)?;
    p.line(12.0, 1.0, W - 12.0, 1.0, faint, 1.0)?;
    p.txt(16.0, 12.0, &perch_data::t("procs.title"), 9.5, p.col(&th.accent), L, false)?;
    p.txt(W - 14.0, 12.0, "✕", 9.0, muted, R, false)?;
    let by_mem = BY_MEM.load(Ordering::Relaxed) == 1;
    for (i, (id, label, x)) in [("pr_cpu", perch_data::t("procs.by_cpu"), 12.0), ("pr_mem", perch_data::t("procs.by_mem"), 88.0)].iter().enumerate() {
        let on = (i == 1) == by_mem;
        let (f, s, sw_) = if on { (p.col(&th.stroke), p.col(&th.accent), 1.5) } else { (p.col(&th.item_bg), p.col(&th.stroke), 1.0) };
        let (f, s) = if p.hot(id) { (p.col(&th.item_hover), p.col(&th.accent)) } else { (f, s) };
        p.rect(*x, 40.0, 70.0, 24.0, 6.0, f, s, sw_)?;
        p.txt(x + 35.0, 44.0, label, 8.0, white, C, false)?;
    }
    let ready = STATE.lock().unwrap().as_ref().map_or(false, |s| s.sampled);
    if !ready {
        p.txt(W / 2.0, 170.0, &perch_data::t("procs.sampling"), 9.0, muted, C, false)?;
        return Ok(());
    }
    for (k, r) in sorted(by_mem).iter().enumerate() {
        let y = 72.0 + 31.0 * k as f32;
        let hot = p.hot(KILL_ID[k]);
        p.rect(10.0, y, W - 20.0, 28.0, 6.0, p.col(&th.item_bg), p.col(&th.stroke), 1.0)?;
        let label = if r.count > 1 { format!("{} ×{}", r.name, r.count) } else { r.name.clone() };
        p.txt(18.0, y + 6.0, &clip(&label, 20), 8.0, white, L, false)?;
        let cpu_c = if r.cpu >= 50.0 { crate::theme::ColorRgba::new(239, 68, 68, 255).to_d2d() } else { p.col(&th.accent) };
        p.txt(W - 108.0, y + 7.0, &format!("{:.1}%", r.cpu), 7.5, cpu_c, R, true)?;
        let mem = if r.mem_mb >= 1024.0 { format!("{:.1}G", r.mem_mb / 1024.0) } else { format!("{:.0}M", r.mem_mb) };
        p.txt(W - 52.0, y + 7.0, &mem, 7.5, p.col(&th.sub), R, true)?;
        if is_protected(&r.name) {
            p.txt(W - 26.0, y + 6.0, "🔒", 8.0, muted, C, false)?;
        } else {
            let col = if hot { crate::theme::ColorRgba::new(255, 120, 120, 255).to_d2d() } else { crate::theme::ColorRgba::new(239, 68, 68, 200).to_d2d() };
            p.txt(W - 26.0, y + 6.0, "✕", 8.5, col, C, false)?;
        }
    }
    p.txt(W / 2.0, H - 18.0, &perch_data::t("procs.legend"), 7.0, muted, C, false)?;
    Ok(())
}

pub fn on_click(id: &str, app: &mut AppState) {
    match id {
        "pr_close" => {
            app.procs_open = false;
            reset();
        }
        "pr_cpu" => BY_MEM.store(0, Ordering::Relaxed),
        "pr_mem" => BY_MEM.store(1, Ordering::Relaxed),
        _ => {
            if let Some(k) = KILL_ID.iter().position(|i| *i == id) {
                let rows = sorted(BY_MEM.load(Ordering::Relaxed) == 1);
                if let Some(r) = rows.get(k) {
                    if is_protected(&r.name) {
                        return;
                    }
                    if crate::settings::confirm(&perch_data::t_fmt("procs.kill_confirm", &[("name", &r.name), ("n", &r.count.to_string())])) {
                        kill_group(r);
                        reset();
                    }
                }
            }
        }
    }
}
