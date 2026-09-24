// Brail Recorder — Core Tauri Application Backend
// Integrates capture, encoder, audio, streaming, replay, hardware, performance, and recovery.

pub mod audio;
pub mod capture;
pub mod encoder;
pub mod hardware;
pub mod hotkeys;
pub mod performance;
pub mod recording;
pub mod recovery;
pub mod replay;
pub mod security;
pub mod storage;
pub mod streaming;

use std::sync::Arc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};

use audio::{AudioConfig, AudioEngine};
use hardware::{AudioDeviceInfo, CameraInfo, HardwareDetector, MonitorInfo, SystemCapabilities};
use performance::{AdaptiveMode, PerformanceMonitor, PerformanceSnapshot};
use recording::{RecordingConfig, RecordingPipeline, RecordingStats};
use recovery::{RecoveryManager, UnfinishedSession};
use replay::{ReplayBuffer, ReplayStatus};
use security::SecureVault;
use storage::{AppConfig, StorageManager};
use streaming::{StreamHealth, StreamProfile, StreamingEngine, StreamingStats};

/// Shared state container for Tauri app
pub struct AppState {
    pub detector: HardwareDetector,
    pub storage: StorageManager,
    pub recovery: RecoveryManager,
    pub performance: Arc<PerformanceMonitor>,
    pub recording_pipeline: Mutex<Option<RecordingPipeline>>,
    pub streaming_engine: Mutex<Option<StreamingEngine>>,
    pub replay_buffer: Mutex<Option<Arc<ReplayBuffer>>>,
    pub audio_engine: Mutex<Option<Arc<AudioEngine>>>,
    pub hotkeys: hotkeys::HotkeyManager,
}

// -----------------------------------------------------------------------------
// Tauri Commands
// -----------------------------------------------------------------------------

#[tauri::command]
pub fn get_hardware_info(state: tauri::State<Arc<AppState>>) -> SystemCapabilities {
    state.detector.detect_capabilities()
}

#[tauri::command]
pub fn get_monitors(state: tauri::State<Arc<AppState>>) -> Vec<MonitorInfo> {
    state.detector.detect_monitors()
}

#[tauri::command]
pub fn get_audio_devices(state: tauri::State<Arc<AppState>>) -> Vec<AudioDeviceInfo> {
    state.detector.detect_audio_devices()
}

#[tauri::command]
pub fn get_cameras(state: tauri::State<Arc<AppState>>) -> Vec<CameraInfo> {
    state.detector.detect_cameras()
}

#[tauri::command]
pub fn get_performance_snapshot(
    state: tauri::State<Arc<AppState>>,
    target_fps: Option<f32>,
) -> PerformanceSnapshot {
    state.performance.sample(target_fps.unwrap_or(60.0))
}

#[tauri::command]
pub fn set_adaptive_mode(state: tauri::State<Arc<AppState>>, mode: String) -> Result<(), String> {
    let adaptive_mode = match mode.to_lowercase().as_str() {
        "ultralite" | "ultra_lite" => AdaptiveMode::UltraLite,
        "lowend" | "low_end" => AdaptiveMode::LowEnd,
        "balanced" => AdaptiveMode::Balanced,
        "quality" => AdaptiveMode::Quality,
        "streaming" => AdaptiveMode::Streaming,
        _ => AdaptiveMode::Custom,
    };
    state.performance.set_mode(adaptive_mode);
    Ok(())
}

#[tauri::command]
pub fn get_config(state: tauri::State<Arc<AppState>>) -> AppConfig {
    state.storage.load_config()
}

#[tauri::command]
pub fn save_config(state: tauri::State<Arc<AppState>>, config: AppConfig) -> Result<(), String> {
    state.storage.save_config(&config)
}

#[tauri::command]
pub fn save_stream_key(key: String) -> Result<Vec<u8>, String> {
    SecureVault::encrypt_secret(&key)
}

#[tauri::command]
pub fn get_decrypted_stream_key(encrypted: Vec<u8>) -> Result<String, String> {
    SecureVault::decrypt_secret(&encrypted)
}

#[tauri::command]
pub fn start_recording(
    state: tauri::State<Arc<AppState>>,
    config: RecordingConfig,
) -> Result<String, String> {
    let mut pipeline_guard = state.recording_pipeline.lock();
    if let Some(ref p) = *pipeline_guard {
        if p.is_recording() {
            return Err("Recording is already in progress".to_string());
        }
    }

    let mut pipeline = RecordingPipeline::new();
    let filepath = pipeline.start(config)?;
    *pipeline_guard = Some(pipeline);
    info!("Recording started successfully: {}", filepath);
    Ok(filepath)
}

#[tauri::command]
pub fn stop_recording(state: tauri::State<Arc<AppState>>) -> Result<recording::RecordingResult, String> {
    let mut pipeline_guard = state.recording_pipeline.lock();
    if let Some(mut pipeline) = pipeline_guard.take() {
        pipeline.stop()
    } else {
        Err("No active recording session".to_string())
    }
}

#[tauri::command]
pub fn pause_recording(state: tauri::State<Arc<AppState>>) -> Result<(), String> {
    let pipeline_guard = state.recording_pipeline.lock();
    if let Some(ref pipeline) = *pipeline_guard {
        pipeline.pause();
        Ok(())
    } else {
        Err("No active recording session".to_string())
    }
}

