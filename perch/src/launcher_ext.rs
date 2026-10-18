//! 启动器扩展(search.rs 调用):
//!   f 关键字  → 通过 Everything 的 IPC 查文件(Everything 必须在运行;HUD 自己不建索引,零额外占用)
//!   = 算式 或直接输入算式  → 计算器,回车复制结果
//!   g 关键字  → 网页搜索(config.toml [launcher] search_url,%s 为关键字,默认 Bing)
//!   > 命令    → 直接运行命令
//! 特殊结果用 "nx:" 前缀的 PathBuf 表示,由 run_special 执行。

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub type Row = (String, PathBuf);

fn row(label: impl Into<String>, action: impl Into<String>) -> Row {
    (label.into(), PathBuf::from(action.into()))
}

/// 中文输入法下常见的全角符号 → 半角(括号、运算符、> / : 等)
pub fn normalize(q: &str) -> String {
    q.chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '。' => '.',
            '、' => '/',
            '》' => '>',
            '　' => ' ',
            c => c,
        })
        .collect()
}

pub fn mode_labels() -> [String; 5] {
    [
        perch_data::t("launcher.mode_apps"),
        perch_data::t("launcher.mode_files"),
        perch_data::t("launcher.mode_windows"),
        perch_data::t("launcher.mode_ai"),
        perch_data::t("launcher.mode_commands"),
    ]
}

pub fn in_mode(q: &str, mode: u8) -> Option<Vec<Row>> {
    let t = q.trim();
    match mode {
        1 => Some(files(t)),
        2 => Some(windows(t)),
        3 => Some(if t.is_empty() { vec![row(perch_data::t("launcher.ask_ai_empty"), "nx:none")] } else { ai_rows(t) }),
        4 => Some(if t.is_empty() { vec![row(perch_data::t("launcher.run_cmd_empty"), "nx:none")] } else { vec![row(format!("{}:{}", perch_data::t("launcher.run_prefix"), t), format!("nx:cmd:{}", t))] }),
        _ => special(q),
    }
}

/// 带前缀的特殊查询;不是特殊查询返回 None,由普通程序搜索处理
pub fn special(q: &str) -> Option<Vec<Row>> {
    let t = q.trim_start();
    for p in ["f ", "/ ", "e "] {
        if let Some(rest) = t.strip_prefix(p) {
            return Some(files(rest.trim()));
        }
    }
    for p in ["g ", "s "] {
        if let Some(rest) = t.strip_prefix(p) {
            let rest = rest.trim();
            return Some(if rest.is_empty() { vec![row(perch_data::t("launcher2.search_empty"), "nx:none")] } else { vec![web_row(rest)] });
        }
    }
    if let Some(rest) = t.strip_prefix('>') {
        let rest = rest.trim();
        return Some(if rest.is_empty() { vec![row(perch_data::t("launcher.run_cmd_empty"), "nx:none")] } else { vec![row(perch_data::t_fmt("launcher2.run_fmt", &[("cmd", rest)]), format!("nx:cmd:{}", rest))] });
    }
    let expr = t.strip_prefix('=').unwrap_or(t).trim();
    if t.starts_with('=') || looks_like_calc(expr) {
        return Some(match eval(expr) {
            Some(v) => {
                let s = fmt_num(v);
                vec![row(perch_data::t_fmt("launcher2.calc_fmt", &[("val", &s)]), format!("nx:calc:{}", s))]
            }
            None => vec![row(perch_data::t("launcher2.calc_bad"), "nx:none")],
        });
    }
    None
}

// ---------------- 问 AI ----------------

struct Ai {
    name: String,
    url: String,
    /// 没有网址预填能力:先把问题复制到剪贴板,打开后让用户 Ctrl+V
    copy: bool,
}

