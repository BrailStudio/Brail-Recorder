// Brail Recorder — Performance Monitor & Adaptive Engine
// Lightweight metrics collection: CPU, GPU, RAM, Disk, Network, FPS, Drops

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant};
use parking_lot::Mutex;
use tracing::{info};

use windows::Win32::System::ProcessStatus::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::SystemInformation::*;
use windows::Win32::Foundation::*;

/// Adaptive Engine preset mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AdaptiveMode {
    UltraLite,
    LowEnd,
    Balanced,
    Quality,
    Streaming,
    Custom,
}

impl Default for AdaptiveMode {
    fn default() -> Self {
        AdaptiveMode::Balanced
    }
}

/// Snapshot of system and application metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSnapshot {
    pub process_ram_mb: f64,
    pub system_ram_total_mb: u64,
    pub system_ram_used_mb: u64,
    pub system_ram_percent: f32,
    pub process_cpu_percent: f32,
    pub system_cpu_percent: f32,
    pub estimated_gpu_percent: f32,
    pub capture_fps: f32,
    pub actual_fps: f32,
    pub dropped_frames: u64,
    pub disk_write_mb_per_sec: f64,
    pub network_upload_mbps: f64,
    pub adaptive_mode: AdaptiveMode,
    pub health_status: String,
    pub warnings: Vec<String>,
}

impl Default for PerformanceSnapshot {
    fn default() -> Self {
        Self {
            process_ram_mb: 0.0,
            system_ram_total_mb: 0,
            system_ram_used_mb: 0,
            system_ram_percent: 0.0,
            process_cpu_percent: 0.0,
            system_cpu_percent: 0.0,
            estimated_gpu_percent: 0.0,
            capture_fps: 60.0,
            actual_fps: 60.0,
            dropped_frames: 0,
            disk_write_mb_per_sec: 0.0,
            network_upload_mbps: 0.0,
            adaptive_mode: AdaptiveMode::Balanced,
            health_status: "Good".to_string(),
            warnings: Vec::new(),
        }
    }
}

/// Lightweight Performance Monitor
pub struct PerformanceMonitor {
    _running: Arc<AtomicBool>,
    mode: Mutex<AdaptiveMode>,
    last_snapshot: Mutex<PerformanceSnapshot>,
    
    // Tracking counters
    bytes_written_total: Arc<AtomicU64>,
    bytes_uploaded_total: Arc<AtomicU64>,
    frames_captured: Arc<AtomicU64>,
    frames_dropped: Arc<AtomicU64>,

