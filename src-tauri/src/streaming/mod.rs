// Brail Recorder — Streaming Engine
// RTMP/RTMPS streaming to YouTube and custom endpoints

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use tracing::{info, warn, error};
use std::time::Instant;

/// Streaming platform
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StreamPlatform {
    YouTube,
    Twitch,
    Facebook,
    CustomRtmp,
    CustomSrt,
}

impl StreamPlatform {
    pub fn default_server_url(&self) -> &str {
        match self {
            Self::YouTube => "rtmp://a.rtmp.youtube.com/live2",
            Self::Twitch => "rtmp://live.twitch.tv/app",
            Self::Facebook => "rtmps://live-api-s.facebook.com:443/rtmp",
            Self::CustomRtmp => "",
            Self::CustomSrt => "",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::YouTube => "YouTube",
            Self::Twitch => "Twitch",
            Self::Facebook => "Facebook",
            Self::CustomRtmp => "Custom RTMP",
            Self::CustomSrt => "Custom SRT",
        }
    }
}

/// Stream quality preset
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StreamQualityPreset {
    UltraLow,   // 360p
    Low,         // 480p
    Balanced,    // 720p60
    FullHD,      // 1080p60
    High,        // 1440p60
    Ultra,       // 4K60
    Custom,
}

impl StreamQualityPreset {
    pub fn resolution(&self) -> (u32, u32) {
        match self {
            Self::UltraLow => (640, 360),
            Self::Low => (854, 480),
            Self::Balanced => (1280, 720),
            Self::FullHD => (1920, 1080),
            Self::High => (2560, 1440),
            Self::Ultra => (3840, 2160),
            Self::Custom => (1920, 1080),
        }
    }

    pub fn fps(&self) -> u32 {
        match self {
            Self::UltraLow => 30,
            Self::Low => 30,
            _ => 60,
        }
    }

    pub fn recommended_bitrate_kbps(&self) -> u32 {
        match self {
            Self::UltraLow => 1000,
            Self::Low => 2500,
            Self::Balanced => 4500,
            Self::FullHD => 6000,
            Self::High => 12000,
            Self::Ultra => 25000,
            Self::Custom => 6000,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::UltraLow => "Ultra Low (360p)",
            Self::Low => "Low (480p)",
            Self::Balanced => "Balanced (720p60)",
            Self::FullHD => "Full HD (1080p60)",
            Self::High => "High (1440p60)",
            Self::Ultra => "Ultra (4K60)",
            Self::Custom => "Custom",
        }
    }
}

/// Stream profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamProfile {
    pub name: String,
    pub platform: StreamPlatform,
    pub server_url: String,
    pub quality_preset: StreamQualityPreset,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub codec: String,
    pub keyframe_interval: u32,
    pub audio_bitrate_kbps: u32,
    pub audio_sample_rate: u32,
}

impl Default for StreamProfile {
    fn default() -> Self {
        Self {
            name: "YouTube 1080p60".to_string(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".to_string(),
            quality_preset: StreamQualityPreset::FullHD,
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 6000,
            codec: "h264".to_string(),
            keyframe_interval: 2,
            audio_bitrate_kbps: 128,
            audio_sample_rate: 48000,
        }
    }
}

/// Stream health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StreamHealthStatus {
    Excellent,
    Good,
    Fair,
    Poor,
    Critical,
    Disconnected,
}

/// Real-time stream statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamStats {
    pub is_live: bool,
    pub health: StreamHealthStatus,
    pub resolution: String,
    pub fps: u32,
    pub bitrate_kbps: f64,
    pub upload_speed_kbps: f64,
    pub dropped_frames: u64,
    pub total_frames: u64,
    pub drop_percentage: f64,
    pub encoder_name: String,
    pub duration_seconds: u64,
    pub reconnect_count: u32,
    pub network_status: String,
}

impl Default for StreamStats {
    fn default() -> Self {
        Self {
            is_live: false,
            health: StreamHealthStatus::Disconnected,
            resolution: "1920x1080".to_string(),
            fps: 60,
            bitrate_kbps: 0.0,
            upload_speed_kbps: 0.0,
            dropped_frames: 0,
            total_frames: 0,
            drop_percentage: 0.0,
            encoder_name: String::new(),
            duration_seconds: 0,
            reconnect_count: 0,
            network_status: "Disconnected".to_string(),
        }
    }
}

