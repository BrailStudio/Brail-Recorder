// Brail Recorder — Encoder Module
// FFmpeg-based video/audio encoding with hardware encoder support

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use tracing::{info, warn, error};

use crate::hardware::{HardwareEncoder, Codec, GpuVendor};

/// Encoder preset
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EncoderPreset {
    UltraFast,
    SuperFast,
    VeryFast,
    Fast,
    Medium,
    Slow,
    Quality,
}

impl EncoderPreset {
    pub fn to_ffmpeg_preset(&self, encoder: &HardwareEncoder) -> &str {
        match encoder {
            HardwareEncoder::Nvenc => match self {
                Self::UltraFast => "p1",
                Self::SuperFast => "p2",
                Self::VeryFast => "p3",
                Self::Fast => "p4",
                Self::Medium => "p5",
                Self::Slow => "p6",
                Self::Quality => "p7",
            },
            HardwareEncoder::Amf => match self {
                Self::UltraFast | Self::SuperFast | Self::VeryFast => "speed",
                Self::Fast | Self::Medium => "balanced",
                Self::Slow | Self::Quality => "quality",
            },
            HardwareEncoder::Qsv => match self {
                Self::UltraFast | Self::SuperFast => "veryfast",
                Self::VeryFast | Self::Fast => "fast",
                Self::Medium => "medium",
                Self::Slow | Self::Quality => "slow",
            },
            HardwareEncoder::None => match self {
                Self::UltraFast => "ultrafast",
                Self::SuperFast => "superfast",
                Self::VeryFast => "veryfast",
                Self::Fast => "fast",
                Self::Medium => "medium",
                Self::Slow => "slow",
                Self::Quality => "veryslow",
            },
        }
    }
}

/// Rate control mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RateControl {
    CBR,     // Constant Bitrate
    VBR,     // Variable Bitrate
    CQP,     // Constant Quantization Parameter
}

/// H.264 profile
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum H264Profile {
    Baseline,
    Main,
    High,
}

/// Encoder configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderConfig {
    pub encoder: HardwareEncoder,
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub max_bitrate_kbps: u32,
    pub keyframe_interval: u32,
    pub preset: EncoderPreset,
    pub profile: H264Profile,
    pub rate_control: RateControl,
    pub b_frames: u32,
    pub quality: u32,          // CQP quality value (0-51 for H.264)
    pub pixel_format: String,  // e.g., "bgra", "nv12"
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            encoder: HardwareEncoder::None,
            codec: Codec::H264,
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 6000,
            max_bitrate_kbps: 8000,
            keyframe_interval: 2,
            preset: EncoderPreset::Fast,
            profile: H264Profile::High,
            rate_control: RateControl::CBR,
            b_frames: 2,
            quality: 23,
            pixel_format: "bgra".to_string(),
        }
    }
}

impl EncoderConfig {
    /// Get FFmpeg encoder name
    pub fn ffmpeg_encoder_name(&self) -> &str {
        match (&self.encoder, &self.codec) {
            (HardwareEncoder::Nvenc, Codec::H264) => "h264_nvenc",
            (HardwareEncoder::Nvenc, Codec::H265) => "hevc_nvenc",
            (HardwareEncoder::Nvenc, Codec::Av1) => "av1_nvenc",
            (HardwareEncoder::Amf, Codec::H264) => "h264_amf",
            (HardwareEncoder::Amf, Codec::H265) => "hevc_amf",
            (HardwareEncoder::Amf, Codec::Av1) => "av1_amf",
            (HardwareEncoder::Qsv, Codec::H264) => "h264_qsv",
            (HardwareEncoder::Qsv, Codec::H265) => "hevc_qsv",
            (HardwareEncoder::Qsv, Codec::Av1) => "av1_qsv",
            (HardwareEncoder::None, Codec::H264) => "libx264",
            (HardwareEncoder::None, Codec::H265) => "libx265",
            (HardwareEncoder::None, Codec::Av1) => "libaom-av1",
        }
    }

    /// Get encoder display name
    pub fn display_name(&self) -> String {
        let encoder_name = match &self.encoder {
            HardwareEncoder::Nvenc => "NVIDIA NVENC",
            HardwareEncoder::Amf => "AMD AMF",
            HardwareEncoder::Qsv => "Intel QSV",
            HardwareEncoder::None => "Software (CPU)",
        };
        let codec_name = match &self.codec {
            Codec::H264 => "H.264",
            Codec::H265 => "H.265/HEVC",
            Codec::Av1 => "AV1",
        };
        format!("{} {}", encoder_name, codec_name)
    }

