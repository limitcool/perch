//! 亮度:笔记本内屏走 WMI,外接显示器走 DDC/CI(dxva2 高级监视器 API)。
//! 只在用户操作 / 打开面板时调用,不后台轮询。DDC/CI 单次调用可能要几十毫秒,所以外接屏只在松手时写入。

use std::sync::atomic::{AtomicI32, Ordering};
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

static INTERNAL: AtomicI32 = AtomicI32::new(-1);
static EXTERNAL: AtomicI32 = AtomicI32::new(-1);

/// 当前已知的亮度(-1 = 未知 / 没有这块屏)
pub fn internal() -> i32 {
    INTERNAL.load(Ordering::Relaxed)
}
pub fn external() -> i32 {
    EXTERNAL.load(Ordering::Relaxed)
}

/// 内屏走 WMI(root\WMI 的 WmiMonitorBrightness*,这是系统自己的亮度滑块用的接口);一次性起 powershell,仅在用户操作时调用
fn powershell(script: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", script]).creation_flags(0x0800_0000).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 后台读取两块屏的当前亮度
pub fn refresh_async() {
    std::thread::spawn(|| {
        let v = powershell("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue | Select-Object -First 1).CurrentBrightness").and_then(|s| s.parse::<i32>().ok()).unwrap_or(-1);
        INTERNAL.store(v, Ordering::Relaxed);
        EXTERNAL.store(external_get().map(|x| x as i32).unwrap_or(-1), Ordering::Relaxed);
    });
}

pub fn set_internal_async(percent: u8) {
    INTERNAL.store(percent as i32, Ordering::Relaxed);
    std::thread::spawn(move || {
        let _ = powershell(&format!("Invoke-CimMethod -InputObject (Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods | Select-Object -First 1) -MethodName WmiSetBrightness -Arguments @{{Timeout=1;Brightness=[byte]{}}} | Out-Null", percent));
    });
}

pub fn set_external_async(percent: u8) {
    EXTERNAL.store(percent as i32, Ordering::Relaxed);
    std::thread::spawn(move || external_set(percent));
}

// ---------------- 外接显示器 (DDC/CI) ----------------

unsafe extern "system" fn on_monitor(mon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<HMONITOR>);
    list.push(mon);
    BOOL(1)
}

/// 对每个物理显示器调用 f,f 返回 Some 就停止(内屏没有 DDC/CI,会自然被跳过)
fn each_physical<R>(mut f: impl FnMut(HANDLE) -> Option<R>) -> Option<R> {
    let mut mons: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(on_monitor), LPARAM(&mut mons as *mut _ as isize));
        for m in mons {
            let mut count = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(m, &mut count).is_err() || count == 0 {
                continue;
            }
            let mut phys = vec![PHYSICAL_MONITOR::default(); count as usize];
            if GetPhysicalMonitorsFromHMONITOR(m, &mut phys).is_err() {
                continue;
            }
            let mut result = None;
            for p in &phys {
                if let Some(r) = f(p.hPhysicalMonitor) {
                    result = Some(r);
                    break;
                }
            }
            let _ = DestroyPhysicalMonitors(&phys);
            if result.is_some() {
                return result;
            }
        }
    }
    None
}

fn external_get() -> Option<u8> {
    each_physical(|h| {
        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
        let ok = unsafe { GetMonitorBrightness(h, &mut min, &mut cur, &mut max) };
        if ok == 0 || max <= min {
            return None;
        }
        Some((((cur - min) as f32 / (max - min) as f32) * 100.0).round() as u8)
    })
}

fn external_set(percent: u8) {
    let _ = each_physical(|h| {
        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
        if unsafe { GetMonitorBrightness(h, &mut min, &mut cur, &mut max) } == 0 || max <= min {
            return None;
        }
        let v = min + ((max - min) as f32 * percent as f32 / 100.0).round() as u32;
        unsafe {
            SetMonitorBrightness(h, v);
        }
        Some(())
    });
}

#[cfg(test)]
mod tests {
    /// 硬件相关:只读,打印当前值(cargo test brightness -- --nocapture)
    #[test]
    fn read_current() {
        println!("external = {:?}", super::external_get());
    }

    /// 硬件相关:外接屏 ±1% 往返(cargo test brightness -- --ignored --nocapture)
    #[test]
    #[ignore]
    fn external_roundtrip() {
        let before = super::external_get().expect("no DDC/CI monitor");
        super::external_set((before + 1).min(100));
        std::thread::sleep(std::time::Duration::from_millis(800));
        let mid = super::external_get();
        super::external_set(before);
        std::thread::sleep(std::time::Duration::from_millis(800));
        println!("before={} mid={:?} after={:?}", before, mid, super::external_get());
    }
}
