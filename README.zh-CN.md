<p align="center"><img src="docs/images/logo.png" alt="Perch logo" width="96"></p>

<p align="center"><a href="README.md">English</a> | <b>简体中文</b></p>

# Perch

一个纯 Rust(Direct2D)编写的 Windows 10/11 桌面悬浮面板:桌面卡片、带窗口预览的 Dock、快速启动器和小工具箱。
核心诉求是**低占用**:专用工作集约 40 MB,桌面被窗口盖住时几乎不占 CPU。

![Perch 在中性壁纸上的效果](docs/images/hero.png)

## 功能

- **卡片** — 番茄钟、日历(ICS 订阅)、待办(可选与 Obsidian 日记双向同步)、媒体(Windows 媒体会话)、喝水提醒、天气、系统监控、电量、语录。
- **Dock** — 运行中程序检测(事件驱动,无轮询)、悬停窗口预览、每个入口可设单开/多开。
- **启动器** — 程序 / 文件(装了 [Everything](https://www.voidtools.com/) 即可用)/ 窗口 / 问 AI / 系统命令,按使用频率排序。
- **工具箱** — 剪贴板历史、收集箱、快速设置(亮度、音频输出设备、Clash 控制器)。
- **主题** — 数据驱动的主题包,支持热重载;音频频谱和环境粒子(被窗口盖住时自动暂停)。
- **界面语言** — 英文和简体中文(`[ui] language`,默认跟随系统)。
- **一切都是可选的** — 不需要壁纸、Obsidian、Clash 或任何特殊硬件。

![内置主题:default、aurora、ember、neon](docs/images/themes.png)

## 安装

### 下载(推荐)

从 [Releases](../../releases) 页面下载 `perch-windows-x64.zip`,解压到任意位置,运行 `perch.exe`。
HUD 需要的所有文件会在首次运行时创建在可执行文件旁边(`config.toml`、`@Resources\...`)。

可执行文件没有代码签名,首次运行时 Windows SmartScreen 可能提示风险:点击**更多信息 → 仍要运行**即可。

### 从源码构建

需要 Windows 10/11 上的 Rust(stable,MSVC 工具链)。

```powershell
cd perch-core; cargo build --release
cd ..\perch; cargo build --release
```

也可以运行 `tools\deploy-hud.ps1`:它会构建两个 crate 并安装到 `%LOCALAPPDATA%\perch`
(`-InstallDir <目录>` 指定其他位置,`-ThemePacks <目录>` 复制额外的主题包,`-NoStart` 安装后不启动)。

```
<安装目录>\perch.exe
<安装目录>\config.toml                 首次运行时生成
<安装目录>\@Resources\bin\perch-core.exe
<安装目录>\@Resources\{data,weather,battery,...}   运行时创建
<安装目录>\themes\*.toml               可选的额外主题包
```

用户状态保存在 `%APPDATA%\perch`。开机自启由你自己决定(计划任务或"启动"文件夹快捷方式)。
运行根目录优先取可执行文件所在位置,其次取环境变量 `PERCH_HOME`。

## 首次运行

启动 `perch.exe`。它会生成带合理默认值的 `config.toml`,拉取一次天气和电量,生成默认主题,
并把卡片显示在主显示器上。右键托盘图标可以打开设置、主题选择、显示器选择和"重新载入配置"。
当有窗口盖住整个显示器时 HUD 会自动隐藏,显示桌面(`Win+D`)后恢复。

默认快捷键:`Win+Alt+H` 显示/隐藏,`Win+Alt+L` 浮窗/桌面小部件切换,`Win+Alt+S` 启动器,`Win+Alt+V` 工具箱。

## 配置

`config.toml` 可以在托盘菜单里重新载入。主要的段有 `[ui]`、`[hotkeys]`、`[alert]`、`[widgets]`、
`[calendar]`、`[notes]`(Obsidian)、`[audio]`、`[pomodoro]`、`[system]`、`[display]`、`[[dock]]`、`[music_assistant]`、
`[thresholds]`、`[intervals]`、`[dock_settings]`。生成的文件里每个选项都有注释。

### 电量卡片

默认显示笔记本电池;没有电池的台式机上,这张卡片会自动隐藏。
想改成显示无线鼠标/键盘(或同时显示),添加一个或多个 `[[battery]]` 来源,取第一个读到数值的:

```toml
[[battery]]
kind = "hid"            # 读取无线设备通过 HID 上报的电量
name = "My Mouse"
vendor_id = 0x1234
product_id = 0x5678
usage_page = 0xFF00     # 以下为默认值
usage = 2
battery_byte = 6        # 电量百分比在输入报告的第几个字节

[[battery]]
kind = "system"         # 回退到笔记本电池
```

找到这些数字:运行 `perch-core.exe battery list`,它会写出并打开 `@Resources\data\hid_devices.txt`,列出本机所有 HID 设备(厂商自定义的排在前面)。
再运行 `perch-core.exe battery dump 0x1234 0x5678`,会抓几份输入报告写进 `@Resources\data\hid_dump.txt`,
随电量变化的那个字节就是你的 `battery_byte`。各厂商协议不同,并非所有设备都能这样读到电量;
如果你的设备不行,欢迎提 issue 并附上这份 dump。

## 主题包

一个主题就是一个 TOML 文件。内置 8 个主题包(`default`、`aurora`、`orchid`、`crimson`、`neon`、
`lavender`、`rose`、`ember`)。要新增或覆盖,把 `<id>.toml` 放进 `<安装目录>\themes\`,
无需重新构建就会出现在主题选择里。缺失的字段会回退到 `default` 主题。

```toml
v = 2
id = "mytheme"
order = 100                # 在选择器里的排序
replaces = ""              # 可选:本主题包在选择器里取代的内置主题 id

[meta]    name = "My Theme"; description = "..."; title = "..."; preview = [66, 133, 244]
[colors]  accent / sub / bg / dock_bg / stroke / item_bg / item_hover / pill_bg / white / muted / on_accent / on_sub / danger / warn / ok
[style]   font_main / font_num / radius / frame_stroke / frame_width / shape2 / corner / bar_h / seg_color
          chrome = "none" | "tactical"     # tactical 会在顶部增加状态栏(chrome_title / chrome_sub / chrome_badges)
[labels]    dock_title / field_name / unit_name / title_* / sub_* / lbl_*
[labels_en] 键与 [labels] 相同;界面语言为英文时代替 [labels](只需填写不同的项)
[fx]        kind = "dot" | "petal" | "diamond" | "heart" | "streak"; rgb; size_*; speed_*; up; sway; alpha_*
[wallpaper] prefix = "mytheme"
[messages]  water = "..."
[quotes]    speaker = "..."; lines = ["原文#CRLF#可选的译文"]
```

完整示例见 `perch-data/builtin/*.toml`。切换界面语言后,重新选择一次主题,标签才会按新语言重新生成。

## 壁纸

不附带壁纸。如需按主题换壁纸,把 `<prefix>.png`(或 `<prefix>_day_N.png` / `<prefix>_night_N.png`)
放进 `@Resources\wallpapers`。没有这些文件时,HUD 不会动你的壁纸。

## 常见问题

- **什么都没显示** — 可能被其他窗口盖住了整个显示器,按 `Win+D`;检查 `config.toml` 里的 `[display] monitor`。
- **再启动一次没反应** — 同一时间只会运行一个 HUD,先退出托盘里的那个。
- **文件搜索提示"没有找到 Everything"** — 安装并启动 Everything(启动器的文件模式需要它)。
- **日志** — `%APPDATA%\perch\hud.log`(也可在 设置 → 关于 里打开)。

## 仓库结构

- `perch/`  HUD 本体(薄客户端:负责渲染和输入)
- `perch-core/` 命令行工具和后台任务(天气、电量、主题、喝水提醒、壁纸同步、媒体)
- `perch-data/` 共享的数据模型、路径、主题包、界面文字、TOML/JSON 原子读写
- `tools/`      部署 / 冒烟 / 稳定性观测脚本

CI 会构建每一次推送,并在每个 `v*` 标签上附带 Windows 压缩包(见 `.github/workflows`)。

## 许可证

MIT,见 [LICENSE](LICENSE)。
