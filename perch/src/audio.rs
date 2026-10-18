//! 系统主音量(Core Audio)。只在用户点击时设置,读取每秒一次。

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};

pub struct Audio {
    vol: std::cell::RefCell<Option<IAudioEndpointVolume>>,
}

impl Audio {
    pub fn new() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let vol = (|| -> windows::core::Result<IAudioEndpointVolume> {
                let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
                dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            })()
            .ok();
            Self { vol: std::cell::RefCell::new(vol) }
        }
    }

    /// 默认输出设备被切换后,重新绑定到新设备的音量控制
    pub fn rebind(&self) {
        let v = unsafe {
            (|| -> windows::core::Result<IAudioEndpointVolume> {
                let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
                dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            })()
            .ok()
        };
        *self.vol.borrow_mut() = v;
    }

    /// (音量百分比 0..100, 是否静音)
    pub fn get(&self) -> (f32, bool) {
        match &*self.vol.borrow() {
            Some(v) => unsafe {
                let level = v.GetMasterVolumeLevelScalar().unwrap_or(0.0);
                let mute = v.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                (level * 100.0, mute)
            },
            None => (0.0, false),
        }
    }

    pub fn set(&self, pct: f32) {
        if let Some(v) = &*self.vol.borrow() {
            unsafe {
                let _ = v.SetMasterVolumeLevelScalar((pct / 100.0).clamp(0.0, 1.0), std::ptr::null());
            }
        }
    }

    pub fn step(&self, delta: f32) {
        let (p, _) = self.get();
        self.set((p + delta).clamp(0.0, 100.0));
    }

    pub fn toggle_mute(&self) {
        if let Some(v) = &*self.vol.borrow() {
            unsafe {
                let m = v.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                let _ = v.SetMute(!m, std::ptr::null());
            }
        }
    }
}

// ---------------- 输出设备列表 / 切换默认设备 ----------------

use std::ffi::c_void;
use windows::core::PCWSTR;
use windows_core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT};

/// 未公开但稳定的系统接口(PolicyConfig,Win7 起;各种音频切换工具都用它)
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, device: PCWSTR, a: *mut c_void, b: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, a: *mut i64, b: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, a: *mut i64) -> HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, device: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
    fn SetPropertyValue(&self, device: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: i32) -> HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
}

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

#[derive(Clone, Debug)]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn list_outputs() -> Vec<OutputDevice> {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE;
    use windows::Win32::System::Com::{CoTaskMemFree, STGM_READ};
    let mut out = Vec::new();
    unsafe {
        let mut run = || -> windows::core::Result<()> {
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let default_id = en.GetDefaultAudioEndpoint(eRender, eMultimedia).and_then(|d| d.GetId()).map(|p| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const c_void));
                s
            });
            let coll = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            for i in 0..coll.GetCount()? {
                let dev = coll.Item(i)?;
                let idp = dev.GetId()?;
                let id = idp.to_string().unwrap_or_default();
                CoTaskMemFree(Some(idp.0 as *const c_void));
                let store = dev.OpenPropertyStore(STGM_READ)?;
                let pv = store.GetValue(&PKEY_Device_FriendlyName)?;
                let name = windows::core::BSTR::try_from(&pv).map(|b| b.to_string()).unwrap_or_default();
                out.push(OutputDevice { is_default: default_id.as_ref().map_or(false, |d| *d == id), id, name });
            }
            Ok(())
        };
        let _ = run();
    }
    out
}

/// 把某个设备设为默认输出(播放 / 通信 / 控制台三种角色都切)
pub fn set_default_output(id: &str) -> bool {
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let Ok(pc) = CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) else { return false };
        let mut ok = true;
        for role in 0..3 {
            ok &= pc.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role).is_ok();
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    /// 只读:列出输出设备(需要 COM:测试线程里先初始化)。cargo test audio -- --nocapture
    #[test]
    fn list_outputs_readonly() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let devs = super::list_outputs();
        for d in &devs {
            println!("{} default={} id={}", d.name, d.is_default, d.id);
        }
        assert!(devs.iter().filter(|d| d.is_default).count() <= 1);
    }
}