/// Streaming configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    pub profile: StreamProfile,
    pub stream_key: String,
    pub auto_reconnect: bool,
    pub reconnect_delay_seconds: u32,
    pub max_reconnect_attempts: u32,
    pub record_while_streaming: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            profile: StreamProfile::default(),
            stream_key: String::new(),
            auto_reconnect: true,
            reconnect_delay_seconds: 5,
            max_reconnect_attempts: 10,
            record_while_streaming: false,
        }
    }
}

/// Streaming engine state
pub struct StreamEngine {
    config: StreamConfig,
    running: Arc<AtomicBool>,
    stats: Arc<Mutex<StreamStats>>,
    start_time: Option<Instant>,
    bytes_sent: Arc<Mutex<u64>>,
    last_bytes_check: Arc<Mutex<Instant>>,
}

impl StreamEngine {
    pub fn new(config: StreamConfig) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(StreamStats::default())),
            start_time: None,
            bytes_sent: Arc::new(Mutex::new(0)),
            last_bytes_check: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Build the full RTMP URL (server + stream key)
    pub fn build_rtmp_url(&self) -> String {
        let server = &self.config.profile.server_url;
        let key = &self.config.stream_key;

        if server.ends_with('/') {
            format!("{}{}", server, key)
        } else {
            format!("{}/{}", server, key)
        }
    }

    /// Get current stream stats
    pub fn get_stats(&self) -> StreamStats {
        let mut stats = self.stats.lock().clone();

        if let Some(start) = &self.start_time {
            stats.duration_seconds = start.elapsed().as_secs();
        }

        // Calculate upload speed
        let bytes = *self.bytes_sent.lock();
        let mut last_check = self.last_bytes_check.lock();
        let elapsed = last_check.elapsed().as_secs_f64();
        if elapsed > 0.5 {
            stats.upload_speed_kbps = (bytes as f64 * 8.0) / (elapsed * 1000.0);
            *last_check = Instant::now();
            *self.bytes_sent.lock() = 0;
        }

        // Determine health
        stats.health = if !stats.is_live {
            StreamHealthStatus::Disconnected
        } else if stats.drop_percentage > 5.0 {
            StreamHealthStatus::Critical
        } else if stats.drop_percentage > 2.0 {
            StreamHealthStatus::Poor
        } else if stats.drop_percentage > 0.5 {
            StreamHealthStatus::Fair
        } else if stats.upload_speed_kbps < self.config.profile.bitrate_kbps as f64 * 0.8 {
            StreamHealthStatus::Fair
        } else {
            StreamHealthStatus::Excellent
        };

        stats.network_status = match &stats.health {
            StreamHealthStatus::Excellent => "Excellent".to_string(),
            StreamHealthStatus::Good => "Good".to_string(),
            StreamHealthStatus::Fair => "Fair".to_string(),
            StreamHealthStatus::Poor => "Poor".to_string(),
            StreamHealthStatus::Critical => "Critical".to_string(),
            StreamHealthStatus::Disconnected => "Disconnected".to_string(),
        };

        stats
    }

    /// Mark stream as started
    pub fn mark_started(&mut self, encoder_name: &str) {
        self.start_time = Some(Instant::now());
        self.running.store(true, Ordering::SeqCst);

        let mut stats = self.stats.lock();
        stats.is_live = true;
        stats.encoder_name = encoder_name.to_string();
        stats.resolution = format!("{}x{}", self.config.profile.width, self.config.profile.height);
        stats.fps = self.config.profile.fps;
        stats.bitrate_kbps = self.config.profile.bitrate_kbps as f64;
        stats.dropped_frames = 0;
        stats.total_frames = 0;

        info!("Stream marked as started");
    }

    /// Mark stream as stopped
    pub fn mark_stopped(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.start_time = None;

        let mut stats = self.stats.lock();
        stats.is_live = false;
        stats.health = StreamHealthStatus::Disconnected;

        info!("Stream marked as stopped");
    }

    /// Update frame statistics
    pub fn update_frame_stats(&self, dropped: bool) {
        let mut stats = self.stats.lock();
        stats.total_frames += 1;
        if dropped {
            stats.dropped_frames += 1;
        }
        if stats.total_frames > 0 {
            stats.drop_percentage = (stats.dropped_frames as f64 / stats.total_frames as f64) * 100.0;
        }
    }

    /// Record bytes sent for bandwidth calculation
    pub fn record_bytes_sent(&self, bytes: u64) {
        *self.bytes_sent.lock() += bytes;
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn config(&self) -> &StreamConfig {
        &self.config
    }

    pub fn update_config(&mut self, config: StreamConfig) {
        self.config = config;
    }
}

