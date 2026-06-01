use nvml_wrapper::Nvml;

#[derive(serde::Serialize, Debug)]
pub struct GpuMetrics {
    pub gpu_vendor: String,
    pub gpu_name: String,
    pub gpu_temperature_celsius: f32,
    pub gpu_usage_percent: f32,
    pub gpu_memory_used_mb: u64,
    pub gpu_memory_total_mb: u64,
}

impl GpuMetrics {
    fn none() -> Self {
        GpuMetrics {
            gpu_vendor: "none".to_string(),
            gpu_name: "N/A".to_string(),
            gpu_temperature_celsius: 0.0,
            gpu_usage_percent: 0.0,
            gpu_memory_used_mb: 0,
            gpu_memory_total_mb: 0,
        }
    }
}

pub struct GpuHandle {
    inner: GpuBackend,
}

enum GpuBackend {
    Nvidia(Nvml),
    Amd(AmdGpu),
    None,
}

impl GpuHandle {
    pub fn init() -> Self {
        if let Ok(nvml) = Nvml::init() {
            let count = nvml.device_count().unwrap_or(0);
            if count > 0 {
                if let Ok(_device) = nvml.device_by_index(0) {
                    if _device.name().is_ok() {
                        return GpuHandle { inner: GpuBackend::Nvidia(nvml) };
                    }
                }
            }
        }
        if let Some(amd) = AmdGpu::init() {
            return GpuHandle { inner: GpuBackend::Amd(amd) };
        }
        GpuHandle { inner: GpuBackend::None }
    }

    // pub fn print_debug(&self) {
    //     match &self.inner {
    //         GpuBackend::Nvidia(nvml) => {
    //             match nvml.device_count() {
    //                 Ok(n) => {}
    //                 Err(e) => {}
    //             }
    //             match nvml.device_by_index(0) {
    //                 Ok(dev) => {
    //                     match dev.memory_info() {
    //                         Ok(m) => {}
    //                         Err(e) => {}
    //                     }
    //                     match dev.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu) {
    //                         Ok(t) => {}
    //                         Err(e) => {}
    //                     }
    //                     match dev.utilization_rates() {
    //                         Ok(u) => {}
    //                         Err(e) => {}
    //                     }
    //                 }
    //                 Err(e) => {}
    //             }
    //         }
    //         GpuBackend::Amd(amd) => {
    //             #[cfg(target_os = "linux")]
    //             if let AmdBackend::Sysfs(s) = &amd.inner {
    //                 match s.read() {
    //                     Some(info) => {}
    //                     None => {}
    //                 }
    //             }
    //             #[cfg(target_os = "windows")]
    //             if let AmdBackend::Wmi = &amd.inner {
    //                 match wmi_amd_info() {
    //                     Some(info) => {}
    //                     None => {}
    //                 }
    //             }
    //         }
    //         GpuBackend::None => {
    //             #[cfg(target_os = "linux")]
    //             Self::probe_sysfs_amd();
    //         }
    //     }
    // }

    #[cfg(target_os = "linux")]
    fn probe_sysfs_amd() {
        use std::{fs, path::Path};

        let drm = Path::new("/sys/class/drm");
        if !drm.exists() {
            return;
        }

        let mut entries = match fs::read_dir(drm) {
            Ok(e) => {
                let mut v: Vec<_> = e.filter_map(|x| x.ok()).collect();
                v.sort_by_key(|e| e.file_name());
                v
            }
            Err(e) => {
                return;
            }
        };

        let read = |p: &std::path::Path| -> String {
            fs::read_to_string(p)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|e| format!("ERROR: {}", e))
        };

        let mut found_any_card = false;

        for entry in &entries {
            let fname = entry.file_name();
            let fname = fname.to_string_lossy();

            if !fname.starts_with("card") { continue; }
            if fname.chars().skip(4).any(|c| !c.is_ascii_digit()) { continue; }

            found_any_card = true;
            let device_path = entry.path().join("device");

            if !device_path.exists() {
                continue;
            }

            let vendor = read(&device_path.join("vendor"));

            if vendor.to_ascii_lowercase() != "0x1002" {
                continue;
            }

            let busy_path = device_path.join("gpu_busy_percent");
            if busy_path.exists() {} else {}

            let uevent = read(&device_path.join("uevent"));
            let product = uevent.lines()
                .find(|l| l.starts_with("PRODUCT="))
                .unwrap_or("(no PRODUCT= line)");

            match fs::read_dir(device_path.join("hwmon")) {
                Err(e) => {}
                Ok(mut rd) => match rd.next().and_then(|e| e.ok()) {
                    None => {}
                    Some(hw) => {
                        let hw = hw.path();
                    }
                },
            }
        }

