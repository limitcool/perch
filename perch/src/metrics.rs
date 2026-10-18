use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::*;
use windows::Win32::System::Threading::GetSystemTimes;

#[derive(Debug, Clone, Default)]
pub struct SystemMetrics {
    pub cpu_usage: f32,    // 0.0 ~ 100.0%
    pub ram_usage: f32,    // 0.0 ~ 100.0%
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub disk_c_usage: f32, // 0.0 ~ 100.0%
    pub disk_c_free_gb: f32,
    pub battery_percent: u8,
    pub battery_charging: bool,
    pub has_battery: bool,
}

pub struct MetricsSampler {
    last_idle_time: u64,
    last_kernel_time: u64,
    last_user_time: u64,
    metrics: SystemMetrics,
}

impl MetricsSampler {
    pub fn new() -> Self {
        let mut sampler = Self {
            last_idle_time: 0,
            last_kernel_time: 0,
            last_user_time: 0,
            metrics: SystemMetrics::default(),
        };
        sampler.sample_cpu();
        sampler.sample_all();
        sampler
    }

    pub fn sample_all(&mut self) -> SystemMetrics {
        self.sample_cpu();
        self.sample_ram();
        self.sample_disk();
        self.sample_power();
        self.metrics.clone()
    }

    fn sample_cpu(&mut self) {
        unsafe {
            let mut idle = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();

            if GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_ok() {
                let idle_t = filetime_to_u64(&idle);
                let kernel_t = filetime_to_u64(&kernel);
                let user_t = filetime_to_u64(&user);

                if self.last_kernel_time > 0 {
                    let total_kernel = kernel_t.saturating_sub(self.last_kernel_time);
                    let total_user = user_t.saturating_sub(self.last_user_time);
                    let total_idle = idle_t.saturating_sub(self.last_idle_time);
                    let total_sys = total_kernel + total_user;

                    if total_sys > 0 {
                        let used = total_sys.saturating_sub(total_idle);
                        self.metrics.cpu_usage = (used as f32 / total_sys as f32) * 100.0;
                    }
                }

                self.last_idle_time = idle_t;
                self.last_kernel_time = kernel_t;
                self.last_user_time = user_t;
            }
        }
    }

    fn sample_ram(&mut self) {
        unsafe {
            let mut mem = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };

            if GlobalMemoryStatusEx(&mut mem).is_ok() {
                self.metrics.ram_usage = mem.dwMemoryLoad as f32;
                let total = mem.ullTotalPhys as f32 / (1024.0 * 1024.0 * 1024.0);
                let avail = mem.ullAvailPhys as f32 / (1024.0 * 1024.0 * 1024.0);
                self.metrics.ram_total_gb = total;
                self.metrics.ram_used_gb = total - avail;
            }
        }
    }

    fn sample_disk(&mut self) {
        unsafe {
            let mut free_bytes = 0u64;
            let mut total_bytes = 0u64;
            let mut total_free = 0u64;

            let path: Vec<u16> = "C:\\\0".encode_utf16().collect();
            if GetDiskFreeSpaceExW(
                PCWSTR::from_raw(path.as_ptr()),
                Some(&mut free_bytes),
                Some(&mut total_bytes),
                Some(&mut total_free),
            )
            .is_ok()
                && total_bytes > 0
            {
                let free_gb = free_bytes as f32 / (1024.0 * 1024.0 * 1024.0);
                let total_gb = total_bytes as f32 / (1024.0 * 1024.0 * 1024.0);
                self.metrics.disk_c_free_gb = free_gb;
                self.metrics.disk_c_usage = ((total_gb - free_gb) / total_gb) * 100.0;
            }
        }
    }

    fn sample_power(&mut self) {
        unsafe {
            let mut status = SYSTEM_POWER_STATUS::default();
            if GetSystemPowerStatus(&mut status).is_ok() {
                self.metrics.has_battery = status.BatteryFlag != 128 && status.BatteryFlag != 255;
                self.metrics.battery_percent = if status.BatteryLifePercent <= 100 {
                    status.BatteryLifePercent
                } else {
                    100
                };
                self.metrics.battery_charging = (status.BatteryFlag & 8) != 0 || status.ACLineStatus == 1;
            }
        }
    }
}

fn filetime_to_u64(ft: &FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

/// 指定盘符的 (剩余 GB, 总量 GB)
pub fn disk_gb(root: &str) -> (f32, f32) {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
    let (mut free, mut total, mut _tf) = (0u64, 0u64, 0u64);
    unsafe {
        if GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), Some(&mut total), Some(&mut _tf)).is_ok() {
            return (free as f32 / 1073741824.0, total as f32 / 1073741824.0);
        }
    }
    (0.0, 0.0)
}