/// Test connection to streaming endpoint (without stream key for basic connectivity)
pub fn test_connection(server_url: &str) -> anyhow::Result<String> {
    use std::net::TcpStream;
    use std::time::Duration;

    // Parse the URL to get host and port
    let url = server_url.trim_start_matches("rtmp://").trim_start_matches("rtmps://");
    let (host, port) = if url.contains(':') {
        let parts: Vec<&str> = url.splitn(2, ':').collect();
        let port_str = parts[1].split('/').next().unwrap_or("1935");
        (parts[0], port_str.parse::<u16>().unwrap_or(1935))
    } else {
        let host = url.split('/').next().unwrap_or(url);
        let port = if server_url.starts_with("rtmps://") { 443 } else { 1935 };
        (host, port)
    };

    let addr = format!("{}:{}", host, port);
    info!("Testing connection to {}", addr);

    match TcpStream::connect_timeout(&addr.parse().map_err(|e| anyhow::anyhow!("Invalid address: {}", e))?, Duration::from_secs(5)) {
        Ok(_) => {
            info!("Connection test successful: {}", addr);
            Ok(format!("Connection successful to {}", host))
        }
        Err(e) => {
            warn!("Connection test failed: {}", e);
            Err(anyhow::anyhow!("Could not connect to {}: {}", host, e))
        }
    }
}

/// YouTube streaming presets
pub fn youtube_presets() -> Vec<StreamProfile> {
    vec![
        StreamProfile {
            name: "YouTube 720p30".into(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".into(),
            quality_preset: StreamQualityPreset::Balanced,
            width: 1280, height: 720, fps: 30,
            bitrate_kbps: 2500, codec: "h264".into(),
            keyframe_interval: 2, audio_bitrate_kbps: 128, audio_sample_rate: 48000,
        },
        StreamProfile {
            name: "YouTube 720p60".into(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".into(),
            quality_preset: StreamQualityPreset::Balanced,
            width: 1280, height: 720, fps: 60,
            bitrate_kbps: 4500, codec: "h264".into(),
            keyframe_interval: 2, audio_bitrate_kbps: 128, audio_sample_rate: 48000,
        },
        StreamProfile {
            name: "YouTube 1080p30".into(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".into(),
            quality_preset: StreamQualityPreset::FullHD,
            width: 1920, height: 1080, fps: 30,
            bitrate_kbps: 4500, codec: "h264".into(),
            keyframe_interval: 2, audio_bitrate_kbps: 128, audio_sample_rate: 48000,
        },
        StreamProfile {
            name: "YouTube 1080p60".into(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".into(),
            quality_preset: StreamQualityPreset::FullHD,
            width: 1920, height: 1080, fps: 60,
            bitrate_kbps: 6000, codec: "h264".into(),
            keyframe_interval: 2, audio_bitrate_kbps: 128, audio_sample_rate: 48000,
        },
        StreamProfile {
            name: "YouTube 1440p30".into(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".into(),
            quality_preset: StreamQualityPreset::High,
            width: 2560, height: 1440, fps: 30,
            bitrate_kbps: 9000, codec: "h264".into(),
            keyframe_interval: 2, audio_bitrate_kbps: 128, audio_sample_rate: 48000,
        },
        StreamProfile {
            name: "YouTube 1440p60".into(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".into(),
            quality_preset: StreamQualityPreset::High,
            width: 2560, height: 1440, fps: 60,
            bitrate_kbps: 12000, codec: "h264".into(),
            keyframe_interval: 2, audio_bitrate_kbps: 128, audio_sample_rate: 48000,
        },
        StreamProfile {
            name: "YouTube 4K30".into(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".into(),
            quality_preset: StreamQualityPreset::Ultra,
            width: 3840, height: 2160, fps: 30,
            bitrate_kbps: 18000, codec: "h264".into(),
            keyframe_interval: 2, audio_bitrate_kbps: 128, audio_sample_rate: 48000,
        },
        StreamProfile {
            name: "YouTube 4K60".into(),
            platform: StreamPlatform::YouTube,
            server_url: "rtmp://a.rtmp.youtube.com/live2".into(),
            quality_preset: StreamQualityPreset::Ultra,
            width: 3840, height: 2160, fps: 60,
            bitrate_kbps: 25000, codec: "h264".into(),
            keyframe_interval: 2, audio_bitrate_kbps: 128, audio_sample_rate: 48000,
        },
    ]
}
