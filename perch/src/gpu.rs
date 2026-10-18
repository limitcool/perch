//! NVIDIA GPU 占用 / 温度 / 显存(动态加载驱动自带的 nvml.dll;没有 NVIDIA 显卡时静默返回 None)。

use std::sync::OnceLock;
use windows::core::{s, PCSTR, PCWSTR};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

#[derive(Clone, Copy, Debug)]
pub struct GpuInfo {
    pub util: u32,
    pub temp: u32,
    pub mem_used_mb: u32,
    pub mem_total_mb: u32,
}

#[repr(C)]
#[derive(Default)]
struct Util {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct Mem {
    total: u64,
    free: u64,
    used: u64,
}

type FnInit = unsafe extern "C" fn() -> i32;
type FnHandle = unsafe extern "C" fn(u32, *mut *mut core::ffi::c_void) -> i32;
type FnUtil = unsafe extern "C" fn(*mut core::ffi::c_void, *mut Util) -> i32;
type FnTemp = unsafe extern "C" fn(*mut core::ffi::c_void, u32, *mut u32) -> i32;
type FnMem = unsafe extern "C" fn(*mut core::ffi::c_void, *mut Mem) -> i32;

struct Nvml {
    handle: *mut core::ffi::c_void,
    util: FnUtil,
    temp: FnTemp,
    mem: FnMem,
}
unsafe impl Send for Nvml {}
unsafe impl Sync for Nvml {}

fn load() -> Option<Nvml> {
    unsafe {
        let candidates = [r"nvml.dll", r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll"];
        let mut lib = None;
        for c in candidates {
            let wide: Vec<u16> = c.encode_utf16().chain(Some(0)).collect();
            if let Ok(h) = LoadLibraryW(PCWSTR(wide.as_ptr())) {
                lib = Some(h);
                break;
            }
        }
        let lib = lib?;
        let sym = |name: PCSTR| GetProcAddress(lib, name);
        let init: FnInit = std::mem::transmute(sym(s!("nvmlInit_v2"))?);
        let get_handle: FnHandle = std::mem::transmute(sym(s!("nvmlDeviceGetHandleByIndex_v2"))?);
        let util: FnUtil = std::mem::transmute(sym(s!("nvmlDeviceGetUtilizationRates"))?);
        let temp: FnTemp = std::mem::transmute(sym(s!("nvmlDeviceGetTemperature"))?);
        let mem: FnMem = std::mem::transmute(sym(s!("nvmlDeviceGetMemoryInfo"))?);
        if init() != 0 {
            return None;
        }
        let mut handle = std::ptr::null_mut();
        if get_handle(0, &mut handle) != 0 || handle.is_null() {
            return None;
        }
        Some(Nvml { handle, util, temp, mem })
    }
}

fn nvml() -> &'static Option<Nvml> {
    static N: OnceLock<Option<Nvml>> = OnceLock::new();
    N.get_or_init(load)
}

pub fn sample() -> Option<GpuInfo> {
    let n = nvml().as_ref()?;
    unsafe {
        let mut u = Util::default();
        let mut t = 0u32;
        let mut m = Mem::default();
        if (n.util)(n.handle, &mut u) != 0 {
            return None;
        }
        let _ = (n.temp)(n.handle, 0, &mut t);
        let _ = (n.mem)(n.handle, &mut m);
        Some(GpuInfo { util: u.gpu, temp: t, mem_used_mb: (m.used / 1048576) as u32, mem_total_mb: (m.total / 1048576) as u32 })
    }
}