        if !found_any_card {}
    }

    pub fn collect(&self) -> GpuMetrics {
        match &self.inner {
            GpuBackend::Nvidia(nvml) => nvidia_metrics(nvml).unwrap_or_else(GpuMetrics::none),
            GpuBackend::Amd(amd) => amd_metrics(amd).unwrap_or_else(GpuMetrics::none),
            GpuBackend::None => GpuMetrics::none(),
        }
    }

    pub fn handshake_info(&self) -> (String, String, u64) {
        match &self.inner {
            GpuBackend::Nvidia(nvml) => {
                let device = match nvml.device_by_index(0) {
                    Ok(d) => d,
                    Err(_) => return ("nvidia".into(), "NVIDIA GPU".into(), 0),
                };
                let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".into());
                let total_mb = device.memory_info()
                    .map(|m| m.total / (1024 * 1024))
                    .unwrap_or(0);
                ("nvidia".into(), name, total_mb)
            }
            GpuBackend::Amd(amd) => {
                let info = amd.info().unwrap_or_else(|| AmdGpuInfo {
                    name: "AMD GPU".into(),
                    temperature_celsius: 0.0,
                    usage_percent: 0.0,
                    memory_used_mb: 0,
                    memory_total_mb: 0,
                });
                ("amd".into(), info.name, info.memory_total_mb)
            }
            GpuBackend::None => ("none".into(), "N/A".into(), 0),
        }
    }
}

fn nvidia_metrics(nvml: &Nvml) -> Option<GpuMetrics> {
    let device = nvml.device_by_index(0).ok()?;

    let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".into());

    let temperature = device
        .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
        .map(|t| t as f32)
        .unwrap_or(0.0);

    let usage_percent = device
        .utilization_rates()
        .map(|u| u.gpu as f32)
        .unwrap_or(0.0);

    let (memory_used_mb, memory_total_mb) = device.memory_info()
        .map(|m| (m.used / (1024 * 1024), m.total / (1024 * 1024)))
        .unwrap_or((0, 0));

    Some(GpuMetrics {
        gpu_vendor: "nvidia".into(),
        gpu_name: name,
        gpu_temperature_celsius: temperature,
        gpu_usage_percent: usage_percent,
        gpu_memory_used_mb: memory_used_mb,
        gpu_memory_total_mb: memory_total_mb,
    })
}

struct AmdGpu {
    inner: AmdBackend,
}

#[allow(dead_code)]
enum AmdBackend {
    #[cfg(target_os = "linux")]
    Sysfs(SysfsAmdGpu),
    #[cfg(target_os = "windows")]
    Wmi,
    Unavailable,
}

struct AmdGpuInfo {
    name: String,
    temperature_celsius: f32,
    usage_percent: f32,
    memory_used_mb: u64,
    memory_total_mb: u64,
}

impl AmdGpu {
    fn init() -> Option<Self> {
        let inner = AmdBackend::detect();
        match inner {
            AmdBackend::Unavailable => None,
            other => Some(AmdGpu { inner: other }),
        }
    }

    fn info(&self) -> Option<AmdGpuInfo> {
        self.inner.read()
    }
}

impl AmdBackend {
    fn detect() -> Self {
        #[cfg(target_os = "linux")]
        if let Some(s) = SysfsAmdGpu::detect() {
            return AmdBackend::Sysfs(s);
        }

        #[cfg(target_os = "windows")]
        if wmi_has_amd_gpu() {
            return AmdBackend::Wmi;
        }

        AmdBackend::Unavailable
    }

    fn read(&self) -> Option<AmdGpuInfo> {
        match self {
            #[cfg(target_os = "linux")]
            AmdBackend::Sysfs(s) => s.read(),
            #[cfg(target_os = "windows")]
            AmdBackend::Wmi => wmi_amd_info(),
            AmdBackend::Unavailable => None,
        }
    }
}