/// config.toml 里的 [[launcher.ai]](name / url / copy),没写就用内置的四个。url 里 %s 是问题
fn ai_targets() -> Vec<Ai> {
    let from_cfg: Vec<Ai> = std::fs::read_to_string(crate::config::path())
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("launcher").and_then(|l| l.get("ai")).and_then(|a| a.as_array()).cloned())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let (name, url) = (e.get("name")?.as_str()?.to_string(), e.get("url")?.as_str()?.to_string());
                    Some(Ai { name, url, copy: e.get("copy").and_then(|c| c.as_bool()).unwrap_or(false) })
                })
                .collect()
        })
        .unwrap_or_default();
    if !from_cfg.is_empty() {
        return from_cfg;
    }
    let d = |n: &str, u: &str, c: bool| Ai { name: n.to_string(), url: u.to_string(), copy: c };
    vec![
        d("ChatGPT", "https://chatgpt.com/?q=%s", false),
        d(&perch_data::t("launcher2.ai_google"), "https://www.google.com/search?q=%s&udm=50", false),
        d("Claude", "https://claude.ai/new?q=%s", false),
        d("Gemini", "https://gemini.google.com/app", true),
    ]
}

fn last_ai_path() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("perch").join("launcher_last.txt")
}

/// 问 AI 的候选行:上次用的排第一,最后一项是传统网页搜索
pub fn ai_rows(q: &str) -> Vec<Row> {
    let q: String = q.trim().chars().take(1500).collect();
    let mut list = ai_targets();
    let last = std::fs::read_to_string(last_ai_path()).unwrap_or_default();
    if let Some(i) = list.iter().position(|a| a.name == last.trim()) {
        let a = list.remove(i);
        list.insert(0, a);
    }
    let mut rows: Vec<Row> = list
        .iter()
        .map(|a| {
            let url = a.url.replace("%s", &percent_encode(&q));
            if a.copy {
                row(perch_data::t_fmt("launcher2.ask_copy", &[("name", &a.name)]), format!("nx:aicopy:{}\u{1}{}\u{1}{}", a.name, url, q))
            } else {
                row(perch_data::t_fmt("launcher2.ask", &[("name", &a.name)]), format!("nx:ai:{}\u{1}{}", a.name, url))
            }
        })
        .collect();
    rows.push(web_row(&q));
    rows
}

/// 普通程序搜索没有结果时的兜底:AI 前三个 + 网页搜索
pub fn fallback(q: &str) -> Vec<Row> {
    let mut rows = ai_rows(q);
    let web = rows.pop();
    rows.truncate(3);
    rows.extend(web);
    rows
}

fn remember_ai(name: &str) {
    let p = last_ai_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, name);
}