    /// Build FFmpeg arguments for encoding
    pub fn build_ffmpeg_args(&self, output: &str) -> Vec<String> {
        let mut args = Vec::new();

        // Input: raw video from pipe
        args.extend_from_slice(&[
            "-y".to_string(),
            "-f".to_string(), "rawvideo".to_string(),
            "-pix_fmt".to_string(), self.pixel_format.clone(),
            "-s".to_string(), format!("{}x{}", self.width, self.height),
            "-r".to_string(), self.fps.to_string(),
            "-i".to_string(), "pipe:0".to_string(),
        ]);

        // Video encoder
        args.extend_from_slice(&[
            "-c:v".to_string(), self.ffmpeg_encoder_name().to_string(),
        ]);

        // Rate control
        match self.rate_control {
            RateControl::CBR => {
                args.extend_from_slice(&[
                    "-b:v".to_string(), format!("{}k", self.bitrate_kbps),
                    "-maxrate".to_string(), format!("{}k", self.bitrate_kbps),
                    "-bufsize".to_string(), format!("{}k", self.bitrate_kbps * 2),
                ]);
            }
            RateControl::VBR => {
                args.extend_from_slice(&[
                    "-b:v".to_string(), format!("{}k", self.bitrate_kbps),
                    "-maxrate".to_string(), format!("{}k", self.max_bitrate_kbps),
                    "-bufsize".to_string(), format!("{}k", self.max_bitrate_kbps * 2),
                ]);
            }
            RateControl::CQP => {
                match &self.encoder {
                    HardwareEncoder::Nvenc => {
                        args.extend_from_slice(&[
                            "-rc".to_string(), "constqp".to_string(),
                            "-qp".to_string(), self.quality.to_string(),
                        ]);
                    }
                    HardwareEncoder::None => {
                        args.extend_from_slice(&[
                            "-crf".to_string(), self.quality.to_string(),
                        ]);
                    }
                    _ => {
                        args.extend_from_slice(&[
                            "-q:v".to_string(), self.quality.to_string(),
                        ]);
                    }
                }
            }
        }

        // Preset
        let preset = self.preset.to_ffmpeg_preset(&self.encoder);
        if self.encoder == HardwareEncoder::Nvenc {
            args.extend_from_slice(&["-preset".to_string(), preset.to_string()]);
        } else if self.encoder != HardwareEncoder::Amf {
            args.extend_from_slice(&["-preset".to_string(), preset.to_string()]);
        }

        // Profile (H.264 only)
        if self.codec == Codec::H264 {
            let profile = match self.profile {
                H264Profile::Baseline => "baseline",
                H264Profile::Main => "main",
                H264Profile::High => "high",
            };
            args.extend_from_slice(&["-profile:v".to_string(), profile.to_string()]);
        }

        // Keyframe interval
        args.extend_from_slice(&[
            "-g".to_string(), (self.fps * self.keyframe_interval).to_string(),
            "-keyint_min".to_string(), (self.fps * self.keyframe_interval).to_string(),
        ]);

        // B-frames
        if self.b_frames > 0 && self.codec == Codec::H264 {
            args.extend_from_slice(&["-bf".to_string(), self.b_frames.to_string()]);
        }

        // Pixel format conversion
        args.extend_from_slice(&["-pix_fmt".to_string(), "yuv420p".to_string()]);

        // Output
        args.push(output.to_string());

        args
    }
}

/// FFmpeg process wrapper for encoding
pub struct FfmpegEncoder {
    config: EncoderConfig,
    process: Option<Child>,
    running: Arc<AtomicBool>,
    ffmpeg_path: String,
    frames_encoded: Arc<Mutex<u64>>,
    encoding_fps: Arc<Mutex<f64>>,
}

impl FfmpegEncoder {
    pub fn new(config: EncoderConfig) -> Self {
        let ffmpeg_path = find_ffmpeg();

        Self {
            config,
            process: None,
            running: Arc::new(AtomicBool::new(false)),
            ffmpeg_path,
            frames_encoded: Arc::new(Mutex::new(0)),
            encoding_fps: Arc::new(Mutex::new(0.0)),
        }
    }

    /// Start encoding to a file
    pub fn start_file_encode(&mut self, output_path: &str) -> anyhow::Result<()> {
        let args = self.config.build_ffmpeg_args(output_path);

        info!("Starting FFmpeg encoder: {} {}", self.ffmpeg_path, args.join(" "));

        let child = Command::new(&self.ffmpeg_path)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start FFmpeg: {}. Is FFmpeg installed?", e))?;

        self.process = Some(child);
        self.running.store(true, Ordering::SeqCst);
        *self.frames_encoded.lock() = 0;

        info!("FFmpeg encoder started: {}", self.config.display_name());
        Ok(())
    }

