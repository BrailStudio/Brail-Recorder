// Brail Recorder — Settings & Profile Storage
// Manages configuration persistence, profiles, directories, and safe filenames

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use tracing::{info, warn, error};
use chrono::Local;

use crate::hardware::{Codec, HardwareEncoder};
use crate::performance::AdaptiveMode;

/// Master application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub recording: RecordingSettings,
    pub streaming: StreamingConfig,
    pub replay: ReplayConfig,
    pub audio: AudioSettings,
    pub performance: PerformanceSettings,
    pub hotkeys: HotkeyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub recording_dir: String,
    pub replay_dir: String,
    pub screenshot_dir: String,
    pub minimize_to_tray: bool,
    pub start_with_windows: bool,
    pub check_for_updates: bool,
    pub theme: String, // "dark", "light", "amoled"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub encoder: HardwareEncoder,
    pub codec: Codec,
    pub bitrate_kbps: u32,
    pub container: String, // "mkv", "mp4", "webm"
    pub auto_remux_to_mp4: bool,
    pub capture_cursor: bool,
    pub highlight_cursor: bool,
    pub monitor_id: String,
    pub custom_resolution: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    pub platform: String, // "youtube", "twitch", "custom"
    pub server_url: String,
    pub stream_key_encrypted: Vec<u8>,
    pub stream_key_cached: String, // In-memory only (redacted in serializations if desired)
    pub bitrate_kbps: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub encoder: HardwareEncoder,
    pub auto_reconnect: bool,
    pub max_reconnect_attempts: u32,
    pub reconnect_delay_sec: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    pub enabled: bool,
    pub buffer_seconds: u32, // 15, 30, 60, 120, 300
    pub save_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub system_audio_enabled: bool,
    pub mic_enabled: bool,
    pub system_volume: f32, // 0.0 - 1.0
    pub mic_volume: f32,    // 0.0 - 1.0
    pub system_device_id: String,
    pub mic_device_id: String,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSettings {
    pub adaptive_mode: AdaptiveMode,
    pub low_overhead_mode: bool,
    pub preview_enabled: bool,
    pub target_fps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub record_toggle: String,  // e.g. "F9"
    pub stream_toggle: String,  // e.g. "F11"
    pub replay_save: String,    // e.g. "F10"
    pub screenshot: String,     // e.g. "F12"
    pub mic_mute_toggle: String,// e.g. "Ctrl+Shift+M"
    pub pause_toggle: String,   // e.g. "F8"
}

impl Default for AppConfig {
    fn default() -> Self {
        let videos_dir = dirs::video_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\Recordings"))
            .join("BrailRecordings");
        let pictures_dir = dirs::picture_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\Screenshots"))
            .join("BrailScreenshots");

        let recording_dir_str = videos_dir.to_string_lossy().to_string();
        let replay_dir_str = videos_dir.join("Replays").to_string_lossy().to_string();
        let screenshot_dir_str = pictures_dir.to_string_lossy().to_string();

        Self {
            general: GeneralConfig {
                recording_dir: recording_dir_str,
                replay_dir: replay_dir_str,
                screenshot_dir: screenshot_dir_str,
                minimize_to_tray: true,
                start_with_windows: false,
                check_for_updates: true,
                theme: "dark".to_string(),
            },
            recording: RecordingSettings {
                width: 1920,
                height: 1080,
                fps: 60,
                encoder: HardwareEncoder::None, // Auto-detected on startup
                codec: Codec::H264,
                bitrate_kbps: 12000,
                container: "mkv".to_string(),
                auto_remux_to_mp4: true,
                capture_cursor: true,
                highlight_cursor: false,
                monitor_id: "primary".to_string(),
                custom_resolution: false,
            },
            streaming: StreamingConfig {
                platform: "youtube".to_string(),
                server_url: "rtmp://a.rtmp.youtube.com/live2".to_string(),
                stream_key_encrypted: Vec::new(),
                stream_key_cached: String::new(),
                bitrate_kbps: 6500,
                width: 1920,
                height: 1080,
                fps: 60,
                encoder: HardwareEncoder::None,
                auto_reconnect: true,
                max_reconnect_attempts: 5,
                reconnect_delay_sec: 3,
            },
            replay: ReplayConfig {
                enabled: true,
                buffer_seconds: 30,
                save_dir: videos_dir.join("Replays").to_string_lossy().to_string(),
            },
            audio: AudioSettings {
                system_audio_enabled: true,
                mic_enabled: false,
                system_volume: 1.0,
                mic_volume: 1.0,
                system_device_id: "default".to_string(),
                mic_device_id: "default".to_string(),
                sample_rate: 48000,
            },
            performance: PerformanceSettings {
                adaptive_mode: AdaptiveMode::Balanced,
                low_overhead_mode: true,
                preview_enabled: true,
                target_fps: 60,
            },
            hotkeys: HotkeyConfig {
                record_toggle: "F9".to_string(),
                stream_toggle: "F11".to_string(),
                replay_save: "F10".to_string(),
                screenshot: "F12".to_string(),
                mic_mute_toggle: "Ctrl+Shift+M".to_string(),
                pause_toggle: "F8".to_string(),
            },
        }
    }
}

/// Settings manager with file persistence
pub struct StorageManager {
    config_path: PathBuf,
}

impl StorageManager {
    pub fn new() -> Self {
        let app_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("BrailRecorder");
        
        let _ = fs::create_dir_all(&app_dir);
        let config_path = app_dir.join("config.json");

        Self { config_path }
    }

    /// Load config from disk or return default
    pub fn load_config(&self) -> AppConfig {
        if self.config_path.exists() {
            match fs::read_to_string(&self.config_path) {
                Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
                    Ok(mut config) => {
                        info!("Loaded configuration from {:?}", self.config_path);
                        // Ensure dirs exist
                        let _ = fs::create_dir_all(&config.general.recording_dir);
                        let _ = fs::create_dir_all(&config.general.replay_dir);
                        let _ = fs::create_dir_all(&config.general.screenshot_dir);
                        return config;
                    }
                    Err(e) => {
                        warn!("Failed to parse config JSON ({:?}), loading default", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read config file ({:?}), loading default", e);
                }
            }
        }

        let default_config = AppConfig::default();
        let _ = self.save_config(&default_config);
        default_config
    }

    /// Save config to disk
    pub fn save_config(&self, config: &AppConfig) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Ensure directories exist
        let _ = fs::create_dir_all(&config.general.recording_dir);
        let _ = fs::create_dir_all(&config.general.replay_dir);
        let _ = fs::create_dir_all(&config.general.screenshot_dir);

        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize config: {:?}", e))?;
        fs::write(&self.config_path, json)
            .map_err(|e| format!("Failed to write config file: {:?}", e))?;
        info!("Saved configuration to {:?}", self.config_path);
        Ok(())
    }

    /// Generate safe, collision-free filename
    pub fn generate_safe_filename(prefix: &str, ext: &str, output_dir: &str) -> PathBuf {
        let dir = Path::new(output_dir);
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
        let mut filename = format!("{}_{}.{}", prefix, timestamp, ext);
        let mut path = dir.join(&filename);

        let mut counter = 1;
        while path.exists() {
            filename = format!("{}_{}_{}.{}", prefix, timestamp, counter, ext);
            path = dir.join(&filename);
            counter += 1;
        }

        path
    }
}