#[tauri::command]
pub fn resume_recording(state: tauri::State<Arc<AppState>>) -> Result<(), String> {
    let pipeline_guard = state.recording_pipeline.lock();
    if let Some(ref pipeline) = *pipeline_guard {
        pipeline.resume();
        Ok(())
    } else {
        Err("No active recording session".to_string())
    }
}

#[tauri::command]
pub fn get_recording_stats(state: tauri::State<Arc<AppState>>) -> Option<RecordingStats> {
    let pipeline_guard = state.recording_pipeline.lock();
    pipeline_guard.as_ref().map(|p| p.get_stats())
}

#[tauri::command]
pub fn start_streaming(
    state: tauri::State<Arc<AppState>>,
    profile: StreamProfile,
) -> Result<(), String> {
    let mut stream_guard = state.streaming_engine.lock();
    if let Some(ref s) = *stream_guard {
        if s.is_streaming() {
            return Err("Streaming is already active".to_string());
        }
    }

    let mut engine = StreamingEngine::new();
    engine.start(profile)?;
    *stream_guard = Some(engine);
    info!("Streaming started successfully");
    Ok(())
}

#[tauri::command]
pub fn stop_streaming(state: tauri::State<Arc<AppState>>) -> Result<(), String> {
    let mut stream_guard = state.streaming_engine.lock();
    if let Some(ref mut engine) = *stream_guard {
        engine.stop();
        *stream_guard = None;
        info!("Streaming stopped cleanly");
        Ok(())
    } else {
        Err("No active streaming session".to_string())
    }
}

#[tauri::command]
pub fn test_stream_connection(server_url: String, stream_key: String) -> Result<String, String> {
    let safe_url = SecureVault::redact(&server_url, Some(&stream_key));
    info!("Testing connection to endpoint {}", safe_url);
    streaming::test_rtmp_connection(&server_url, &stream_key)
}

#[tauri::command]
pub fn get_streaming_stats(state: tauri::State<Arc<AppState>>) -> Option<StreamingStats> {
    let stream_guard = state.streaming_engine.lock();
    stream_guard.as_ref().map(|s| s.get_stats())
}

#[tauri::command]
pub fn save_replay(
    state: tauri::State<Arc<AppState>>,
    output_dir: Option<String>,
) -> Result<String, String> {
    let replay_guard = state.replay_buffer.lock();
    if let Some(ref replay) = *replay_guard {
        let dir = output_dir.unwrap_or_else(|| {
            dirs::video_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("BrailRecordings")
                .join("Replays")
                .to_string_lossy()
                .to_string()
        });
        replay.save_replay(&dir)
    } else {
        Err("Instant replay buffer is not currently active".to_string())
    }
}

#[tauri::command]
pub fn get_replay_status(state: tauri::State<Arc<AppState>>) -> ReplayStatus {
    let replay_guard = state.replay_buffer.lock();
    if let Some(ref replay) = *replay_guard {
        replay.get_status()
    } else {
        ReplayStatus {
            buffer_duration_secs: 30,
            stored_duration_secs: 0.0,
            stored_frames: 0,
            buffer_memory_mb: 0.0,
            is_active: false,
            fill_percent: 0.0,
        }
    }
}

#[tauri::command]
pub fn take_screenshot(output_dir: Option<String>) -> Result<String, String> {
    let dir = output_dir.unwrap_or_else(|| {
        dirs::picture_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("BrailScreenshots")
            .to_string_lossy()
            .to_string()
    });
    let _ = std::fs::create_dir_all(&dir);
    let target = storage::StorageManager::generate_safe_filename("Brail_Screenshot", "png", &dir);
    
    // Primary display screenshot capture
    capture::take_fullscreen_screenshot(&target.to_string_lossy())
}

#[tauri::command]
pub fn scan_crashed_sessions(state: tauri::State<Arc<AppState>>) -> Vec<UnfinishedSession> {
    state.recovery.scan_for_crashed_sessions()
}

#[tauri::command]
pub fn recover_session(state: tauri::State<Arc<AppState>>, session_id: String) -> Result<String, String> {
    state.recovery.recover_session(&session_id)
}

#[tauri::command]
pub fn discard_session(
    state: tauri::State<Arc<AppState>>,
    session_id: String,
    delete_file: bool,
) -> Result<(), String> {
    state.recovery.discard_session(&session_id, delete_file)
}

// -----------------------------------------------------------------------------
// Application Entrypoint
// -----------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = Arc::new(AppState {
        detector: HardwareDetector::new(),
        storage: StorageManager::new(),
        recovery: RecoveryManager::new(),
        performance: Arc::new(PerformanceMonitor::new()),
        recording_pipeline: Mutex::new(None),
        streaming_engine: Mutex::new(None),
        replay_buffer: Mutex::new(None),
        audio_engine: Mutex::new(None),
        hotkeys: hotkeys::HotkeyManager::new(),
    });

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            get_hardware_info,
            get_monitors,
            get_audio_devices,
            get_cameras,
            get_performance_snapshot,
            set_adaptive_mode,
            get_config,
            save_config,
            save_stream_key,
            get_decrypted_stream_key,
            start_recording,
            stop_recording,
            pause_recording,
            resume_recording,
            get_recording_stats,
            start_streaming,
            stop_streaming,
            test_stream_connection,
            get_streaming_stats,
            save_replay,
            get_replay_status,
            take_screenshot,
            scan_crashed_sessions,
            recover_session,
            discard_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Brail Recorder");
}