fn open_url(url: &str) {
    let wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

/// 普通搜索没有结果时的兜底:网页搜索
pub fn web_row(q: &str) -> Row {
    let tpl = crate::config::raw_str("launcher", "search_url");
    let tpl = if tpl.contains("%s") { tpl } else { "https://www.bing.com/search?q=%s".to_string() };
    row(perch_data::t_fmt("launcher2.web_search", &[("q", q)]), format!("nx:web:{}", tpl.replace("%s", &percent_encode(q))))
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// 执行特殊结果;不是特殊结果返回 false
pub fn run_special(action: &str) -> bool {
    let Some(rest) = action.strip_prefix("nx:") else { return false };
    if let Some(v) = rest.strip_prefix("calc:") {
        crate::tools::set_clipboard(v);
    } else if let Some(v) = rest.strip_prefix("win:") {
        if let Some(h) = v.split('|').next().and_then(|s| s.parse::<isize>().ok()) {
            crate::running::focus_window(h);
        }
    } else if let Some(id) = rest.strip_prefix("sys:") {
        run_sys(id);
    } else if let Some(v) = rest.strip_prefix("ai:") {
        let mut it = v.splitn(2, '\u{1}');
        let (name, url) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
        remember_ai(name);
        open_url(url);
    } else if let Some(v) = rest.strip_prefix("aicopy:") {
        let mut it = v.splitn(3, '\u{1}');
        let (name, url, text) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        remember_ai(name);
        crate::tools::set_clipboard(text);
        open_url(url);
    } else if let Some(url) = rest.strip_prefix("web:") {
        let wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        unsafe {
            ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
        }
    } else if let Some(cmd) = rest.strip_prefix("cmd:") {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd.exe").args(["/C", cmd]).creation_flags(0x0800_0000).spawn();
    }
    true
}

// ---------------- 计算器 ----------------

fn looks_like_calc(s: &str) -> bool {
    let has_digit = s.chars().any(|c| c.is_ascii_digit());
    let ok_chars = s.chars().all(|c| c.is_ascii_digit() || "+-*/()%^. ×÷".contains(c));
    let has_op = s.char_indices().any(|(i, c)| "+*/%^×÷".contains(c) || (c == '-' && i > 0));
    has_digit && ok_chars && has_op
}

fn fmt_num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{:.10}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// + - * / % ^ 括号 一元负号 小数
fn eval(s: &str) -> Option<f64> {
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).map(|c| match c {
        '×' => '*',
        '÷' => '/',
        c => c,
    }).collect();
    let mut pos = 0usize;
    let v = expr(&chars, &mut pos)?;
    if pos != chars.len() || !v.is_finite() {
        return None;
    }
    Some(v)
}

fn expr(c: &[char], p: &mut usize) -> Option<f64> {
    let mut v = term(c, p)?;
    while *p < c.len() && (c[*p] == '+' || c[*p] == '-') {
        let op = c[*p];
        *p += 1;
        let r = term(c, p)?;
        v = if op == '+' { v + r } else { v - r };
    }
    Some(v)
}

fn term(c: &[char], p: &mut usize) -> Option<f64> {
    let mut v = power(c, p)?;
    while *p < c.len() && (c[*p] == '*' || c[*p] == '/' || c[*p] == '%') {
        let op = c[*p];
        *p += 1;
        let r = power(c, p)?;
        v = match op {
            '*' => v * r,
            '/' => v / r,
            _ => v % r,
        };
    }
    Some(v)
}

fn power(c: &[char], p: &mut usize) -> Option<f64> {
    let b = unary(c, p)?;
    if *p < c.len() && c[*p] == '^' {
        *p += 1;
        let e = power(c, p)?;
        return Some(b.powf(e));
    }
    Some(b)
}

fn unary(c: &[char], p: &mut usize) -> Option<f64> {
    if *p < c.len() && c[*p] == '-' {
        *p += 1;
        return Some(-unary(c, p)?);
    }
    if *p < c.len() && c[*p] == '+' {
        *p += 1;
        return unary(c, p);
    }
    atom(c, p)
}

fn atom(c: &[char], p: &mut usize) -> Option<f64> {
    if *p < c.len() && c[*p] == '(' {
        *p += 1;
        let v = expr(c, p)?;
        if *p < c.len() && c[*p] == ')' {
            *p += 1;
            return Some(v);
        }
        return None;
    }
    let start = *p;
    while *p < c.len() && (c[*p].is_ascii_digit() || c[*p] == '.') {
        *p += 1;
    }
    if start == *p {
        return None;
    }
    c[start..*p].iter().collect::<String>().parse().ok()
}

// ---------------- Everything IPC ----------------

static REPLY_HWND: AtomicIsize = AtomicIsize::new(0);
static REPLY: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn wstr_at(data: &[u8], off: usize) -> String {
    let mut u = Vec::new();
    let mut i = off;
    while i + 1 < data.len() {
        let w = u16::from_le_bytes([data[i], data[i + 1]]);
        if w == 0 {
            break;
        }
        u.push(w);
        i += 2;
    }
    String::from_utf16_lossy(&u)
}

unsafe extern "system" fn reply_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA && lparam.0 != 0 {
        let cds = &*(lparam.0 as *const COPYDATASTRUCT);
        if !cds.lpData.is_null() && cds.cbData >= 28 {
            let data = std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize);
            let rd = |o: usize| -> usize { if o + 4 <= data.len() { u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize } else { 0 } };
            let n = rd(20).min(16);
            let mut out = Vec::new();
            for i in 0..n {
                let it = 28 + 12 * i;
                let (fname, path) = (rd(it + 4), rd(it + 8));
                if fname == 0 {
                    continue;
                }
                let name = wstr_at(data, fname);
                let dir = wstr_at(data, path);
                out.push(if dir.is_empty() { PathBuf::from(name) } else { PathBuf::from(dir).join(name) });
            }
            if let Ok(mut r) = REPLY.lock() {
                *r = out;
            }
        }
        return LRESULT(1);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn reply_window() -> Option<HWND> {
    let cur = REPLY_HWND.load(Ordering::Relaxed);
    if cur != 0 {
        return Some(HWND(cur as *mut core::ffi::c_void));
    }
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let class = w!("PerchEverythingReply");
        let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(reply_proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
        RegisterClassExW(&wc);
        let h = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, HWND_MESSAGE, None, inst, None).ok()?;
        // Everything 常以管理员身份运行:普通权限的接收窗口必须放行 WM_COPYDATA(UIPI),否则收不到回复
        let _ = ChangeWindowMessageFilterEx(h, WM_COPYDATA, MSGFLT_ALLOW, None);
        REPLY_HWND.store(h.0 as isize, Ordering::Relaxed);
        Some(h)
    }
}

