// Brail Recorder — Recording Pipeline
// Orchestrates capture → encode → file

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use tracing::{info, warn, error};
use chrono::Local;
use std::time::Instant;

use crate::capture::{CaptureEngine, CaptureConfig, CapturedFrame, CaptureSource};
use crate::encoder::{FfmpegEncoder, EncoderConfig};
use crate::audio::{AudioEngine, AudioConfig};

/// Recording container format
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContainerFormat {
    Mkv,
    Mp4,
    WebM,
}

impl ContainerFormat {
    pub fn extension(&self) -> &str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
        }
    }
}

/// Recording state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RecordingState {
    Idle,
    Starting,
    Recording,
    Paused,
    Stopping,
    Error(String),
}

/// Recording configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingConfig {
    pub capture: CaptureConfig,
    pub encoder: EncoderConfig,
    pub audio: AudioConfig,
    pub container: ContainerFormat,
    pub output_directory: String,
    pub filename_format: String,
    pub auto_remux_to_mp4: bool,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        let output_dir = dirs::video_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\Users\\Public\\Videos"))
            .join("Brail Recorder");

        Self {
            capture: CaptureConfig::default(),
            encoder: EncoderConfig::default(),
            audio: AudioConfig::default(),
            container: ContainerFormat::Mkv,
            output_directory: output_dir.to_string_lossy().to_string(),
            filename_format: "Brail_{date}_{time}".to_string(),
            auto_remux_to_mp4: true,
        }
    }
}

/// Real-time recording statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingStats {
    pub state: RecordingState,
    pub resolution: String,
    pub fps: u32,
    pub actual_fps: f64,
    pub encoder_name: String,
    pub duration_seconds: u64,
    pub file_size_bytes: u64,
    pub file_size_display: String,
    pub disk_write_speed_mbps: f64,
    pub frames_captured: u64,
    pub frames_encoded: u64,
    pub frames_dropped: u64,
    pub cpu_percent: f32,
    pub gpu_percent: f32,
    pub ram_mb: f64,
    pub output_path: String,
}

impl Default for RecordingStats {
    fn default() -> Self {
        Self {
            state: RecordingState::Idle,
            resolution: String::new(),
            fps: 0,
            actual_fps: 0.0,
            encoder_name: String::new(),
            duration_seconds: 0,
            file_size_bytes: 0,
            file_size_display: "0 B".to_string(),
            disk_write_speed_mbps: 0.0,
            frames_captured: 0,
            frames_encoded: 0,
            frames_dropped: 0,
            cpu_percent: 0.0,
            gpu_percent: 0.0,
            ram_mb: 0.0,
            output_path: String::new(),
        }
    }
}

/// The recording pipeline orchestrates capture, encoding, and file writing
pub struct RecordingPipeline {
    config: RecordingConfig,
    state: Arc<Mutex<RecordingState>>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    stats: Arc<Mutex<RecordingStats>>,
    start_time: Option<Instant>,
    output_path: Option<PathBuf>,
    recording_thread: Option<std::thread::JoinHandle<()>>,
}