    /// Start encoding for RTMP streaming
    pub fn start_stream_encode(&mut self, rtmp_url: &str) -> anyhow::Result<()> {
        let mut args = Vec::new();

        // Input: raw video from pipe
        args.extend_from_slice(&[
            "-y".to_string(),
            "-f".to_string(), "rawvideo".to_string(),
            "-pix_fmt".to_string(), self.config.pixel_format.clone(),
            "-s".to_string(), format!("{}x{}", self.config.width, self.config.height),
            "-r".to_string(), self.config.fps.to_string(),
            "-i".to_string(), "pipe:0".to_string(),
        ]);

        // Video encoder
        args.extend_from_slice(&[
            "-c:v".to_string(), self.config.ffmpeg_encoder_name().to_string(),
        ]);

        // Streaming-optimized rate control (always CBR for streaming)
        args.extend_from_slice(&[
            "-b:v".to_string(), format!("{}k", self.config.bitrate_kbps),
            "-maxrate".to_string(), format!("{}k", self.config.bitrate_kbps),
            "-bufsize".to_string(), format!("{}k", self.config.bitrate_kbps * 2),
        ]);

        // Preset
        let preset = self.config.preset.to_ffmpeg_preset(&self.config.encoder);
        args.extend_from_slice(&["-preset".to_string(), preset.to_string()]);

        // Profile
        if self.config.codec == Codec::H264 {
            args.extend_from_slice(&["-profile:v".to_string(), "high".to_string()]);
        }

        // Keyframe interval (2s for streaming)
        args.extend_from_slice(&[
            "-g".to_string(), (self.config.fps * 2).to_string(),
            "-keyint_min".to_string(), (self.config.fps * 2).to_string(),
        ]);

        // Pixel format
        args.extend_from_slice(&["-pix_fmt".to_string(), "yuv420p".to_string()]);

        // FLV output for RTMP
        args.extend_from_slice(&[
            "-f".to_string(), "flv".to_string(),
            rtmp_url.to_string(),
        ]);

        info!("Starting FFmpeg streaming encoder");

        let child = Command::new(&self.ffmpeg_path)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start FFmpeg for streaming: {}", e))?;

        self.process = Some(child);
        self.running.store(true, Ordering::SeqCst);
        *self.frames_encoded.lock() = 0;

        info!("FFmpeg streaming encoder started");
        Ok(())
    }

    /// Send a raw frame to the encoder
    pub fn send_frame(&mut self, frame_data: &[u8]) -> anyhow::Result<()> {
        if let Some(ref mut process) = self.process {
            if let Some(ref mut stdin) = process.stdin {
                stdin.write_all(frame_data)?;
                *self.frames_encoded.lock() += 1;
                return Ok(());
            }
        }
        Err(anyhow::anyhow!("Encoder not running"))
    }

    /// Stop encoding
    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(mut process) = self.process.take() {
            // Close stdin to signal end of input
            drop(process.stdin.take());

            // Wait for FFmpeg to finish
            match process.wait() {
                Ok(status) => {
                    if status.success() {
                        info!("FFmpeg encoder finished successfully");
                    } else {
                        warn!("FFmpeg encoder exited with status: {}", status);
                    }
                }
                Err(e) => {
                    error!("Failed to wait for FFmpeg: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Get number of frames encoded
    pub fn frames_encoded(&self) -> u64 {
        *self.frames_encoded.lock()
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn config(&self) -> &EncoderConfig {
        &self.config
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Find FFmpeg executable path
pub fn find_ffmpeg() -> String {
    // Check bundled FFmpeg first
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(dir) = &exe_dir {
        let bundled = dir.join("ffmpeg.exe");
        if bundled.exists() {
            return bundled.to_string_lossy().to_string();
        }

        // Check resources directory
        let resources = dir.join("resources").join("ffmpeg.exe");
        if resources.exists() {
            return resources.to_string_lossy().to_string();
        }
    }

    // Check PATH
    if let Ok(output) = Command::new("where").arg("ffmpeg").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = path.lines().next() {
                return first_line.trim().to_string();
            }
        }
    }

    // Default — will fail with a clear error message
    "ffmpeg".to_string()
}

/// Check if FFmpeg is available
pub fn is_ffmpeg_available() -> bool {
    let ffmpeg = find_ffmpeg();
    Command::new(&ffmpeg)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get FFmpeg version string
pub fn ffmpeg_version() -> Option<String> {
    let ffmpeg = find_ffmpeg();
    Command::new(&ffmpeg)
        .args(&["-version"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout)
                    .ok()
                    .and_then(|s| s.lines().next().map(|l| l.to_string()))
            } else {
                None
            }
        })
}

/// Probe available FFmpeg encoders
pub fn probe_ffmpeg_encoders() -> Vec<String> {
    let ffmpeg = find_ffmpeg();
    let output = Command::new(&ffmpeg)
        .args(&["-encoders", "-hide_banner"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let relevant = ["h264_nvenc", "hevc_nvenc", "av1_nvenc",
                           "h264_amf", "hevc_amf", "av1_amf",
                           "h264_qsv", "hevc_qsv", "av1_qsv",
                           "libx264", "libx265"];

            relevant.iter()
                .filter(|e| text.contains(**e))
                .map(|e| e.to_string())
                .collect()
        }
        _ => Vec::new(),
    }
}