/// 向 Everything 发查询(WM_COPYDATA),同步等回复
fn everything_query(q: &str, max: u32) -> std::result::Result<Vec<PathBuf>, String> {
    unsafe {
        let ev = ["EVERYTHING", "EVERYTHING_(1.5a)"].iter().find_map(|c| {
            let w: Vec<u16> = c.encode_utf16().chain(Some(0)).collect();
            FindWindowW(PCWSTR(w.as_ptr()), None).ok().filter(|h| !h.0.is_null())
        });
        let Some(ev) = ev else { return Err(perch_data::t("launcher2.ev_missing")) };
        let Some(rh) = reply_window() else { return Err(perch_data::t("launcher2.ev_nowindow")) };
        // EVERYTHING_IPC_QUERYW: reply_hwnd, reply_copydata_message, search_flags, offset, max_results, search_string
        let mut buf: Vec<u8> = Vec::new();
        for v in [rh.0 as usize as u32, 0, 0, 0, max] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        for u in q.encode_utf16().chain(Some(0)) {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        if let Ok(mut r) = REPLY.lock() {
            r.clear();
        }
        let cds = COPYDATASTRUCT { dwData: 2, cbData: buf.len() as u32, lpData: buf.as_mut_ptr() as *mut core::ffi::c_void };
        SetLastError(WIN32_ERROR(0));
        let r = SendMessageW(ev, WM_COPYDATA, WPARAM(rh.0 as usize), LPARAM(&cds as *const COPYDATASTRUCT as isize));
        let err = GetLastError().0;
        let got = REPLY.lock().map(|g| g.clone()).unwrap_or_default();
        crate::log::log(&format!("everything ipc: q={:?} ret={} err={} items={}", q, r.0, err, got.len()));
        if r.0 == 0 && err == 5 {
            // ERROR_ACCESS_DENIED:Everything 以管理员运行,普通权限的进程被 UIPI 拦截
            return Err(perch_data::t("launcher2.ev_admin"));
        }
        if r.0 == 0 && got.is_empty() {
            return Err(perch_data::t("launcher2.ev_noresp"));
        }
        Ok(got)
    }
}

fn files(q: &str) -> Vec<Row> {
    if q.chars().count() < 2 {
        return vec![row(perch_data::t("launcher2.ev_short"), "nx:none")];
    }
    match everything_query(q, 8) {
        Err(msg) => vec![row(msg, "nx:none")],
        Ok(list) if list.is_empty() => vec![row(perch_data::t("launcher2.ev_none"), "nx:none")],
        Ok(list) => list.into_iter().map(|p| (p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string()), p)).collect(),
    }
}

// ---------------- 使用频率(frecency) ----------------

fn freq_path() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("perch").join("launcher_freq.json")
}