impl RecordingPipeline {
    pub fn new(config: RecordingConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(RecordingState::Idle)),
            running: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(RecordingStats::default())),
            start_time: None,
            output_path: None,
            recording_thread: None,
        }
    }

    /// Generate output filename
    fn generate_output_path(&self) -> PathBuf {
        let now = Local::now();
        let date = now.format("%Y-%m-%d").to_string();
        let time = now.format("%H-%M-%S").to_string();

        let filename = self.config.filename_format
            .replace("{date}", &date)
            .replace("{time}", &time);

        let dir = Path::new(&self.config.output_directory);
        let _ = std::fs::create_dir_all(dir);

        let ext = self.config.container.extension();
        let mut path = dir.join(format!("{}.{}", filename, ext));

        // Never overwrite existing files
        let mut counter = 1;
        while path.exists() {
            path = dir.join(format!("{}_{}.{}", filename, counter, ext));
            counter += 1;
        }

        path
    }

    /// Start recording
    pub fn start(&mut self) -> anyhow::Result<String> {
        if self.running.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Recording already in progress"));
        }

        *self.state.lock() = RecordingState::Starting;

        let output_path = self.generate_output_path();
        let output_str = output_path.to_string_lossy().to_string();

        info!("Starting recording to: {}", output_str);

        // Write session marker for crash recovery
        write_session_marker(&output_path);

        let config = self.config.clone();
        let running = self.running.clone();
        let paused = self.paused.clone();
        let state = self.state.clone();
        let stats = self.stats.clone();
        let path = output_path.clone();

        running.store(true, Ordering::SeqCst);
        self.start_time = Some(Instant::now());
        self.output_path = Some(output_path);

        let handle = std::thread::spawn(move || {
            if let Err(e) = run_recording_pipeline(config, &running, &paused, &state, &stats, &path) {
                error!("Recording pipeline error: {}", e);
                *state.lock() = RecordingState::Error(e.to_string());
            }
        });

        self.recording_thread = Some(handle);

        Ok(output_str)
    }

    /// Stop recording
    pub fn stop(&mut self) -> anyhow::Result<Option<String>> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(None);
        }

        info!("Stopping recording...");
        *self.state.lock() = RecordingState::Stopping;
        self.running.store(false, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);

        if let Some(handle) = self.recording_thread.take() {
            let _ = handle.join();
        }

        // Remove session marker
        if let Some(ref path) = self.output_path {
            remove_session_marker(path);
        }

        *self.state.lock() = RecordingState::Idle;
        let path = self.output_path.take().map(|p| p.to_string_lossy().to_string());

        // Optionally remux to MP4
        if self.config.auto_remux_to_mp4 && self.config.container == ContainerFormat::Mkv {
            if let Some(ref mkv_path) = path {
                let mp4_path = mkv_path.replace(".mkv", ".mp4");
                info!("Auto-remuxing to MP4: {}", mp4_path);
                if let Err(e) = remux_to_mp4(mkv_path, &mp4_path) {
                    warn!("Remux failed: {}", e);
                }
            }
        }

        info!("Recording stopped");
        Ok(path)
    }

    /// Pause recording
    pub fn pause(&mut self) {
        if self.running.load(Ordering::SeqCst) {
            self.paused.store(true, Ordering::SeqCst);
            *self.state.lock() = RecordingState::Paused;
            info!("Recording paused");
        }
    }

    /// Resume recording
    pub fn resume(&mut self) {
        if self.running.load(Ordering::SeqCst) {
            self.paused.store(false, Ordering::SeqCst);
            *self.state.lock() = RecordingState::Recording;
            info!("Recording resumed");
        }
    }

    /// Get current stats
    pub fn get_stats(&self) -> RecordingStats {
        let mut stats = self.stats.lock().clone();
        if let Some(start) = &self.start_time {
            stats.duration_seconds = start.elapsed().as_secs();
        }
        stats.state = self.state.lock().clone();
        stats
    }

    pub fn is_recording(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn update_config(&mut self, config: RecordingConfig) {
        self.config = config;
    }
}