fn amd_metrics(amd: &AmdGpu) -> Option<GpuMetrics> {
    let info = amd.info()?;
    Some(GpuMetrics {
        gpu_vendor: "amd".into(),
        gpu_name: info.name,
        gpu_temperature_celsius: info.temperature_celsius,
        gpu_usage_percent: info.usage_percent,
        gpu_memory_used_mb: info.memory_used_mb,
        gpu_memory_total_mb: info.memory_total_mb,
    })
}

#[cfg(target_os = "linux")]
struct SysfsAmdGpu {
    device_path: std::path::PathBuf,
    hwmon_path: Option<std::path::PathBuf>,
    name: String,
}

#[cfg(target_os = "linux")]
impl SysfsAmdGpu {
    fn detect() -> Option<Self> {
        use std::{fs, path::Path};

        let drm = Path::new("/sys/class/drm");
        if !drm.exists() { return None; }

        let mut entries: Vec<_> = fs::read_dir(drm).ok()?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let fname = entry.file_name();
            let fname = fname.to_string_lossy();

            if !fname.starts_with("card") { continue; }
            if fname.chars().skip(4).any(|c| !c.is_ascii_digit()) { continue; }

            let device_path = entry.path().join("device");
            if !device_path.exists() { continue; }

            let vendor = fs::read_to_string(device_path.join("vendor"))
                .unwrap_or_default().trim().to_ascii_lowercase();
            if vendor != "0x1002" { continue; }

            if !device_path.join("gpu_busy_percent").exists() { continue; }

            let name = Self::read_name(&device_path);
            let hwmon_path = Self::find_hwmon(&device_path);

            return Some(SysfsAmdGpu { device_path, hwmon_path, name });
        }
        None
    }

    fn read_name(device: &std::path::Path) -> String {
        if let Ok(uevent) = std::fs::read_to_string(device.join("uevent")) {
            for line in uevent.lines() {
                if let Some(rest) = line.strip_prefix("PRODUCT=") {
                    return format!("AMD GPU ({})", rest.trim());
                }
            }
        }
        if let Some(hwmon) = Self::find_hwmon(device) {
            if let Ok(n) = std::fs::read_to_string(hwmon.join("name")) {
                let n = n.trim().to_string();
                if !n.is_empty() { return format!("AMD GPU ({})", n); }
            }
        }
        "AMD GPU".to_string()
    }

    fn find_hwmon(device: &std::path::Path) -> Option<std::path::PathBuf> {
        std::fs::read_dir(device.join("hwmon")).ok()?
            .filter_map(|e| e.ok())
            .next()
            .map(|e| e.path())
    }

    fn read_u64(path: &std::path::Path) -> Option<u64> {
        std::fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    fn read(&self) -> Option<AmdGpuInfo> {
        let usage_percent =
            Self::read_u64(&self.device_path.join("gpu_busy_percent")).unwrap_or(0) as f32;

        let vram_used = Self::read_u64(&self.device_path.join("mem_info_vram_used")).unwrap_or(0);
        let vram_total = Self::read_u64(&self.device_path.join("mem_info_vram_total")).unwrap_or(0);

        let temperature_celsius = self.hwmon_path.as_ref()
            .and_then(|h| Self::read_u64(&h.join("temp1_input")))
            .map(|t| t as f32 / 1000.0)
            .unwrap_or(0.0);

        Some(AmdGpuInfo {
            name: self.name.clone(),
            temperature_celsius,
            usage_percent,
            memory_used_mb: vram_used / (1024 * 1024),
            memory_total_mb: vram_total / (1024 * 1024),
        })
    }
}

