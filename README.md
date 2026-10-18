<p align="center"><img src="docs/images/logo.png" alt="Perch logo" width="96"></p>

# Perch

A lightweight, pure-Rust (Direct2D) desktop HUD for Windows 10/11 — floating cards on your desktop, an app dock with
window previews, a quick launcher and a small toolbox. Built for **low CPU and RAM**: about 40 MB private working set,
and almost no CPU while the desktop is covered by windows.

一个纯 Rust(Direct2D)的 Windows 桌面悬浮面板:桌面卡片、带窗口预览的 Dock、启动器、工具箱。
核心诉求是低占用:专用工作集约 40 MB,桌面被窗口盖住时几乎不占 CPU。

![Perch on a neutral wallpaper](docs/images/hero.png)

## Features / 功能

- **Cards** — pomodoro timer, calendar (ICS subscriptions), todo (optional two-way Obsidian daily-note sync),
  media (Windows media session), water reminder, weather, system monitor, battery, quotes.
- **Dock** — running-app detection (event driven, no polling), hover window previews, per-entry single/multi launch.
- **Launcher** — apps / files (via [Everything](https://www.voidtools.com/) if installed) / windows / ask-AI / system commands, frecency ranking.
- **Toolbox** — clipboard history, inbox, quick settings (brightness, audio output device, Clash controller).
- **Themes** — data-driven theme packs with hot reload, audio spectrum and ambient particles (paused while covered).
- **UI languages** — English and Simplified Chinese (`[ui] language`, default follows the system).
- **Everything is optional** — no wallpapers, Obsidian, Clash or special hardware are needed.

![Built-in themes: default, aurora, ember, neon](docs/images/themes.png)

## Install / 安装

### Download (recommended)

Grab `perch-windows-x64.zip` from the [Releases](../../releases) page, extract it anywhere and run `perch.exe`.
Everything the HUD needs is created next to the executable on first run (`config.toml`, `@Resources\...`).

### Build from source

Requires Rust (stable, MSVC toolchain) on Windows 10/11.

```powershell
cd perch-core; cargo build --release
cd ..\perch; cargo build --release
```

Or run `tools\deploy-hud.ps1`: it builds both crates and installs them to `%LOCALAPPDATA%\perch`
(`-InstallDir <dir>` chooses another folder, `-ThemePacks <dir>` copies extra theme packs, `-NoStart` skips launching).

```
<install dir>\perch.exe
<install dir>\config.toml                 created on first run
<install dir>\@Resources\bin\perch-core.exe
<install dir>\@Resources\{data,weather,battery,...}   created at runtime
<install dir>\themes\*.toml               optional extra theme packs
```

User state lives in `%APPDATA%\perch`. Autostart is up to you (a scheduled task or a Startup shortcut).
The runtime root is resolved from the executable location, then the `PERCH_HOME` environment variable.

## First run / 首次运行

Start `perch.exe`. It creates `config.toml` with sensible defaults, fetches weather and battery once, generates the
default theme and shows the cards on your primary monitor. Right-click the tray icon for settings, theme selector,
monitor selection and "Reload Configuration". The HUD hides while a window covers the whole monitor and returns when you
show the desktop (`Win+D`).

Default hotkeys: `Win+Alt+H` show/hide, `Win+Alt+L` float/widget, `Win+Alt+S` launcher, `Win+Alt+V` toolbox.

## Configuration / 配置

`config.toml` is reloaded from the tray menu. Sections include `[ui]`, `[hotkeys]`, `[alert]`, `[widgets]`,
`[calendar]`, `[notes]` (Obsidian), `[audio]`, `[pomodoro]`, `[system]`, `[display]`, `[[dock]]`, `[music_assistant]`,
`[thresholds]`, `[intervals]`, `[dock_settings]`. Every option is commented in the generated file.

Optional wireless-mouse battery (HID) — leave it out if you do not need it:

```toml
[mouse]
name = "My Mouse"
vendor_id = 0x1234      # see Device Manager -> Details -> Hardware Ids
product_id = 0x5678
usage_page = 0xFF00     # defaults shown
usage = 2
battery_byte = 6
```

## Theme packs / 主题包

A theme is one TOML file. Eight packs are built in (`default`, `aurora`, `orchid`, `crimson`, `neon`,
`lavender`, `rose`, `ember`). To add or override one, put `<id>.toml` into `<install dir>\themes\`;
it appears in the theme selector without rebuilding. Missing fields fall back to the `default` theme.

```toml
v = 2
id = "mytheme"
order = 100                # position in the selector
replaces = ""              # optional: id of a built-in theme this pack replaces in the selector

[meta]    name = "My Theme"; description = "..."; title = "..."; preview = [66, 133, 244]
[colors]  accent / sub / bg / dock_bg / stroke / item_bg / item_hover / pill_bg / white / muted / on_accent / on_sub / danger / warn / ok
[style]   font_main / font_num / radius / frame_stroke / frame_width / shape2 / corner / bar_h / seg_color
          chrome = "none" | "tactical"     # tactical adds a top status bar (chrome_title / chrome_sub / chrome_badges)
[labels]    dock_title / field_name / unit_name / title_* / sub_* / lbl_*
[labels_en] the same keys; used instead of [labels] when the UI language is English (only fill what differs)
[fx]        kind = "dot" | "petal" | "diamond" | "heart" | "streak"; rgb; size_*; speed_*; up; sway; alpha_*
[wallpaper] prefix = "mytheme"
[messages]  water = "..."
[quotes]    speaker = "..."; lines = ["Original line#CRLF#Optional translation"]
```

See `perch-data/builtin/*.toml` for complete examples. After changing the UI language, re-select a theme once so its
labels are regenerated.

## Wallpapers / 壁纸

Not included. For per-theme wallpapers put `<prefix>.png` (or `<prefix>_day_N.png` / `<prefix>_night_N.png`)
into `@Resources\wallpapers`. Without such files the HUD leaves your wallpaper untouched.

## Troubleshooting / 常见问题

- **Nothing shows up** — another window may cover the monitor; press `Win+D`. Check `[display] monitor` in `config.toml`.
- **Second instance does nothing** — only one HUD runs at a time; exit the tray icon first.
- **Files search says "Everything not found"** — install and start Everything (the launcher's file mode needs it).
- **Logs** — `%APPDATA%\perch\hud.log` (also reachable from Settings → About).

## Repository layout

- `perch/`  the HUD (thin client: rendering and input)
- `perch-core/` command-line tool and background jobs (weather, battery, themes, water reminder, wallpaper sync, media)
- `perch-data/` shared data models, paths, theme packs, UI strings, atomic TOML/JSON I/O
- `tools/`      deploy / smoke / soak scripts

CI builds every push and attaches a Windows zip to each `v*` tag (see `.github/workflows`).

## License

MIT, see [LICENSE](LICENSE).