/// 路径 → (启动次数, 最近一次的 unix 秒)
pub fn freq_load() -> std::collections::HashMap<String, (u32, i64)> {
    std::fs::read_to_string(freq_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn freq_bump(path: &str) {
    let mut m = freq_load();
    let e = m.entry(path.to_lowercase()).or_insert((0, 0));
    e.0 += 1;
    e.1 = chrono::Local::now().timestamp();
    // 只保留最常用的 400 条
    if m.len() > 400 {
        let mut v: Vec<(String, (u32, i64))> = m.into_iter().collect();
        v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        v.truncate(400);
        m = v.into_iter().collect();
    }
    if let Some(dir) = freq_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(freq_path(), serde_json::to_string(&m).unwrap_or_default());
}

/// 排序加成:次数取对数,最近 7 天内用过再加一点
pub fn freq_bonus(m: &std::collections::HashMap<String, (u32, i64)>, path: &str) -> i32 {
    match m.get(&path.to_lowercase()) {
        None => 0,
        Some((c, last)) => {
            let recent = if chrono::Local::now().timestamp() - last < 7 * 86400 { 25 } else { 0 };
            ((*c as f32 + 1.0).ln() * 70.0) as i32 + recent
        }
    }
}

// ---------------- 窗口切换 ----------------

struct WinCtx(Vec<(isize, String, String)>);

unsafe extern "system" fn on_win(hwnd: HWND, lparam: LPARAM) -> BOOL {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    let list = &mut *(lparam.0 as *mut WinCtx);
    if !IsWindowVisible(hwnd).as_bool() || GetWindow(hwnd, GW_OWNER).map_or(false, |o| !o.0.is_null()) || GetWindowTextLengthW(hwnd) == 0 {
        return BOOL(1);
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
        return BOOL(1);
    }
    let mut cloaked = 0u32;
    if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4).is_ok() && cloaked != 0 {
        return BOOL(1);
    }
    let mut buf = [0u16; 200];
    let n = GetWindowTextW(hwnd, &mut buf) as usize;
    let title = String::from_utf16_lossy(&buf[..n]);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == windows::Win32::System::Threading::GetCurrentProcessId() {
        return BOOL(1);
    }
    list.0.push((hwnd.0 as isize, title, exe_of(pid)));
    BOOL(1)
}

fn exe_of(pid: u32) -> String {
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 520];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
        let _ = CloseHandle(h);
        if ok {
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            String::new()
        }
    }
}

/// 窗口模式:按标题 / 进程名过滤所有可见顶层窗口(Z 序,最近用过的在前),回车切过去
pub fn windows(q: &str) -> Vec<Row> {
    let mut ctx = WinCtx(Vec::new());
    unsafe {
        let _ = EnumWindows(Some(on_win), LPARAM(&mut ctx as *mut WinCtx as isize));
    }
    let q = q.trim().to_lowercase();
    let rows: Vec<Row> = ctx
        .0
        .into_iter()
        .filter(|(_, title, exe)| q.is_empty() || title.to_lowercase().contains(&q) || exe.to_lowercase().contains(&q))
        .take(8)
        .map(|(h, title, exe)| row(title, format!("nx:win:{}|{}", h, exe)))
        .collect();
    if rows.is_empty() {
        vec![row(perch_data::t("launcher2.win_none"), "nx:none")]
    } else {
        rows
    }
}

// ---------------- 系统命令 ----------------

/// (id, 翻译键, 关键词键)
const SYS_KEYS: [(&str, &str, &str); 8] = [
    ("lock", "sys.lock", "sys.lock_kw"),
    ("sleep", "sys.sleep", "sys.sleep_kw"),
    ("shutdown", "sys.shutdown", "sys.shutdown_kw"),
    ("restart", "sys.restart", "sys.restart_kw"),
    ("logoff", "sys.logoff", "sys.logoff_kw"),
    ("recycle", "sys.recycle", "sys.recycle_kw"),
    ("desktop", "sys.desktop", "sys.desktop_kw"),
    ("taskmgr", "sys.taskmgr", "sys.taskmgr_kw"),
];

pub fn commands(q: &str) -> Vec<Row> {
    let q = q.trim().to_lowercase();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    let pfx = perch_data::t("sys.prefix");
    SYS_KEYS
        .iter()
        .filter_map(|(id, name_key, kw_key)| {
            let name = perch_data::t(name_key);
            let kw = perch_data::t(kw_key);
            if name.to_lowercase().contains(&q) || kw.split(' ').any(|k| k.starts_with(&q)) {
                Some(row(format!("{}{}", pfx, name), format!("nx:sys:{}", id)))
            } else {
                None
            }
        })
        .collect()
}

