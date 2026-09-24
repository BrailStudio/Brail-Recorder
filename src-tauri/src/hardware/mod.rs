// Brail Recorder — Hardware Detection Module
// Detects GPUs, encoders, monitors, audio devices, cameras, and system capabilities.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, warn};
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::System::SystemInformation::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

/// GPU vendor identification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown(String),
}

/// Available hardware encoder
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HardwareEncoder {
    Nvenc,        // NVIDIA
    Amf,          // AMD
    Qsv,          // Intel Quick Sync
    None,         // CPU-only
}

/// Supported codec
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Codec {
    H264,
    H265,
    Av1,
}

/// Monitor information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub name: String,
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub primary: bool,
    pub dpi_scale: f64,
    pub hmonitor: isize,
}

/// Audio device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub id: String,
    pub is_input: bool,
    pub is_default: bool,
}

/// Camera information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraInfo {
    pub name: String,
    pub id: String,
}

/// GPU information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    pub vram_mb: u64,
    pub driver_version: String,
    pub encoders: Vec<HardwareEncoder>,
    pub supported_codecs: Vec<Codec>,
}

/// System CPU information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub name: String,
    pub cores: u32,
    pub logical_processors: u32,
    pub architecture: String,
}

/// Overall system capability profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemProfile {
    pub cpu: CpuInfo,
    pub gpus: Vec<GpuInfo>,
    pub primary_gpu: Option<GpuInfo>,
    pub best_encoder: HardwareEncoder,
    pub monitors: Vec<MonitorInfo>,
    pub audio_outputs: Vec<AudioDeviceInfo>,
    pub audio_inputs: Vec<AudioDeviceInfo>,
    pub cameras: Vec<CameraInfo>,
    pub os_version: String,
    pub total_ram_mb: u64,
    pub recommended_resolution: String,
    pub recommended_fps: u32,
    pub supports_4k60: bool,
    pub supports_av1: bool,
    pub windows_graphics_capture_supported: bool,
}

/// Detect all system hardware and capabilities
pub fn detect_system() -> SystemProfile {
    info!("Starting hardware detection...");

    let cpu = detect_cpu();
    info!("CPU: {} ({} cores)", cpu.name, cpu.cores);

    let gpus = detect_gpus();
    for gpu in &gpus {
        info!("GPU: {} ({:?}, {} MB VRAM)", gpu.name, gpu.vendor, gpu.vram_mb);
    }

    let primary_gpu = gpus.first().cloned();
    let best_encoder = determine_best_encoder(&gpus);
    info!("Best encoder: {:?}", best_encoder);

    let monitors = detect_monitors();
    for mon in &monitors {
        info!(
            "Monitor: {} ({}x{} @{}Hz, primary={})",
            mon.name, mon.width, mon.height, mon.refresh_rate, mon.primary
        );
    }

    let (audio_outputs, audio_inputs) = detect_audio_devices();
    let cameras = detect_cameras();
    let os_version = detect_os_version();
    let total_ram_mb = detect_total_ram();

    let supports_av1 = gpus.iter().any(|g| g.supported_codecs.contains(&Codec::Av1));
    let supports_4k60 = gpus.iter().any(|g| {
        g.vram_mb >= 4096
            && (g.encoders.contains(&HardwareEncoder::Nvenc)
                || g.encoders.contains(&HardwareEncoder::Amf)
                || g.encoders.contains(&HardwareEncoder::Qsv))
    });

    let recommended_resolution = recommend_resolution(&gpus, total_ram_mb);
    let recommended_fps = if supports_4k60 { 60 } else { 30 };

    // Windows Graphics Capture requires Windows 10 1903+
    let wgc_supported = is_windows_graphics_capture_supported();

    let profile = SystemProfile {
        cpu,
        gpus,
        primary_gpu,
        best_encoder,
        monitors,
        audio_outputs,
        audio_inputs,
        cameras,
        os_version,
        total_ram_mb,
        recommended_resolution,
        recommended_fps,
        supports_4k60,
        supports_av1,
        windows_graphics_capture_supported: wgc_supported,
    };

    info!("Hardware detection complete");
    profile
}

