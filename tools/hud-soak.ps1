# Long-run stability monitor for perch: samples handles / GDI / USER objects / memory every minute.
# Usage:  powershell -File perch\tools\hud-soak.ps1 [-Hours 24] [-IntervalSec 60]
# Output: %APPDATA%\perch\soak.csv   and a growth summary at the end (also: -Summary to just summarise an existing csv).
param([double]$Hours = 24, [int]$IntervalSec = 60, [switch]$Summary)
Add-Type @"
using System;using System.Runtime.InteropServices;
public class SK{ [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr h, uint f); }
"@
$csv = "$env:APPDATA\perch\soak.csv"
New-Item -ItemType Directory -Force (Split-Path $csv) | Out-Null
function Summarise {
    $rows = Import-Csv $csv
    if ($rows.Count -lt 3) { "not enough samples"; return }
    $first = $rows[0]; $last = $rows[-1]
    $hrs = [math]::Max(0.01, ([datetime]$last.time - [datetime]$first.time).TotalHours)
    foreach ($c in 'handles', 'gdi', 'user', 'private_mb', 'threads') {
        $d = [double]$last.$c - [double]$first.$c
        "{0,-11} first={1,8} last={2,8} growth={3,8:N1} ({4:N2}/h)" -f $c, $first.$c, $last.$c, $d, ($d / $hrs)
    }
    "samples: $($rows.Count) over {0:N1} h; restarts seen: {1}" -f $hrs, (($rows | Select-Object -ExpandProperty pid -Unique) | Measure-Object).Count
}
if ($Summary) { Summarise; return }
if (-not (Test-Path $csv)) { "time,pid,handles,gdi,user,private_mb,ws_mb,threads,cpu_s" | Set-Content $csv }
$end = (Get-Date).AddHours($Hours)
while ((Get-Date) -lt $end) {
    $p = Get-Process perch -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($p) {
        $gdi = [SK]::GetGuiResources($p.Handle, 0); $usr = [SK]::GetGuiResources($p.Handle, 1)
        "{0},{1},{2},{3},{4},{5},{6},{7},{8}" -f (Get-Date -Format s), $p.Id, $p.HandleCount, $gdi, $usr, [math]::Round($p.PrivateMemorySize64 / 1MB, 1), [math]::Round($p.WorkingSet64 / 1MB, 1), $p.Threads.Count, [math]::Round($p.CPU, 1) | Add-Content $csv
    }
    Start-Sleep -Seconds $IntervalSec
}
Summarise
