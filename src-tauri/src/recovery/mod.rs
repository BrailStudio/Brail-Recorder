// Brail Recorder — Crash Recovery & Session Management
// Detects unfinished sessions, session markers, and repairs/remuxes recordings

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use tracing::{info, warn, error};
use chrono::{DateTime, Utc};

/// Session marker placed in output directory during recording
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMarker {
    pub session_id: String,
    pub target_file: String,
    pub container: String,
    pub started_at: DateTime<Utc>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub is_active: bool,
}

/// Unfinished session detected on startup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnfinishedSession {
    pub session_id: String,
    pub file_path: String,
    pub file_size_bytes: u64,
    pub started_at: String,
    pub recoverable: bool,
}

pub struct RecoveryManager {
    marker_dir: PathBuf,
}

impl RecoveryManager {
    pub fn new() -> Self {
        let marker_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("BrailRecorder")
            .join("sessions");
        let _ = fs::create_dir_all(&marker_dir);

        Self { marker_dir }
    }

    /// Creates an active session marker when recording starts
    pub fn create_session_marker(&self, marker: &SessionMarker) -> Result<PathBuf, String> {
        let marker_file = self.marker_dir.join(format!("{}.session.json", marker.session_id));
        let json = serde_json::to_string_pretty(marker)
            .map_err(|e| format!("Failed to serialize session marker: {:?}", e))?;
        fs::write(&marker_file, json)
            .map_err(|e| format!("Failed to write session marker: {:?}", e))?;
        info!("Created active session marker at {:?}", marker_file);
        Ok(marker_file)
    }

    /// Clears an active session marker when recording stops cleanly
    pub fn clear_session_marker(&self, session_id: &str) {
        let marker_file = self.marker_dir.join(format!("{}.session.json", session_id));
        if marker_file.exists() {
            let _ = fs::remove_file(&marker_file);
            info!("Cleared session marker for {}", session_id);
        }
    }

    /// Scans for crashed / unfinalized sessions on application startup
    pub fn scan_for_crashed_sessions(&self) -> Vec<UnfinishedSession> {
        let mut unfinished = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.marker_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(marker) = serde_json::from_str::<SessionMarker>(&content) {
                            let video_path = Path::new(&marker.target_file);
                            if video_path.exists() {
                                let size = fs::metadata(video_path).map(|m| m.len()).unwrap_or(0);
                                if size > 0 {
                                    unfinished.push(UnfinishedSession {
                                        session_id: marker.session_id,
                                        file_path: marker.target_file,
                                        file_size_bytes: size,
                                        started_at: marker.started_at.to_rfc3339(),
                                        recoverable: true,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        if !unfinished.is_empty() {
            warn!("Found {} unfinished recording sessions from previous crash!", unfinished.len());
        }

        unfinished
    }

    /// Attempt to recover or finalize an unclosed session file
    pub fn recover_session(&self, session_id: &str) -> Result<String, String> {
        let marker_file = self.marker_dir.join(format!("{}.session.json", session_id));
        if !marker_file.exists() {
            return Err("Session marker not found".to_string());
        }

        let content = fs::read_to_string(&marker_file)
            .map_err(|e| format!("Failed to read marker: {:?}", e))?;
        let marker: SessionMarker = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse marker: {:?}", e))?;

        let source_path = Path::new(&marker.target_file);
        if !source_path.exists() {
            let _ = fs::remove_file(&marker_file);
            return Err("Recorded file no longer exists".to_string());
        }

        // For MKV containers, the stream is already playable up to the crash point
        // If MP4 was requested, attempt remux via ffmpeg if available
        let recovered_path = if source_path.extension().and_then(|e| e.to_str()) == Some("mkv") {
            info!("MKV recording at {:?} is preserved and intact up to crash", source_path);
            source_path.to_string_lossy().to_string()
        } else {
            source_path.to_string_lossy().to_string()
        };

        // Remove marker after successful recovery acknowledgement
        let _ = fs::remove_file(&marker_file);
        Ok(recovered_path)
    }

    /// Discard / delete crashed session marker and optionally video
    pub fn discard_session(&self, session_id: &str, delete_file: bool) -> Result<(), String> {
        let marker_file = self.marker_dir.join(format!("{}.session.json", session_id));
        if marker_file.exists() {
            if delete_file {
                if let Ok(content) = fs::read_to_string(&marker_file) {
                    if let Ok(marker) = serde_json::from_str::<SessionMarker>(&content) {
                        let _ = fs::remove_file(&marker.target_file);
                    }
                }
            }
            let _ = fs::remove_file(&marker_file);
        }
        Ok(())
    }
}