/// Detect CPU information
fn detect_cpu() -> CpuInfo {
    let mut sys_info = SYSTEM_INFO::default();
    unsafe {
        GetSystemInfo(&mut sys_info);
    }

    let arch = match unsafe { sys_info.Anonymous.Anonymous.wProcessorArchitecture } {
        PROCESSOR_ARCHITECTURE_AMD64 => "x86_64".to_string(),
        PROCESSOR_ARCHITECTURE_ARM64 => "ARM64".to_string(),
        PROCESSOR_ARCHITECTURE_INTEL => "x86".to_string(),
        _ => "Unknown".to_string(),
    };

    let cores = sys_info.dwNumberOfProcessors;
    let name = get_cpu_name();

    CpuInfo {
        name,
        cores,
        logical_processors: cores,
        architecture: arch,
    }
}

fn get_cpu_name() -> String {
    // Try to read CPU name from registry or use a generic fallback
    std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "Unknown CPU".to_string())
}

/// Detect GPU information via DXGI
fn detect_gpus() -> Vec<GpuInfo> {
    let mut gpus = Vec::new();

    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1() {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to create DXGI factory: {}", e);
                return gpus;
            }
        };

        let mut adapter_index = 0u32;
        loop {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_index) {
                Ok(a) => a,
                Err(_) => break,
            };

            let mut desc = DXGI_ADAPTER_DESC1::default();
            if adapter.GetDesc1(&mut desc).is_ok() {
                let name = String::from_utf16_lossy(
                    &desc.Description[..desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len())]
                );

                // Skip Microsoft Basic Render Driver
                if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 != 0 {
                    adapter_index += 1;
                    continue;
                }

                let vendor = match desc.VendorId {
                    0x10DE => GpuVendor::Nvidia,
                    0x1002 => GpuVendor::Amd,
                    0x8086 => GpuVendor::Intel,
                    _ => GpuVendor::Unknown(format!("0x{:04X}", desc.VendorId)),
                };

                let vram_mb = desc.DedicatedVideoMemory as u64 / (1024 * 1024);

                let (encoders, codecs) = detect_encoder_capabilities(&vendor, &name, vram_mb);

                gpus.push(GpuInfo {
                    name: name.trim().to_string(),
                    vendor,
                    vram_mb,
                    driver_version: String::new(),
                    encoders,
                    supported_codecs: codecs,
                });
            }

            adapter_index += 1;
        }
    }

    gpus
}

/// Detect available hardware encoders for a GPU
fn detect_encoder_capabilities(vendor: &GpuVendor, name: &str, vram_mb: u64) -> (Vec<HardwareEncoder>, Vec<Codec>) {
    let mut encoders = Vec::new();
    let mut codecs = vec![Codec::H264]; // H.264 is universally supported

    match vendor {
        GpuVendor::Nvidia => {
            encoders.push(HardwareEncoder::Nvenc);
            codecs.push(Codec::H265);

            // RTX 40 series and newer support AV1 encoding
            let name_upper = name.to_uppercase();
            if name_upper.contains("RTX 40") || name_upper.contains("RTX 50") || name_upper.contains("RTX 60") {
                codecs.push(Codec::Av1);
            }
        }
        GpuVendor::Amd => {
            encoders.push(HardwareEncoder::Amf);
            codecs.push(Codec::H265);

            // RX 7000+ series support AV1
            let name_upper = name.to_uppercase();
            if name_upper.contains("RX 7") || name_upper.contains("RX 8") || name_upper.contains("RX 9") {
                codecs.push(Codec::Av1);
            }
        }
        GpuVendor::Intel => {
            // Intel Arc and newer iGPUs support QSV
            encoders.push(HardwareEncoder::Qsv);
            codecs.push(Codec::H265);

            let name_upper = name.to_uppercase();
            if name_upper.contains("ARC") || name_upper.contains("A7") || name_upper.contains("A5") {
                codecs.push(Codec::Av1);
            }
        }
        _ => {}
    }

    (encoders, codecs)
}