#[cfg(target_os = "windows")]
fn wmi_has_amd_gpu() -> bool {
    use wmi::{COMLibrary, WMIConnection};

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct VideoController {
        name: String,
    }

    let com = match COMLibrary::without_security() {
        Ok(c) => c,
        Err(_) => return false
    };
    let wmi = match WMIConnection::new(com.into()) {
        Ok(w) => w,
        Err(_) => return false
    };

    wmi.raw_query::<VideoController>("SELECT Name FROM Win32_VideoController")
        .map(|v| v.iter().any(|c| is_amd_name(&c.name)))
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn wmi_amd_info() -> Option<AmdGpuInfo> {
    use wmi::{COMLibrary, WMIConnection};

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct VideoController {
        name: String,
        adapter_ram: Option<u64>,
    }

    let com = COMLibrary::without_security().ok()?;
    let wmi = WMIConnection::new(com.into()).ok()?;

    let controller = wmi
        .raw_query::<VideoController>("SELECT Name, AdapterRAM FROM Win32_VideoController")
        .ok()?
        .into_iter()
        .find(|c| is_amd_name(&c.name))?;

    let name = controller.name;
    let memory_total_mb = controller.adapter_ram.unwrap_or(0) / (1024 * 1024);

    let (usage_percent, memory_used_mb) = pdh_amd_usage().unwrap_or((0.0, 0));

    let temperature_celsius = adl_temperature().unwrap_or(0.0);

    Some(AmdGpuInfo {
        name,
        temperature_celsius,
        usage_percent,
        memory_used_mb,
        memory_total_mb,
    })
}

#[cfg(target_os = "windows")]
fn is_amd_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.contains("amd") || l.contains("radeon") || l.contains("rx ")
}

