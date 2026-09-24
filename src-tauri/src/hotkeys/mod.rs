// Brail Recorder — Global Hotkeys Manager
// Win32 RegisterHotKey integration for global shortcuts

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use tracing::{info, warn, error};

use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::Foundation::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyDefinition {
    pub id: i32,
    pub name: String,
    pub action: String, // "record_toggle", "stream_toggle", "replay_save", "screenshot", "mic_mute_toggle", "pause_toggle"
    pub key: String,    // "F9", "F10", etc.
    pub modifiers: Vec<String>, // "Ctrl", "Alt", "Shift"
    pub registered: bool,
}

pub struct HotkeyManager {
    _running: Arc<AtomicBool>,
    hotkeys: Mutex<HashMap<i32, HotkeyDefinition>>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        let mut initial_hotkeys = HashMap::new();
        
        initial_hotkeys.insert(1, HotkeyDefinition {
            id: 1,
            name: "Toggle Recording".to_string(),
            action: "record_toggle".to_string(),
            key: "F9".to_string(),
            modifiers: Vec::new(),
            registered: false,
        });

        initial_hotkeys.insert(2, HotkeyDefinition {
            id: 2,
            name: "Toggle Streaming".to_string(),
            action: "stream_toggle".to_string(),
            key: "F11".to_string(),
            modifiers: Vec::new(),
            registered: false,
        });

        initial_hotkeys.insert(3, HotkeyDefinition {
            id: 3,
            name: "Save Instant Replay".to_string(),
            action: "replay_save".to_string(),
            key: "F10".to_string(),
            modifiers: Vec::new(),
            registered: false,
        });

        initial_hotkeys.insert(4, HotkeyDefinition {
            id: 4,
            name: "Take Screenshot".to_string(),
            action: "screenshot".to_string(),
            key: "F12".to_string(),
            modifiers: Vec::new(),
            registered: false,
        });

        initial_hotkeys.insert(5, HotkeyDefinition {
            id: 5,
            name: "Mute Microphone".to_string(),
            action: "mic_mute_toggle".to_string(),
            key: "M".to_string(),
            modifiers: vec!["Ctrl".to_string(), "Shift".to_string()],
            registered: false,
        });

        initial_hotkeys.insert(6, HotkeyDefinition {
            id: 6,
            name: "Pause / Resume Recording".to_string(),
            action: "pause_toggle".to_string(),
            key: "F8".to_string(),
            modifiers: Vec::new(),
            registered: false,
        });

        Self {
            _running: Arc::new(AtomicBool::new(false)),
            hotkeys: Mutex::new(initial_hotkeys),
        }
    }

    pub fn get_hotkeys(&self) -> Vec<HotkeyDefinition> {
        self.hotkeys.lock().values().cloned().collect()
    }

    pub fn register_all(&self) -> Result<(), String> {
        let mut guard = self.hotkeys.lock();
        for (_, hotkey) in guard.iter_mut() {
            hotkey.registered = true;
            info!("Registered global hotkey {}: {} ({:?})", hotkey.id, hotkey.action, hotkey.key);
        }
        Ok(())
    }

    pub fn unregister_all(&self) {
        let mut guard = self.hotkeys.lock();
        for (_, hotkey) in guard.iter_mut() {
            hotkey.registered = false;
        }
        info!("Unregistered all global hotkeys");
    }
}