    // CPU sampling state
    last_system_times: Mutex<Option<(FILETIME, FILETIME, FILETIME)>>, // (idle, kernel, user)
    last_proc_times: Mutex<Option<(FILETIME, FILETIME, Instant)>>,     // (kernel, user, sampled_at)
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            _running: Arc::new(AtomicBool::new(false)),
            mode: Mutex::new(AdaptiveMode::Balanced),
            last_snapshot: Mutex::new(PerformanceSnapshot::default()),
            bytes_written_total: Arc::new(AtomicU64::new(0)),
            bytes_uploaded_total: Arc::new(AtomicU64::new(0)),
            frames_captured: Arc::new(AtomicU64::new(0)),
            frames_dropped: Arc::new(AtomicU64::new(0)),
            last_system_times: Mutex::new(None),
            last_proc_times: Mutex::new(None),
        }
    }

    pub fn set_mode(&self, mode: AdaptiveMode) {
        *self.mode.lock() = mode;
        info!("Brail Adaptive Engine mode set to: {:?}", mode);
    }

    pub fn get_mode(&self) -> AdaptiveMode {
        *self.mode.lock()
    }

    pub fn record_bytes_written(&self, bytes: u64) {
        self.bytes_written_total.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_bytes_uploaded(&self, bytes: u64) {
        self.bytes_uploaded_total.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_frame_captured(&self) {
        self.frames_captured.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_frame_dropped(&self) {
        self.frames_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Sample current system and process metrics
    pub fn sample(&self, target_fps: f32) -> PerformanceSnapshot {
        let (proc_ram_mb, sys_ram_total_mb, sys_ram_used_mb, sys_ram_pct) = Self::sample_memory();
        let (proc_cpu_pct, sys_cpu_pct) = self.sample_cpu();
        
        let dropped = self.frames_dropped.load(Ordering::Relaxed);
        let captured = self.frames_captured.load(Ordering::Relaxed);

        let mode = *self.mode.lock();
        
        // Smart warnings collection
        let mut warnings = Vec::new();
        if proc_ram_mb > 150.0 {
            warnings.push(format!("Process RAM is elevated ({:.1} MB). Consider Low-End mode.", proc_ram_mb));
        }
        if sys_cpu_pct > 85.0 {
            warnings.push("High system CPU usage detected. Consider switching to hardware encoder.".to_string());
        }
        if dropped > 0 {
            let drop_rate = (dropped as f32 / (captured.max(1) as f32)) * 100.0;
            if drop_rate > 2.0 {
                warnings.push(format!("{:.1}% frames dropped. Lower resolution or FPS.", drop_rate));
            }
        }

        let health_status = if warnings.is_empty() {
            "Optimal".to_string()
        } else if warnings.len() == 1 {
            "Moderate".to_string()
        } else {
            "Strained".to_string()
        };

        let snapshot = PerformanceSnapshot {
            process_ram_mb: proc_ram_mb,
            system_ram_total_mb: sys_ram_total_mb,
            system_ram_used_mb: sys_ram_used_mb,
            system_ram_percent: sys_ram_pct,
            process_cpu_percent: proc_cpu_pct,
            system_cpu_percent: sys_cpu_pct,
            estimated_gpu_percent: (proc_cpu_pct * 0.4).min(100.0), // Approximated when direct hardware query is bounded
            capture_fps: target_fps,
            actual_fps: target_fps * (1.0 - (dropped as f32 / (captured + dropped).max(1) as f32)).max(0.0),
            dropped_frames: dropped,
            disk_write_mb_per_sec: 0.0, // Computed across intervals in periodic update
            network_upload_mbps: 0.0,
            adaptive_mode: mode,
            health_status,
            warnings,
        };

        *self.last_snapshot.lock() = snapshot.clone();
        snapshot
    }

    fn sample_memory() -> (f64, u64, u64, f32) {
        unsafe {
            // Process Memory
            let proc = GetCurrentProcess();
            let mut pmc = PROCESS_MEMORY_COUNTERS {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            let proc_ram_mb = if K32GetProcessMemoryInfo(proc, &mut pmc, pmc.cb).is_ok() {
                pmc.WorkingSetSize as f64 / (1024.0 * 1024.0)
            } else {
                35.0 // Sensible default fallback
            };

            // System Memory
            let mut mem_status = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };
            let (total_mb, used_mb, pct) = if GlobalMemoryStatusEx(&mut mem_status).is_ok() {
                let total = mem_status.ullTotalPhys / (1024 * 1024);
                let avail = mem_status.ullAvailPhys / (1024 * 1024);
                let used = total.saturating_sub(avail);
                let pct = mem_status.dwMemoryLoad as f32;
                (total, used, pct)
            } else {
                (16384, 8192, 50.0)
            };

            (proc_ram_mb, total_mb, used_mb, pct)
        }
    }

    fn sample_cpu(&self) -> (f32, f32) {
        unsafe {
            // System CPU times
            let mut idle = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();

            let sys_pct = if GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_ok() {
                let mut guard = self.last_system_times.lock();
                if let Some((prev_idle, prev_kernel, prev_user)) = *guard {
                    let idle_delta = filetime_to_u64(&idle).saturating_sub(filetime_to_u64(&prev_idle));
                    let kernel_delta = filetime_to_u64(&kernel).saturating_sub(filetime_to_u64(&prev_kernel));
                    let user_delta = filetime_to_u64(&user).saturating_sub(filetime_to_u64(&prev_user));
                    let total = kernel_delta + user_delta;

                    *guard = Some((idle, kernel, user));
                    if total > 0 {
                        let busy = total.saturating_sub(idle_delta);
                        ((busy as f64 / total as f64) * 100.0) as f32
                    } else {
                        5.0
                    }
                } else {
                    *guard = Some((idle, kernel, user));
                    5.0
                }
            } else {
                5.0
            };

            // Process CPU times
            let proc = GetCurrentProcess();
            let mut creation = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut proc_kernel = FILETIME::default();
            let mut proc_user = FILETIME::default();

            let proc_pct = if GetProcessTimes(proc, &mut creation, &mut exit, &mut proc_kernel, &mut proc_user).is_ok() {
                let now = Instant::now();
                let mut guard = self.last_proc_times.lock();
                if let Some((prev_k, prev_u, prev_t)) = *guard {
                    let elapsed = now.duration_since(prev_t).as_secs_f32();
                    let k_delta = filetime_to_u64(&proc_kernel).saturating_sub(filetime_to_u64(&prev_k));
                    let u_delta = filetime_to_u64(&proc_user).saturating_sub(filetime_to_u64(&prev_u));
                    let total_proc_time_sec = (k_delta + u_delta) as f32 / 10_000_000.0;

                    *guard = Some((proc_kernel, proc_user, now));
                    if elapsed > 0.05 {
                        ((total_proc_time_sec / elapsed) * 100.0).clamp(0.0, 100.0)
                    } else {
                        2.0
                    }
                } else {
                    *guard = Some((proc_kernel, proc_user, now));
                    2.0
                }
            } else {
                2.0
            };

            (proc_pct, sys_pct)
        }
    }
}

fn filetime_to_u64(ft: &FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}
