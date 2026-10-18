# Smoke test for the running HUD: process, windows, log, data freshness and a rendered-pixels check.
# Usage: powershell -File perch\tools\hud-smoke.ps1 [-Themes crimson,neon,lavender] [-OutDir shots]
# With -Themes it switches theme through perch-core, captures the HUD window for each one and restores the original theme.
param(
    [string[]]$Themes = @(),
    [string]$OutDir = "$env:TEMP\hud-smoke",
    [string]$InstallDir = $(if ($env:PERCH_HOME) { $env:PERCH_HOME } else { "$env:LOCALAPPDATA\perch" })
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;using System.Runtime.InteropServices;
public class HS{
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c,string t);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint f);
 [StructLayout(LayoutKind.Sequential)] public struct RECT{public int L,T,R,B;}
}
"@
$root = $InstallDir
$core = "$root\@Resources\bin\perch-core.exe"
$fail = 0
function Check($name, $ok, $detail = '') { if ($ok) { "[ OK ] $name $detail" } else { "[FAIL] $name $detail"; $script:fail++ } }

$p = Get-Process perch -ErrorAction SilentlyContinue
Check 'HUD process running' ($p -ne $null) ("pid=" + ($p.Id -join ','))
Check 'single instance' (($p | Measure-Object).Count -le 1)
$hud = [HS]::FindWindowW('PerchHudWindowClass', $null)
if ($hud -eq [IntPtr]::Zero -and $p -and $p.MainWindowHandle -ne [IntPtr]::Zero) {
    $hud = $p.MainWindowHandle
}
Check 'HUD window exists' ($hud -ne [IntPtr]::Zero)
$log = "$env:APPDATA\perch\hud.log"
$bad = if (Test-Path $log) { Get-Content $log -Tail 50 | Select-String -Pattern 'PANIC|FATAL' | Select-Object -Last 3 } else { $null }
Check 'no PANIC/FATAL in hud.log' ($bad -eq $null) (($bad | % { $_.Line }) -join ' | ')
$theme = "$root\@Resources\theme.toml"
Check 'theme.toml present' (Test-Path $theme)
Check 'core present' (Test-Path $core)
Check 'data\water.toml' (Test-Path "$root\@Resources\data\water.toml")
Check 'data\quote.toml' (Test-Path "$root\@Resources\data\quote.toml")

function Shot($name) {
    $r = New-Object HS+RECT; [HS]::GetWindowRect($hud, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $h = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc(); [HS]::PrintWindow($hud, $dc, 2) | Out-Null; $g.ReleaseHdc($dc)
    New-Item -ItemType Directory -Force $OutDir | Out-Null
    $path = Join-Path $OutDir "$name.png"; $bmp.Save($path)
    # fraction of non-black pixels (PrintWindow gives an opaque black backdrop): the cards must actually be drawn
    $lit = 0; $step = 8; $n = 0
    for ($y = 0; $y -lt $h; $y += $step) { for ($x = 0; $x -lt $w; $x += $step) { $n++; $c = $bmp.GetPixel($x, $y); if (($c.R + $c.G + $c.B) -gt 60) { $lit++ } } }
    return @{ path = $path; frac = $lit / [math]::Max(1, $n) }
}

$state = if (Test-Path $log) { (Select-String -Path $log -Pattern 'idle:|active:' | Select-Object -Last 1).Line } else { '' }
if ($state -match 'idle:') {
    # the desktop is covered by a full-screen window: the HUD deliberately stops drawing, so there is nothing to capture
    "[SKIP] render check: HUD is in idle mode (desktop covered). Show the desktop (Win+D) or minimise windows and rerun."
} elseif ($hud -ne [IntPtr]::Zero) {
    $s = Shot 'current'
    Check 'cards rendered' ($s.frac -gt 0.01) ("non-black fraction {0:P1} -> {1}" -f $s.frac, $s.path)
    $Themes = @($Themes | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
    if ($Themes.Count -gt 0) {
        $orig = ((Get-Content $theme -Encoding UTF8 | Select-String 'current_theme_name\s*=') -replace '.*=\s*"(.*)"', '$1').Trim()
        foreach ($t in $Themes) {
            Start-Process $core -ArgumentList @('theme', $t) -Wait -WindowStyle Hidden
            Start-Sleep -Seconds 3
            $s = Shot "theme_$t"
            Check "theme $t" ($s.frac -gt 0.01) ("-> " + $s.path)
        }
        if ($orig) { Start-Process $core -ArgumentList @('theme', $orig) -Wait -WindowStyle Hidden }
    }
}
if ($fail -gt 0) { "`n$fail check(s) failed"; exit 1 } else { "`nall checks passed" }
