#![windows_subsystem = "windows"]

mod ambient;
mod audio;
mod backup;
mod brightness;
mod calendar;
mod clash;
mod cards;
mod config;
mod data;
mod desktop;
mod gpu;
mod launcher_ext;
mod live_theme;
mod log;
mod metrics;
mod net;
mod obsidian_tasks;
mod procs;
mod preview;
mod prompt;
mod quotes_data;
mod renderer;
mod running;
mod search;
mod settings;
mod spectrum;
mod tools;
mod state;
mod theme;
mod window;

use std::time::{Duration, Instant};
use windows::Win32::UI::WindowsAndMessaging::*;

use audio::Audio;
use data::Hub;
use metrics::MetricsSampler;
use renderer::D2dRenderer;
use state::AppState;
use window::{TRAY_TOOLS, TRAY_SETTINGS, TRAY_SEARCH, set_lifted, RELAYOUT, TRAY_FLOAT, TRAY_FONTS, TRAY_OPEN_SELECTOR, TRAY_RELOAD, apply_window_region, pop_click_events, update_hit_boxes, HudWindow, HOVERED_ID, MOUSE_POS, SHOULD_EXIT};

fn disk_path(i: usize) -> String {
    config::get().system.disks.get(i).cloned().unwrap_or_else(|| if i == 0 { r"C:\".into() } else { r"D:\".into() })
}

fn main() {
    // 先声明 DPI 感知,再取系统缩放;之后所有布局按 DIP 计算
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem();
        window::set_scale(dpi as f32 / 96.0);
    }
    log::install_panic_hook();

    // 单实例:自启 + 手动启动不会叠出两个 HUD
    let _single = unsafe {
        use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        let h = windows::Win32::System::Threading::CreateMutexW(None, true, windows::core::w!("Local\\PerchHudSingleInstance"));
        if h.is_err() || GetLastError() == ERROR_ALREADY_EXISTS {
            log::log("another instance is already running, exiting");
            std::process::exit(0);
        }
        h
    };

    log::log("HUD starting");
    desktop::set_watchdog(true);
    match run_app() {
        Ok(()) => {
            log::log("HUD exited normally");
        }
        Err(e) => {
            // 非零退出码:计划任务的"失败后重启"会据此把 HUD 拉起来
            log::log(&format!("[FATAL] {:?}", e));
            std::process::exit(1);
        }
    }
}

fn run_app() -> windows::core::Result<()> {
    cards::load_layout();
    let mut window = HudWindow::create()?;
    let hwnd = window.hwnd;
    log::log(&format!("HUD window created: {:?}", hwnd));
    unsafe {
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(window.hwnd);
        if dpi > 0 {
            window::set_scale(dpi as f32 / 96.0);
        }
    }
    let sc = window::scale();
    cards::set_screen(window.width as f32 / sc, window.height as f32 / sc);
    desktop::hide();
    let _icons_guard = desktop::RestoreOnDrop;
    let mut renderer = D2dRenderer::new(window.hwnd, window.width, window.height)?;
    let mut ambient = ambient::Ambient::new(&renderer.d2d_factory, &renderer.dwrite_factory, window.width as i32, window.height as i32)
        .map_err(|e| { log::log(&format!("ambient layer unavailable: {:?}", e)); e })
        .ok();
    let mut app = AppState::new("default");
    let mut hub = Hub::new();
    let audio = Audio::new();
    let mut sampler = MetricsSampler::new();
    let mut sys = sampler.sample_all();
    let mut disk_c = metrics::disk_gb(&disk_path(0));
    let mut disk_d = metrics::disk_gb(&disk_path(1));
    let mut vol = audio.get();
    let mut gpu_info = gpu::sample();
    let mut last_gpu_alert: Option<Instant> = None;
    net::start();

    // 窗口只保留卡片区域(换装面板打开 / 战术顶栏时动态增减),其余位置点击穿透到桌面
    let hover_now = || HOVERED_ID.lock().ok().and_then(|h| *h);
    let layout_key = |a: &AppState, nv: bool| (nv, a.notif_open, a.selector_open, a.chrome_tactical, a.cal_open, a.style.radius as i32, cards::tooltip_rect().map(|t| (t.0 as i32, t.1 as i32, t.2 as i32)), a.edit_layout, cards::layout_signature(), (a.settings_open, a.settings_tab, a.settings_scroll, config::dock_count(), a.tools_open, a.tools_tab, cards::hidden_mask(), a.procs_open, preview::signature()));
    let apply_layout = |a: &AppState, nv: bool| {
        let mut boxes = Vec::new();
        let mut regions = cards::regions(a.cal_open, a.style.radius.max(1.0));
        if preview::active() {
            boxes.extend(preview::boxes());
            if let Some(r) = preview::region() {
                regions.push(r);
            }
        }
        if a.procs_open {
            boxes.extend(procs::boxes());
            regions.push(procs::region());
        }
        if a.tools_open {
            boxes.extend(tools::boxes(a.tools_tab));
            regions.push(tools::region());
        }
        if a.settings_open {
            boxes.extend(settings::boxes(a.settings_tab, a.settings_scroll));
            regions.push(settings::region());
        }
        if nv {
            // 通知条可见时才占据窗口区域(并可点击关闭)
            regions.push((cards::notify_rect().0, cards::notify_rect().1, cards::notify_rect().2, cards::notify_rect().3, 10.0));
        }
        if a.notif_open {
            boxes.extend(cards::notif_boxes());
            regions.push(cards::notif_region());
        }
        if a.selector_open {
            boxes.extend(cards::selector_boxes());
            regions.push(cards::selector_region());
        }
        if a.chrome_tactical {
            regions.extend(cards::tactical_regions());
        }
        if a.edit_layout {
            boxes.extend(cards::edit_boxes());
        } else {
            boxes.extend(cards::hit_boxes(a.cal_open));
        }
        if let Some(t) = cards::tooltip_rect() {
            regions.push(t);
        }
        if nv {
            let r = cards::notify_rect();
            boxes.insert(0, window::RectBox { id: "notify_card", x: r.0, y: r.1, w: r.2, h: r.3 });
        }
        update_hit_boxes(boxes);
        apply_window_region(hwnd, &regions);
    };
    apply_layout(&app, hub.notify_alpha() > 0.0);
    let mut last_layout = layout_key(&app, hub.notify_alpha() > 0.0);

    let mut spectrum = spectrum::Spectrum::new(&renderer.d2d_factory).ok();
    spectrum::start_capture();
    calendar::start_background();
    let mut last_cal_check = Instant::now();
    // Dock 运行状态:订阅窗口事件(回调经本线程消息循环投递),不轮询
    running::install();

    let mut msg = MSG::default();
    let mut last_1s = crate::log::ago(2);
    let mut last_hover: Option<&'static str> = None;
    let mut hover_since = Instant::now();
    let mut dirty = true;
    let mut last_desktop_check = crate::log::ago(5);
    // 桌面被整屏窗口盖住(或 HUD 被隐藏)时进入"静默":不绘制、不采样、不拉起子进程,CPU≈0
    let mut covered = false;

    while !SHOULD_EXIT.load(std::sync::atomic::Ordering::Relaxed) {
        unsafe {
            // 空闲时最多睡 50ms;有输入消息立即唤醒
            let wait = if covered {
                500
            } else {
                match (&ambient, &spectrum) {
                (Some(a), _) if a.active() => a.frame_ms().min(50) as u32,
                (_, Some(s)) if s.active() => 33,
                _ if hub.notify_alpha() > 0.0 => 33,
                _ if hub.cleaning() => 50,
                _ => 50,
                }
            };
            let _ = MsgWaitForMultipleObjects(None, false, wait, QS_ALLINPUT);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return Ok(());
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        for event in pop_click_events() {
            let mx = MOUSE_POS.lock().map(|p| p.0).unwrap_or(0.0);
            let my = MOUSE_POS.lock().map(|p| p.1).unwrap_or(0.0);
            cards::on_click(event, &mut app, &mut hub, &audio, (mx, my));
            // 拖动音量条时立刻读回,不用等每秒一次的刷新(否则看起来卡)
            vol = audio.get();
            dirty = true;
        }

        // Dock 右键菜单的动作
        let actions: Vec<(usize, u8)> = window::DOCK_ACTIONS.lock().map(|mut d| std::mem::take(&mut *d)).unwrap_or_default();
        for (idx, cmd) in actions {
            cards::handle_dock_action(idx, cmd);
            dirty = true;
        }
        // 日历事件开始前提醒:每 20 秒看一次(静默模式下也要提醒,所以放在 covered 判断之外)
        if last_cal_check.elapsed() >= Duration::from_secs(20) {
            last_cal_check = Instant::now();
            for e in calendar::due_notifications() {
                let mins = ((e.start - chrono::Local::now()).num_seconds() as f32 / 60.0).ceil().max(1.0) as i64;
                data::core(&["notify", &perch_data::t_fmt("misc.remind_title", &[("mins", &mins.to_string()), ("title", &e.title)]), &perch_data::t_fmt("misc.remind_body", &[("time", &e.start.format("%H:%M").to_string())])]);
            }
        }
        if !covered && running::tick() {
            dirty = true;
        }
        // 进程面板:打开期间每 1.5 秒采样一次
        if app.procs_open && !covered && procs::tick() {
            dirty = true;
        }

        // 拖到 HUD 上的文件 → Dock
        let dropped: Vec<String> = window::DROPPED.lock().map(|mut d| std::mem::take(&mut *d)).unwrap_or_default();
        for path in dropped {
            settings::add_dock_app(&path);
            dirty = true;
        }

        // 分辨率 / DPI / 显示器变化:重建窗口尺寸、渲染目标、覆盖层窗口和布局
        if RELAYOUT.swap(false, std::sync::atomic::Ordering::SeqCst) {
            preview::close();
            window.refresh_size();
            let sc = window::scale();
            cards::set_screen(window.width as f32 / sc, window.height as f32 / sc);
            renderer.reset_for_display(window.width, window.height);
            ambient = ambient::Ambient::new(&renderer.d2d_factory, &renderer.dwrite_factory, window.width as i32, window.height as i32).ok();
            spectrum = spectrum::Spectrum::new(&renderer.d2d_factory).ok();
            apply_layout(&app, hub.notify_alpha() > 0.0);
            last_layout = layout_key(&app, hub.notify_alpha() > 0.0);
            dirty = true;
            log::log(&format!("display changed: {}x{} @ {:.2}x", window.width, window.height, sc));
        }

        if layout_key(&app, hub.notify_alpha() > 0.0) != last_layout {
            last_layout = layout_key(&app, hub.notify_alpha() > 0.0);
            apply_layout(&app, hub.notify_alpha() > 0.0);
            dirty = true;
        }

        if TRAY_OPEN_SELECTOR.swap(false, std::sync::atomic::Ordering::SeqCst) {
            app.selector_open = true;
        }
        if TRAY_TOOLS.swap(false, std::sync::atomic::Ordering::SeqCst) {
            if app.tools_open {
                app.tools_open = false;
            } else {
                tools::open(&mut app);
            }
            dirty = true;
        }
        if TRAY_SETTINGS.swap(false, std::sync::atomic::Ordering::SeqCst) {
            app.settings_open = true;
            app.selector_open = false;
        }
        if TRAY_SEARCH.swap(false, std::sync::atomic::Ordering::SeqCst) {
            search::run(&app.theme);
            dirty = true;
        }
        if TRAY_FLOAT.swap(false, std::sync::atomic::Ordering::SeqCst) {
            let on = !app.float_mode;
            app.set_float(on);
            dirty = true;
        }
        if TRAY_FONTS.swap(false, std::sync::atomic::Ordering::SeqCst) {
            app.toggle_theme_fonts();
            dirty = true;
        }
        if tools::AUDIO_REBIND.swap(false, std::sync::atomic::Ordering::SeqCst) {
            audio.rebind();
            vol = audio.get();
            dirty = true;
        }
        if window::REHOTKEY.swap(false, std::sync::atomic::Ordering::SeqCst) {
            window::register_hotkeys(hwnd);
        }
        if TRAY_RELOAD.swap(false, std::sync::atomic::Ordering::SeqCst) {
            config::reload();
            window::register_hotkeys(hwnd);
            app.reload_theme();
            // Dock 数量/番茄钟预设可能变了:重建布局和覆盖层
            RELAYOUT.store(true, std::sync::atomic::Ordering::SeqCst);
            dirty = true;
        }

        let cleaning = hub.cleaning();
        let fading = hub.notify_alpha() > 0.0 || cleaning;
        if fading {
            dirty = true;
        }

        let hover = HOVERED_ID.lock().ok().and_then(|h| *h);
        if hover != last_hover {
            last_hover = hover;
            hover_since = Instant::now();
            cards::set_tip(None);
            if hover.map_or(false, |h| h.starts_with("dk")) {
                running::refresh_soon();
            }
            dirty = true;
        }
        if preview::update(hover, hover_since.elapsed()) {
            dirty = true;
        }
        // 预览弹出时收掉已经出现的文字提示,避免叠在一起
        if preview::active() && cards::current_tip().is_some() {
            cards::set_tip(None);
            dirty = true;
        }
        // 稳定悬停 0.45 秒后显示提示(预览弹出时不再显示)
        if hover.is_some() && !preview::active() && cards::current_tip().is_none() && hover_since.elapsed() >= Duration::from_millis(450) {
            cards::set_tip(hover);
            dirty = true;
        }

        if last_1s.elapsed() >= Duration::from_secs(1) {
            last_1s = Instant::now();
            // 番茄钟倒计时和主题监视很便宜,任何时候都要走
            app.tick_1s();
            app.reload_theme_if_changed();
            if covered {
                hub.tick_background();
            } else {
                hub.tick();
                sys = sampler.sample_all();
                disk_c = metrics::disk_gb(&disk_path(0));
                disk_d = metrics::disk_gb(&disk_path(1));
                vol = audio.get();
                gpu_info = gpu::sample();
                // GPU 过热提醒(先用便宜的固定下限挡掉绝大多数情况,再读配置)
                if let Some(g) = gpu_info {
                    if g.temp >= 50 {
                        let al = config::get().alert;
                        if al.gpu_temp > 0 && g.temp >= al.gpu_temp && last_gpu_alert.map_or(true, |t| t.elapsed() >= Duration::from_secs(al.cooldown_min.max(1) as u64 * 60)) {
                            last_gpu_alert = Some(Instant::now());
                            data::core(&["notify", &perch_data::t("misc.gpu_hot_title"), &perch_data::t_fmt("misc.gpu_hot_body", &[("t", &g.temp.to_string())])]);
                        }
                    }
                }
                dirty = true;
            }
        }

        // 桌面状态:Win+D 时把 HUD 浮到桌面之上;整屏窗口盖住桌面时进入静默
        let check_every = if covered { 500 } else { 300 };
        if last_desktop_check.elapsed() >= Duration::from_millis(check_every) {
            last_desktop_check = Instant::now();
            let (shown, cover) = desktop::desktop_state();
            set_lifted(window.hwnd, app.float_mode || shown);
            let now_covered = !window::hud_visible() || (!app.float_mode && cover && !shown);
            if now_covered != covered {
                covered = now_covered;
                net::PAUSED.store(covered, std::sync::atomic::Ordering::Relaxed);
                spectrum::PAUSED.store(covered, std::sync::atomic::Ordering::Relaxed);
                if covered {
                    if let Some(a) = ambient.as_mut() {
                        a.hide();
                    }
                    if let Some(s) = spectrum.as_mut() {
                        s.hide();
                    }
                    log::log("idle: desktop covered, rendering and sampling paused");
                } else {
                    // 回到桌面:马上刷新一遍所有数据并重绘
                    hub.refresh_soon();
                    last_1s = crate::log::ago(2);
                    dirty = true;
                    log::log("active: desktop visible again");
                }
            }
        }

        if !covered {
            if let Some(a) = ambient.as_mut() {
                a.configure(app.current_theme_name, &hub.ambient);
                a.tick();
            }

            if let Some(s) = spectrum.as_mut() {
                let rgb = |c: &theme::ColorRgba| (c.r, c.g, c.b);
                s.tick(rgb(&app.theme.accent), rgb(&app.theme.sub));
            }
        }

        if dirty && !covered {
            dirty = false;
            renderer.begin_draw()?;
            if renderer.font_num != app.style.font_num {
                renderer.font_num = app.style.font_num.clone();
            }
            let view = cards::View {
                th: &app.theme,
                st: &app.style,
                app: &app,
                hub: &hub,
                sys: &sys,
                disk_c,
                disk_d,
                vol,
                gpu: gpu_info,
                cleaning,
                hover,
            };
            cards::draw(&mut renderer, &view)?;
            renderer.end_draw()?;
        }
    }
    Ok(())
}