fn run_sys(id: &str) {
    use std::os::windows::process::CommandExt;
    let spawn = |prog: &str, args: &[&str]| {
        let _ = std::process::Command::new(prog).args(args).creation_flags(0x0800_0000).spawn();
    };
    let danger = |what_key: &str| {
        let what = perch_data::t(what_key);
        crate::settings::confirm(&perch_data::t_fmt("sys.confirm_prompt", &[("what", &what)]))
    };
    match id {
        "lock" => spawn("rundll32.exe", &["user32.dll,LockWorkStation"]),
        "sleep" => spawn("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"]),
        "shutdown" if danger("sys.shutdown") => spawn("shutdown.exe", &["/s", "/t", "0"]),
        "restart" if danger("sys.restart") => spawn("shutdown.exe", &["/r", "/t", "0"]),
        "logoff" if danger("sys.logoff") => spawn("shutdown.exe", &["/l"]),
        "recycle" if danger("sys.recycle") => spawn("powershell.exe", &["-NoProfile", "-Command", "Clear-RecycleBin -Force -ErrorAction SilentlyContinue"]),
        "desktop" => spawn("explorer.exe", &["shell:::{3080F90D-D7AD-11D9-BD98-0000FFB1E2FA}"]),
        "taskmgr" => spawn("taskmgr.exe", &[]),
        _ => {}
    }
}

/// 回车 + Shift:文件结果打开所在文件夹并选中
pub fn reveal(path: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", path)).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calc_basic() {
        assert_eq!(eval("12*(3+4)+0.5"), Some(84.5));
        assert_eq!(eval("2^3^2"), Some(512.0));
        assert_eq!(eval("-3+10%4"), Some(-1.0));
        assert_eq!(eval("(1+2"), None);
        assert_eq!(eval("1/0"), None);
        assert_eq!(fmt_num(84.5), "84.5");
        assert_eq!(fmt_num(10.0), "10");
    }

    #[test]
    fn calc_detection() {
        assert!(looks_like_calc("12*(3+4)"));
        assert!(looks_like_calc("100-1"));
        assert!(!looks_like_calc("-5"));
        assert!(!looks_like_calc("chrome"));
        assert!(!looks_like_calc("2024"));
    }

    #[test]
    fn fullwidth_normalized() {
        assert_eq!(normalize("（1＋2）×3"), "(1+2)×3");
        assert_eq!(normalize("》ls"), ">ls");
        let rows = special(&normalize("（1＋2）×3")).unwrap();
        assert!(rows[0].0.starts_with("= 9"));
    }

    #[test]
    fn ai_rows_encode_question() {
        perch_data::i18n::set_language("zh-CN");
        let rows = ai_rows("rust 怎么写 & 测试");
        assert!(rows.iter().any(|r| r.0 == "问 ChatGPT" && r.1.to_string_lossy().contains("chatgpt.com/?q=rust%20%E6%80%8E")));
        assert!(rows.iter().any(|r| r.1.to_string_lossy().contains("udm=50")));
        assert!(rows.iter().any(|r| r.0.contains("Gemini") && r.1.to_string_lossy().starts_with("nx:aicopy:")));
        assert!(rows.last().unwrap().0.starts_with("网页搜索"));
        // & 被编码,不会截断参数
        assert!(!rows[0].1.to_string_lossy().contains("& "));
    }

    #[test]
    fn modes() {
        perch_data::i18n::set_language("zh-CN");
        assert!(in_mode("", 3).unwrap()[0].0.contains("输入你的问题"));
        assert!(in_mode("ls -la", 4).unwrap()[0].1.to_string_lossy().starts_with("nx:cmd:"));
        assert!(!windows("").is_empty());
        assert!(in_mode("chrome", 0).is_none());
    }

    /// 需要本机 Everything 在运行且是普通权限(或服务模式):cargo test everything_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn everything_live() {
        for q in ["hosts", "perch.exe", "config.toml"] {
            let t = std::time::Instant::now();
            let r = everything_query(q, 5);
            println!("{:?} -> {:?} ({} ms)", q, r, t.elapsed().as_millis());
        }
    }

    #[test]
    fn frecency_and_commands() {
        perch_data::i18n::set_language("zh-CN");
        let mut m = std::collections::HashMap::new();
        assert_eq!(freq_bonus(&m, r"C:\a.lnk"), 0);
        m.insert(r"c:\a.lnk".to_string(), (10u32, chrono::Local::now().timestamp()));
        m.insert(r"c:\b.lnk".to_string(), (1u32, 0));
        assert!(freq_bonus(&m, r"C:\A.lnk") > freq_bonus(&m, r"C:\b.lnk"));
        assert!(commands("锁屏").iter().any(|r| r.1.to_string_lossy() == "nx:sys:lock"));
        assert!(commands("shut").iter().any(|r| r.1.to_string_lossy() == "nx:sys:shutdown"));
        assert!(commands("x").is_empty());
    }
}