#[cfg(target_os = "windows")]
fn pdh_amd_usage() -> Option<(f32, u64)> {
    use winapi::um::winnt::HANDLE;

    const PDH_OK: i32 = 0x0000_0000_u32 as i32;
    const PDH_FMT_DOUBLE: u32 = 0x0000_0200;
    const PDH_MORE_DATA: i32 = 0x800007D2_u32 as i32;

    #[repr(C)]
    struct PdhValue {
        c_status: u32,
        _pad: u32,
        double: f64,
    }

    #[repr(C)]
    struct PdhFmtCounterValue {
        sz_name: *const u16,
        fmt_value: PdhValue,
    }

    #[link(name = "pdh")]
    unsafe extern "system" {
        fn PdhOpenQueryW(src: *const u16, ud: usize, q: *mut HANDLE) -> i32;
        fn PdhAddEnglishCounterW(q: HANDLE, path: *const u16, ud: usize, c: *mut HANDLE) -> i32;
        fn PdhCollectQueryData(q: HANDLE) -> i32;
        fn PdhGetFormattedCounterArrayW(
            counter: HANDLE,
            format: u32,
            buf_size: *mut u32,
            item_count: *mut u32,
            items: *mut PdhFmtCounterValue,
        ) -> i32;
        fn PdhCloseQuery(q: HANDLE) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe {
        let mut query: HANDLE = std::ptr::null_mut();
        if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != PDH_OK { return None; }

        let engine_path = wide(r"\GPU Engine(*engtype_3D)\Utilization Percentage");
        let mem_path = wide(r"\GPU Local Adapter Memory(*)\Local Usage");

        let mut engine_ctr: HANDLE = std::ptr::null_mut();
        let mut mem_ctr: HANDLE = std::ptr::null_mut();
        PdhAddEnglishCounterW(query, engine_path.as_ptr(), 0, &mut engine_ctr);
        PdhAddEnglishCounterW(query, mem_path.as_ptr(), 0, &mut mem_ctr);

        PdhCollectQueryData(query);
        std::thread::sleep(std::time::Duration::from_millis(250));
        PdhCollectQueryData(query);

        let read_array_sum = |counter: HANDLE| -> f64 {
            let mut buf_size: u32 = 0;
            let mut count: u32 = 0;
            let rc = PdhGetFormattedCounterArrayW(
                counter, PDH_FMT_DOUBLE, &mut buf_size, &mut count, std::ptr::null_mut(),
            );
            if rc != PDH_MORE_DATA && rc != PDH_OK { return 0.0; }
            if buf_size == 0 { return 0.0; }

            let mut buf = vec![0u8; buf_size as usize];
            let rc = PdhGetFormattedCounterArrayW(
                counter, PDH_FMT_DOUBLE,
                &mut buf_size, &mut count,
                buf.as_mut_ptr() as *mut PdhFmtCounterValue,
            );
            if rc != PDH_OK { return 0.0; }

            let items = std::slice::from_raw_parts(
                buf.as_ptr() as *const PdhFmtCounterValue,
                count as usize,
            );
            items.iter().map(|i| i.fmt_value.double).sum()
        };

        let usage_percent = read_array_sum(engine_ctr).min(100.0) as f32;
        let memory_used_mb = (read_array_sum(mem_ctr) / (1024.0 * 1024.0)) as u64;

        PdhCloseQuery(query);
        Some((usage_percent, memory_used_mb))
    }
}

#[cfg(target_os = "windows")]
fn adl_temperature() -> Option<f32> {
    use winapi::um::libloaderapi::{FreeLibrary, GetProcAddress, LoadLibraryA};

    type AdlCtx = *mut std::ffi::c_void;

    #[repr(C)]
    struct AdlTemperature {
        size: i32,
        temperature: i32,
    }

    #[repr(C)]
    struct AdlodnTemperatureOutput {
        size: i32,
        temperature: i32,
    }

    type FnCreate = unsafe extern "C" fn(extern "C" fn(i32) -> *mut std::ffi::c_void, i32, *mut AdlCtx) -> i32;
    type FnDestroy = unsafe extern "C" fn(AdlCtx) -> i32;
    type FnNumAdapters = unsafe extern "C" fn(AdlCtx, *mut i32) -> i32;
    type FnOd5temp = unsafe extern "C" fn(AdlCtx, i32, i32, *mut AdlTemperature) -> i32;
    type FnOdntemp = unsafe extern "C" fn(AdlCtx, i32, i32, *mut AdlodnTemperatureOutput) -> i32;

    extern "C" fn adl_alloc(size: i32) -> *mut std::ffi::c_void {
        let layout = std::alloc::Layout::from_size_align(size as usize, 8)
            .unwrap_or(std::alloc::Layout::new::<u8>());
        unsafe { std::alloc::alloc_zeroed(layout) as *mut std::ffi::c_void }
    }

    fn cstr(s: &str) -> std::ffi::CString { std::ffi::CString::new(s).unwrap() }

    unsafe {
        let hmod = {
            let h = LoadLibraryA(cstr("atiadlxx.dll").as_ptr());
            if h.is_null() { LoadLibraryA(cstr("atiadlxy.dll").as_ptr()) } else { h }
        };
        if hmod.is_null() {
            return None;
        }

        macro_rules! resolve {
            ($sym:expr, $ty:ty) => {{
                let ptr = GetProcAddress(hmod, cstr($sym).as_ptr());
                if ptr.is_null() {
                    FreeLibrary(hmod);
                    return None;
                }
                std::mem::transmute::<_, $ty>(ptr)
            }};
        }

        let create: FnCreate = resolve!("ADL2_Main_Control_Create",         FnCreate);
        let destroy: FnDestroy = resolve!("ADL2_Main_Control_Destroy",        FnDestroy);
        let num_adapters: FnNumAdapters = resolve!("ADL2_Adapter_NumberOfAdapters_Get",FnNumAdapters);
        let od5_temp: FnOd5temp = resolve!("ADL2_Overdrive5_Temperature_Get",  FnOd5temp);

        let odn_temp_ptr = GetProcAddress(hmod, cstr("ADL2_OverdriveN_Temperature_Get").as_ptr());
        let odn_temp: Option<FnOdntemp> = if odn_temp_ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute(odn_temp_ptr))
        };

        let mut ctx: AdlCtx = std::ptr::null_mut();
        if create(adl_alloc, 1, &mut ctx) != 0 {
            FreeLibrary(hmod);
            return None;
        }

        let mut adapter_count: i32 = 0;
        num_adapters(ctx, &mut adapter_count);

        let mut best_celsius: Option<f32> = None;

        for idx in 0..adapter_count.max(1) {
            if let Some(get_odn) = odn_temp {
                let mut out = AdlodnTemperatureOutput { size: std::mem::size_of::<AdlodnTemperatureOutput>() as i32, temperature: 0 };
                if get_odn(ctx, idx, 1, &mut out) == 0 && out.temperature != 0 {
                    let c = out.temperature as f32 / 1000.0;
                    best_celsius = Some(c);
                    break;
                }
            }

            let mut tmp = AdlTemperature { size: std::mem::size_of::<AdlTemperature>() as i32, temperature: 0 };
            if od5_temp(ctx, idx, 0, &mut tmp) == 0 && tmp.temperature != 0 {
                let c = tmp.temperature as f32 / 1000.0;
                best_celsius = Some(c);
                break;
            }
        }

        destroy(ctx);
        FreeLibrary(hmod);

        best_celsius
    }
}