impl Drop for RecordingPipeline {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Run the recording pipeline
fn run_recording_pipeline(
    config: RecordingConfig,
    running: &AtomicBool,
    paused: &AtomicBool,
    state: &Mutex<RecordingState>,
    stats: &Mutex<RecordingStats>,
    output_path: &Path,
) -> anyhow::Result<()> {
    // Initialize capture
    let mut capture = CaptureEngine::new(config.capture.clone());
    capture.start()?;

    // Initialize audio
    let mut audio = AudioEngine::new(config.audio.clone());
    audio.start()?;

    // Initialize encoder
    let mut encoder_config = config.encoder.clone();
    let output_str = output_path.to_string_lossy().to_string();

    let mut encoder = FfmpegEncoder::new(encoder_config.clone());
    encoder.start_file_encode(&output_str)?;

    // Update state
    *state.lock() = RecordingState::Recording;

    {
        let mut s = stats.lock();
        s.state = RecordingState::Recording;
        s.resolution = format!("{}x{}", config.encoder.width, config.encoder.height);
        s.fps = config.encoder.fps;
        s.encoder_name = encoder_config.display_name();
        s.output_path = output_str.clone();
    }

    info!("Recording pipeline running");

    let frame_receiver = capture.frame_receiver();
    let start_time = Instant::now();
    let mut frames_captured: u64 = 0;
    let mut frames_dropped: u64 = 0;
    let mut last_stats_update = Instant::now();

    // Main pipeline loop
    while running.load(Ordering::SeqCst) {
        // Handle pause
        if paused.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }

        // Receive captured frame
        match frame_receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(frame) => {
                frames_captured += 1;

                // Send frame to encoder
                if let Err(e) = encoder.send_frame(&frame.data) {
                    frames_dropped += 1;
                    if frames_dropped % 100 == 0 {
                        warn!("Frame drop: {}", e);
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                continue;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                warn!("Capture channel disconnected");
                break;
            }
        }

        // Update stats periodically (every 500ms)
        if last_stats_update.elapsed().as_millis() > 500 {
            let elapsed = start_time.elapsed().as_secs_f64();
            let actual_fps = if elapsed > 0.0 { frames_captured as f64 / elapsed } else { 0.0 };

            let file_size = std::fs::metadata(&output_str).map(|m| m.len()).unwrap_or(0);
            let disk_speed = if elapsed > 0.0 { file_size as f64 / elapsed / (1024.0 * 1024.0) } else { 0.0 };

            let mut s = stats.lock();
            s.actual_fps = actual_fps;
            s.frames_captured = frames_captured;
            s.frames_encoded = encoder.frames_encoded();
            s.frames_dropped = frames_dropped;
            s.file_size_bytes = file_size;
            s.file_size_display = format_file_size(file_size);
            s.disk_write_speed_mbps = disk_speed;

            last_stats_update = Instant::now();
        }
    }

    // Stop components
    info!("Stopping recording components...");
    capture.stop();
    audio.stop();
    encoder.stop()?;

    info!("Recording pipeline finished. File: {}", output_str);
    Ok(())
}

/// Write a session marker for crash recovery
fn write_session_marker(output_path: &Path) {
    let marker_path = output_path.with_extension("brail_session");
    let session_data = serde_json::json!({
        "path": output_path.to_string_lossy(),
        "started": chrono::Local::now().to_rfc3339(),
        "pid": std::process::id(),
    });

    if let Err(e) = std::fs::write(&marker_path, session_data.to_string()) {
        warn!("Failed to write session marker: {}", e);
    }
}

/// Remove session marker after successful recording
fn remove_session_marker(output_path: &Path) {
    let marker_path = output_path.with_extension("brail_session");
    let _ = std::fs::remove_file(marker_path);
}

/// Remux MKV to MP4 using FFmpeg (no re-encoding)
pub fn remux_to_mp4(input: &str, output: &str) -> anyhow::Result<()> {
    let ffmpeg = crate::encoder::find_ffmpeg();

    let status = std::process::Command::new(&ffmpeg)
        .args(&[
            "-i", input,
            "-c", "copy",
            "-movflags", "+faststart",
            output,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if status.success() {
        info!("Remux successful: {} -> {}", input, output);
        Ok(())
    } else {
        Err(anyhow::anyhow!("Remux failed"))
    }
}

/// Format file size for display
fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Check for unfinished recording sessions (crash recovery)
pub fn check_unfinished_sessions() -> Vec<(PathBuf, String)> {
    let video_dir = dirs::video_dir()
        .unwrap_or_else(|| PathBuf::from("C:\\Users\\Public\\Videos"))
        .join("Brail Recorder");

    let mut sessions = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&video_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "brail_session").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                        let recording_path = data["path"].as_str().unwrap_or("Unknown").to_string();
                        sessions.push((path, recording_path));
                    }
                }
            }
        }
    }

    sessions
}
