// Brail Recorder — Instant Replay Module
// Rolling buffer for saving the last N seconds of gameplay

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use tracing::{info, warn, error};
use std::path::{Path, PathBuf};
use chrono::Local;

/// Replay buffer duration preset
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReplayDuration {
    Seconds15,
    Seconds30,
    Seconds60,
    Minutes2,
    Minutes5,
    Custom(u32), // seconds
}

impl ReplayDuration {
    pub fn as_seconds(&self) -> u32 {
        match self {
            Self::Seconds15 => 15,
            Self::Seconds30 => 30,
            Self::Seconds60 => 60,
            Self::Minutes2 => 120,
            Self::Minutes5 => 300,
            Self::Custom(s) => *s,
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Seconds15 => "15 seconds".into(),
            Self::Seconds30 => "30 seconds".into(),
            Self::Seconds60 => "60 seconds".into(),
            Self::Minutes2 => "2 minutes".into(),
            Self::Minutes5 => "5 minutes".into(),
            Self::Custom(s) => format!("{} seconds", s),
        }
    }
}

/// Replay configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    pub enabled: bool,
    pub duration: ReplayDuration,
    pub save_directory: String,
    pub hotkey: String,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        let replay_dir = dirs::video_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\Users\\Public\\Videos"))
            .join("Brail Recorder")
            .join("Replays");

        Self {
            enabled: false,
            duration: ReplayDuration::Seconds30,
            save_directory: replay_dir.to_string_lossy().to_string(),
            hotkey: "F10".to_string(),
        }
    }
}

/// Encoded frame stored in the replay buffer
#[derive(Clone)]
struct ReplayFrame {
    data: Vec<u8>,
    width: u32,
    height: u32,
    timestamp_ns: u64,
}

/// Replay buffer that maintains a rolling window of frames
pub struct ReplayBuffer {
    config: ReplayConfig,
    buffer: Arc<Mutex<VecDeque<ReplayFrame>>>,
    running: Arc<AtomicBool>,
    max_frames: usize,
    fps: u32,
    buffer_thread: Option<std::thread::JoinHandle<()>>,
}

impl ReplayBuffer {
    pub fn new(config: ReplayConfig, fps: u32) -> Self {
        let max_frames = (config.duration.as_seconds() * fps) as usize;

        Self {
            config,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(max_frames))),
            running: Arc::new(AtomicBool::new(false)),
            max_frames,
            fps,
            buffer_thread: None,
        }
    }

    /// Start the replay buffer (begins accumulating frames)
    pub fn start(&mut self) {
        self.running.store(true, Ordering::SeqCst);
        info!(
            "Replay buffer started: {} ({} max frames)",
            self.config.duration.display_name(),
            self.max_frames
        );
    }

    /// Stop the replay buffer
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.buffer.lock().clear();
        info!("Replay buffer stopped");
    }

    /// Add a frame to the rolling buffer
    pub fn push_frame(&self, data: Vec<u8>, width: u32, height: u32, timestamp_ns: u64) {
        if !self.running.load(Ordering::SeqCst) {
            return;
        }

        let frame = ReplayFrame {
            data,
            width,
            height,
            timestamp_ns,
        };

        let mut buffer = self.buffer.lock();

        // Maintain rolling window — drop oldest frames
        while buffer.len() >= self.max_frames {
            buffer.pop_front();
        }

        buffer.push_back(frame);
    }

    /// Save the current replay buffer to a file
    pub fn save_replay(&self) -> anyhow::Result<String> {
        let buffer = self.buffer.lock();

        if buffer.is_empty() {
            return Err(anyhow::anyhow!("Replay buffer is empty"));
        }

        info!("Saving replay: {} frames", buffer.len());

        // Generate output path
        let dir = Path::new(&self.config.save_directory);
        let _ = std::fs::create_dir_all(dir);

        let now = Local::now();
        let filename = format!(
            "Brail_Replay_{}.mkv",
            now.format("%Y-%m-%d_%H-%M-%S")
        );

        let mut output_path = dir.join(&filename);
        let mut counter = 1;
        while output_path.exists() {
            output_path = dir.join(format!(
                "Brail_Replay_{}_{}.mkv",
                now.format("%Y-%m-%d_%H-%M-%S"),
                counter
            ));
            counter += 1;
        }

        let output_str = output_path.to_string_lossy().to_string();

        // Get frame dimensions from first frame
        let first = buffer.front().ok_or_else(|| anyhow::anyhow!("No frames"))?;
        let width = first.width;
        let height = first.height;

        // Encode frames using FFmpeg
        let ffmpeg_path = crate::encoder::find_ffmpeg();

        let mut child = std::process::Command::new(&ffmpeg_path)
            .args(&[
                "-y",
                "-f", "rawvideo",
                "-pix_fmt", "bgra",
                "-s", &format!("{}x{}", width, height),
                "-r", &self.fps.to_string(),
                "-i", "pipe:0",
                "-c:v", "libx264",
                "-preset", "ultrafast",
                "-crf", "23",
                "-pix_fmt", "yuv420p",
                &output_str,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start FFmpeg for replay: {}", e))?;

        // Write all buffered frames
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            for frame in buffer.iter() {
                if stdin.write_all(&frame.data).is_err() {
                    break;
                }
            }
        }

        // Close stdin and wait
        drop(child.stdin.take());
        let status = child.wait()?;

        if status.success() {
            info!("Replay saved: {}", output_str);
            Ok(output_str)
        } else {
            Err(anyhow::anyhow!("FFmpeg replay encoding failed"))
        }
    }

    /// Get buffer status
    pub fn get_status(&self) -> ReplayStatus {
        let buffer = self.buffer.lock();
        let current_frames = buffer.len();
        let current_seconds = if self.fps > 0 {
            current_frames as f64 / self.fps as f64
        } else {
            0.0
        };

        // Estimate memory usage
        let memory_bytes: usize = buffer.iter().map(|f| f.data.len()).sum();

        ReplayStatus {
            enabled: self.running.load(Ordering::SeqCst),
            buffer_seconds: current_seconds,
            max_seconds: self.config.duration.as_seconds() as f64,
            buffer_frames: current_frames,
            max_frames: self.max_frames,
            memory_mb: memory_bytes as f64 / (1024.0 * 1024.0),
            buffer_percent: if self.max_frames > 0 {
                (current_frames as f64 / self.max_frames as f64) * 100.0
            } else {
                0.0
            },
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn config(&self) -> &ReplayConfig {
        &self.config
    }
}

impl Drop for ReplayBuffer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Replay buffer status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStatus {
    pub enabled: bool,
    pub buffer_seconds: f64,
    pub max_seconds: f64,
    pub buffer_frames: usize,
    pub max_frames: usize,
    pub memory_mb: f64,
    pub buffer_percent: f64,
}