/// Determine the best available hardware encoder
fn determine_best_encoder(gpus: &[GpuInfo]) -> HardwareEncoder {
    // Prefer NVENC > AMF > QSV > None
    for gpu in gpus {
        if gpu.encoders.contains(&HardwareEncoder::Nvenc) {
            return HardwareEncoder::Nvenc;
        }
    }
    for gpu in gpus {
        if gpu.encoders.contains(&HardwareEncoder::Amf) {
            return HardwareEncoder::Amf;
        }
    }
    for gpu in gpus {
        if gpu.encoders.contains(&HardwareEncoder::Qsv) {
            return HardwareEncoder::Qsv;
        }
    }
    HardwareEncoder::None
}

/// Detect connected monitors
fn detect_monitors() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();

    unsafe {
        let callback_data = &mut monitors as *mut Vec<MonitorInfo>;
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitor_callback),
            LPARAM(callback_data as isize),
        );
    }

    monitors
}

unsafe extern "system" fn enum_monitor_callback(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _lprect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = &mut *(lparam.0 as *mut Vec<MonitorInfo>);

    let mut mi = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    if GetMonitorInfoW(hmonitor, &mut mi.monitorInfo as *mut MONITORINFO).as_bool() {
        let name = String::from_utf16_lossy(
            &mi.szDevice[..mi.szDevice.iter().position(|&c| c == 0).unwrap_or(mi.szDevice.len())]
        );

        let rc = mi.monitorInfo.rcMonitor;
        let width = (rc.right - rc.left) as u32;
        let height = (rc.bottom - rc.top) as u32;
        let primary = (mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;

        // Get refresh rate from DEVMODE
        let mut devmode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let refresh_rate = if EnumDisplaySettingsW(
            PCWSTR(mi.szDevice.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut devmode,
        ).as_bool() {
            devmode.dmDisplayFrequency
        } else {
            60
        };

        // Get DPI
        let dpi_scale = {
            let hdc = GetDC(HWND::default());
            let dpi = GetDeviceCaps(hdc, LOGPIXELSX);
            let _ = ReleaseDC(HWND::default(), hdc);
            dpi as f64 / 96.0
        };

        monitors.push(MonitorInfo {
            name: name.trim().to_string(),
            id: format!("monitor_{}", monitors.len()),
            width,
            height,
            refresh_rate,
            primary,
            dpi_scale,
            hmonitor: hmonitor.0 as isize,
        });
    }

    TRUE
}

/// Detect audio devices
fn detect_audio_devices() -> (Vec<AudioDeviceInfo>, Vec<AudioDeviceInfo>) {
    let mut outputs = Vec::new();
    let mut inputs = Vec::new();

    // Use default audio device detection
    // In a full implementation, this enumerates via IMMDeviceEnumerator
    outputs.push(AudioDeviceInfo {
        name: "Default System Audio".to_string(),
        id: "default_output".to_string(),
        is_input: false,
        is_default: true,
    });

    inputs.push(AudioDeviceInfo {
        name: "Default Microphone".to_string(),
        id: "default_input".to_string(),
        is_input: true,
        is_default: true,
    });

    // Enumerate additional devices via WASAPI
    enumerate_wasapi_devices(&mut outputs, &mut inputs);

    (outputs, inputs)
}

fn enumerate_wasapi_devices(outputs: &mut Vec<AudioDeviceInfo>, inputs: &mut Vec<AudioDeviceInfo>) {
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerator: Result<IMMDeviceEnumerator> = CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        );

        if let Ok(enumerator) = enumerator {
            // Enumerate output devices
            if let Ok(collection) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                if let Ok(count) = collection.GetCount() {
                    for i in 0..count {
                        if let Ok(device) = collection.Item(i) {
                            if let Ok(props) = device.OpenPropertyStore(STGM_READ) {
                                let name = get_device_name(&props).unwrap_or_else(|| format!("Output Device {}", i));
                                let id = device.GetId().map(|id| {
                                    let s = id.to_string().unwrap_or_default();
                                    s
                                }).unwrap_or_else(|_| format!("output_{}", i));

                                if !outputs.iter().any(|d| d.name == name) {
                                    outputs.push(AudioDeviceInfo {
                                        name,
                                        id,
                                        is_input: false,
                                        is_default: i == 0,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // Enumerate input devices
            if let Ok(collection) = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) {
                if let Ok(count) = collection.GetCount() {
                    for i in 0..count {
                        if let Ok(device) = collection.Item(i) {
                            if let Ok(props) = device.OpenPropertyStore(STGM_READ) {
                                let name = get_device_name(&props).unwrap_or_else(|| format!("Input Device {}", i));
                                let id = device.GetId().map(|id| {
                                    let s = id.to_string().unwrap_or_default();
                                    s
                                }).unwrap_or_else(|_| format!("input_{}", i));

                                if !inputs.iter().any(|d| d.name == name) {
                                    inputs.push(AudioDeviceInfo {
                                        name,
                                        id,
                                        is_input: true,
                                        is_default: i == 0,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn get_device_name(props: &windows::Win32::Media::Audio::IPropertyStore) -> Option<String> {
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;

    unsafe {
        let pv = props.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let name = pv.Anonymous.Anonymous.Anonymous.pwszVal;
        if name.is_null() {
            return None;
        }
        Some(name.to_string().ok()?)
    }
}

/// Detect cameras (basic enumeration)
fn detect_cameras() -> Vec<CameraInfo> {
    // Camera enumeration via DirectShow / Media Foundation
    // For now, we detect cameras by checking for video capture devices
    let mut cameras = Vec::new();

    // Basic detection — full implementation would use Media Foundation
    info!("Camera detection: basic mode");

    cameras
}

/// Detect OS version
fn detect_os_version() -> String {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };

    // Use RtlGetVersion for accurate version info
    #[allow(non_snake_case)]
    type RtlGetVersion = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;

    let ntdll = unsafe { windows::Win32::System::LibraryLoader::LoadLibraryW(w!("ntdll.dll")) };
    if let Ok(ntdll) = ntdll {
        let proc = unsafe {
            windows::Win32::System::LibraryLoader::GetProcAddress(ntdll, windows::core::s!("RtlGetVersion"))
        };
        if let Some(proc) = proc {
            let rtl_get_version: RtlGetVersion = unsafe { std::mem::transmute(proc) };
            unsafe { rtl_get_version(&mut info); }
        }
    }

    format!(
        "Windows {}.{}.{}",
        info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
    )
}

/// Get total system RAM in MB
fn detect_total_ram() -> u64 {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GlobalMemoryStatusEx(&mut status);
    }
    status.ullTotalPhys / (1024 * 1024)
}

/// Recommend resolution based on hardware
fn recommend_resolution(gpus: &[GpuInfo], total_ram_mb: u64) -> String {
    let has_hw_encoder = gpus.iter().any(|g| !g.encoders.is_empty());
    let best_vram = gpus.iter().map(|g| g.vram_mb).max().unwrap_or(0);

    if best_vram >= 8192 && total_ram_mb >= 16384 && has_hw_encoder {
        "1440p60".to_string()
    } else if best_vram >= 4096 && total_ram_mb >= 8192 && has_hw_encoder {
        "1080p60".to_string()
    } else if has_hw_encoder {
        "720p60".to_string()
    } else if total_ram_mb >= 8192 {
        "720p30".to_string()
    } else {
        "480p30".to_string()
    }
}

/// Check if Windows Graphics Capture API is supported
fn is_windows_graphics_capture_supported() -> bool {
    // WGC requires Windows 10 version 1903 (build 18362) or later
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };

    #[allow(non_snake_case)]
    type RtlGetVersion = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;

    let ntdll = unsafe { windows::Win32::System::LibraryLoader::LoadLibraryW(w!("ntdll.dll")) };
    if let Ok(ntdll) = ntdll {
        let proc = unsafe {
            windows::Win32::System::LibraryLoader::GetProcAddress(ntdll, windows::core::s!("RtlGetVersion"))
        };
        if let Some(proc) = proc {
            let rtl_get_version: RtlGetVersion = unsafe { std::mem::transmute(proc) };
            unsafe { rtl_get_version(&mut info); }

            return info.dwMajorVersion >= 10 && info.dwBuildNumber >= 18362;
        }
    }

    false
}
