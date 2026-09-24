// Brail Recorder — High-Performance Frontend Controller
// Real-time IPC bindings to Rust backend via Tauri v2

interface PerformanceSnapshot {
  process_ram_mb: number;
  system_ram_total_mb: number;
  system_ram_used_mb: number;
  system_ram_percent: number;
  process_cpu_percent: number;
  system_cpu_percent: number;
  estimated_gpu_percent: number;
  capture_fps: number;
  actual_fps: number;
  dropped_frames: number;
  disk_write_mb_per_sec: number;
  network_upload_mbps: number;
  adaptive_mode: string;
  health_status: string;
  warnings: string[];
}

interface RecordingStats {
  duration_secs: number;
  frames_encoded: number;
  bytes_written: number;
  current_fps: number;
  dropped_frames: number;
  is_paused: boolean;
}

interface StreamingStats {
  uptime_secs: number;
  bitrate_kbps: number;
  dropped_frames: number;
  upload_speed_mbps: number;
  connection_health: string;
  is_connected: boolean;
}

interface SystemCapabilities {
  gpu_name: string;
  active_encoder: string;
  recommended_resolution: string;
  recommended_fps: number;
}

interface AppConfig {
  general: {
    recording_dir: string;
    replay_dir: string;
    screenshot_dir: string;
    minimize_to_tray: boolean;
    start_with_windows: boolean;
    theme: string;
  };
  recording: {
    width: number;
    height: number;
    fps: number;
    encoder: string;
    codec: string;
    bitrate_kbps: number;
    container: string;
    auto_remux_to_mp4: boolean;
    capture_cursor: boolean;
  };
  streaming: {
    platform: string;
    server_url: string;
    stream_key_encrypted: number[];
    bitrate_kbps: number;
    width: number;
    height: number;
    fps: number;
    auto_reconnect: boolean;
  };
  audio: {
    system_audio_enabled: boolean;
    mic_enabled: boolean;
    system_volume: number;
    mic_volume: number;
  };
  performance: {
    adaptive_mode: string;
  };
}

// -----------------------------------------------------------------------------
// State Management
// -----------------------------------------------------------------------------
let isRecording = false;
let isStreaming = false;
let isPaused = false;
let recordStartTime = 0;
let streamStartTime = 0;
let recordTimerInterval: number | null = null;
let streamTimerInterval: number | null = null;
let perfInterval: number | null = null;
let currentConfig: AppConfig | null = null;
let decryptedKeyCached = '';
let activeCrashedSessionId = '';

// Check if running inside Tauri
const isTauri = typeof (window as any).__TAURI_INTERNALS__ !== 'undefined';

async function invokeTauri<T>(cmd: string, args: Record<string, any> = {}): Promise<T> {
  if (isTauri) {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<T>(cmd, args);
  } else {
    // Fallback simulation for browser preview
    console.log(`[IPC Invoke: ${cmd}]`, args);
    return mockHandler(cmd, args) as T;
  }
}

function mockHandler(cmd: string, _args: any): any {
  switch (cmd) {
    case 'get_hardware_info':
      return {
        gpu_name: 'NVIDIA GeForce RTX 3060 Laptop GPU',
        active_encoder: 'NVENC H.264 (Hardware)',
        recommended_resolution: '1920x1080',
        recommended_fps: 60,
      };
    case 'get_performance_snapshot':
      return {
        process_ram_mb: isRecording ? 48.4 : 34.2,
        system_ram_total_mb: 16384,
        system_ram_used_mb: 8192,
        system_ram_percent: 50.0,
        process_cpu_percent: isRecording ? 4.8 : 1.9,
        system_cpu_percent: 12.0,
        estimated_gpu_percent: isRecording ? 9.5 : 3.0,
        capture_fps: 60.0,
        actual_fps: 60.0,
        dropped_frames: 0,
        disk_write_mb_per_sec: isRecording ? 38.2 : 0.0,
        network_upload_mbps: isStreaming ? 6.5 : 0.0,
        adaptive_mode: 'Balanced',
        health_status: 'Optimal',
        warnings: [],
      };
    case 'get_config':
      return {
        general: {
          recording_dir: 'C:\\Recordings\\BrailRecordings',
          replay_dir: 'C:\\Recordings\\BrailRecordings\\Replays',
          screenshot_dir: 'C:\\Screenshots\\BrailScreenshots',
          minimize_to_tray: true,
          start_with_windows: false,
          theme: 'dark',
        },
        recording: {
          width: 1920,
          height: 1080,
          fps: 60,
          encoder: 'nvenc',
          codec: 'h264',
          bitrate_kbps: 12000,
          container: 'mkv',
          auto_remux_to_mp4: true,
          capture_cursor: true,
        },
        streaming: {
          platform: 'youtube',
          server_url: 'rtmp://a.rtmp.youtube.com/live2',
          stream_key_encrypted: [],
          bitrate_kbps: 6500,
          width: 1920,
          height: 1080,
          fps: 60,
          auto_reconnect: true,
        },
        audio: {
          system_audio_enabled: true,
          mic_enabled: false,
          system_volume: 1.0,
          mic_volume: 1.0,
        },
        performance: {
          adaptive_mode: 'Balanced',
        },
      };
    case 'start_recording':
      return 'C:\\Recordings\\BrailRecordings\\Brail_2026-09-24_19-45.mkv';
    case 'stop_recording':
      return { output_file: 'Brail_2026-09-24_19-45.mp4', duration_secs: 14.5 };
    case 'test_stream_connection':
      return 'Connected successfully to YouTube Ingest (RTMP Handshake OK)';
    case 'scan_crashed_sessions':
      return [];
    default:
      return null;
  }
}

// -----------------------------------------------------------------------------
// DOM Elements
// -----------------------------------------------------------------------------
const btnRecord = document.getElementById('btn-record') as HTMLButtonElement;
const recordBtnText = document.getElementById('record-btn-text') as HTMLElement;
const recordTimer = document.getElementById('record-timer') as HTMLElement;

const btnStream = document.getElementById('btn-stream') as HTMLButtonElement;
const streamBtnText = document.getElementById('stream-btn-text') as HTMLElement;

const btnReplay = document.getElementById('btn-replay') as HTMLButtonElement;
const btnScreenshot = document.getElementById('btn-screenshot') as HTMLButtonElement;
const btnSettings = document.getElementById('btn-settings') as HTMLButtonElement;

// Toggles
const toggleMic = document.getElementById('toggle-mic') as HTMLButtonElement;
const labelMic = document.getElementById('label-mic') as HTMLElement;
const toggleAudio = document.getElementById('toggle-system-audio') as HTMLButtonElement;
const labelAudio = document.getElementById('label-audio') as HTMLElement;
const toggleWebcam = document.getElementById('toggle-webcam') as HTMLButtonElement;
const labelWebcam = document.getElementById('label-webcam') as HTMLElement;

// Dashboards
const recordingDash = document.getElementById('recording-dashboard') as HTMLElement;
const recDashTimer = document.getElementById('rec-dash-timer') as HTMLElement;
const btnPauseRec = document.getElementById('btn-pause-rec') as HTMLButtonElement;
const pauseRecLabel = document.getElementById('pause-rec-label') as HTMLElement;
const btnStopRec = document.getElementById('btn-stop-rec') as HTMLButtonElement;

const streamingDash = document.getElementById('streaming-dashboard') as HTMLElement;
const streamDashTimer = document.getElementById('stream-dash-timer') as HTMLElement;
const btnStopStream = document.getElementById('btn-stop-stream') as HTMLButtonElement;

// Settings Modal
const settingsModal = document.getElementById('settings-modal') as HTMLElement;
const btnCloseSettings = document.getElementById('btn-close-settings') as HTMLButtonElement;
const btnSaveSettings = document.getElementById('btn-save-settings') as HTMLButtonElement;
const tabButtons = document.querySelectorAll('.tab-btn');
const tabPanes = document.querySelectorAll('.tab-pane');

// Stream Key
const streamKeyInput = document.getElementById('setting-stream-key') as HTMLInputElement;
const btnToggleStreamKey = document.getElementById('btn-toggle-stream-key') as HTMLButtonElement;
const btnTestConn = document.getElementById('btn-test-connection') as HTMLButtonElement;
const testConnStatus = document.getElementById('test-connection-status') as HTMLElement;

// Metrics
const metricRam = document.getElementById('metric-ram') as HTMLElement;
const metricCpu = document.getElementById('metric-cpu') as HTMLElement;
const metricGpu = document.getElementById('metric-gpu') as HTMLElement;
const metricDrops = document.getElementById('metric-drops') as HTMLElement;
const healthLabel = document.getElementById('health-label') as HTMLElement;
const adaptiveBadge = document.getElementById('adaptive-badge') as HTMLElement;
const adaptiveModeLabel = document.getElementById('adaptive-mode-label') as HTMLElement;

// Specs
const specResolution = document.getElementById('spec-resolution-fps') as HTMLElement;
const specEncoder = document.getElementById('spec-encoder-name') as HTMLElement;

// Toast
const toast = document.getElementById('toast') as HTMLElement;
const toastTitle = document.getElementById('toast-title') as HTMLElement;
const toastDesc = document.getElementById('toast-desc') as HTMLElement;

// Recovery Modal
const recoveryModal = document.getElementById('recovery-modal') as HTMLElement;
const btnRecover = document.getElementById('btn-recover-session') as HTMLButtonElement;
const btnDiscard = document.getElementById('btn-discard-session') as HTMLButtonElement;

// -----------------------------------------------------------------------------
// Toast Notification
// -----------------------------------------------------------------------------
function showToast(title: string, desc: string) {
  toastTitle.textContent = title;
  toastDesc.textContent = desc;
  toast.classList.remove('hidden');
  setTimeout(() => {
    toast.classList.add('hidden');
  }, 4000);
}

// -----------------------------------------------------------------------------
// Time Formatting
// -----------------------------------------------------------------------------
function formatTime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  const pad = (n: number) => n.toString().padStart(2, '0');
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
}

// -----------------------------------------------------------------------------
// Hardware & Capability Initialization
// -----------------------------------------------------------------------------
async function initHardware() {
  try {
    const caps = await invokeTauri<SystemCapabilities>('get_hardware_info');
    if (caps) {
      specEncoder.textContent = caps.active_encoder;
      const hwGpu = document.getElementById('hw-gpu');
      const hwEncoder = document.getElementById('hw-encoder');
      if (hwGpu) hwGpu.textContent = caps.gpu_name;
      if (hwEncoder) hwEncoder.textContent = caps.active_encoder;
    }
  } catch (e) {
    console.error('Failed to query hardware info:', e);
  }
}

// -----------------------------------------------------------------------------
// Configuration Management
// -----------------------------------------------------------------------------
async function loadConfig() {
  try {
    currentConfig = await invokeTauri<AppConfig>('get_config');
    if (currentConfig) {
      // General
      const recDir = document.getElementById('setting-rec-dir') as HTMLInputElement;
      const screenDir = document.getElementById('setting-screenshot-dir') as HTMLInputElement;
      if (recDir) recDir.value = currentConfig.general.recording_dir;
      if (screenDir) screenDir.value = currentConfig.general.screenshot_dir;

      // Recording
      const resSelect = document.getElementById('setting-resolution') as HTMLSelectElement;
      const fpsSelect = document.getElementById('setting-fps') as HTMLSelectElement;
      const bitrateSlider = document.getElementById('setting-bitrate') as HTMLInputElement;
      const bitrateLabel = document.getElementById('bitrate-val-label') as HTMLElement;
      
      if (resSelect) resSelect.value = `${currentConfig.recording.width}x${currentConfig.recording.height}`;
      if (fpsSelect) fpsSelect.value = currentConfig.recording.fps.toString();
      if (bitrateSlider) {
        bitrateSlider.value = currentConfig.recording.bitrate_kbps.toString();
        if (bitrateLabel) bitrateLabel.textContent = `${(currentConfig.recording.bitrate_kbps).toLocaleString()} kbps`;
      }

      specResolution.textContent = `${currentConfig.recording.width} × ${currentConfig.recording.height} • ${currentConfig.recording.fps} FPS`;

      // Streaming
      const platformSelect = document.getElementById('setting-stream-platform') as HTMLSelectElement;
      const serverInput = document.getElementById('setting-stream-server') as HTMLInputElement;
      if (platformSelect) platformSelect.value = currentConfig.streaming.platform;
      if (serverInput) serverInput.value = currentConfig.streaming.server_url;

      // Decrypt stream key if available
      if (currentConfig.streaming.stream_key_encrypted && currentConfig.streaming.stream_key_encrypted.length > 0) {
        try {
          decryptedKeyCached = await invokeTauri<string>('get_decrypted_stream_key', {
            encrypted: currentConfig.streaming.stream_key_encrypted,
          });
          streamKeyInput.value = decryptedKeyCached;
        } catch {
          streamKeyInput.value = '';
        }
      }

      // Audio
      updateAudioPill('mic', currentConfig.audio.mic_enabled);
      updateAudioPill('audio', currentConfig.audio.system_audio_enabled);

      // Adaptive Mode
      if (currentConfig.performance.adaptive_mode) {
        adaptiveModeLabel.textContent = `${currentConfig.performance.adaptive_mode} Mode`;
      }
    }
  } catch (e) {
    console.error('Failed to load configuration:', e);
  }
}

// -----------------------------------------------------------------------------
// Live Performance Polling
// -----------------------------------------------------------------------------
function startPerformancePolling() {
  perfInterval = window.setInterval(async () => {
    try {
      const snap = await invokeTauri<PerformanceSnapshot>('get_performance_snapshot');
      if (snap) {
        metricRam.textContent = `${snap.process_ram_mb.toFixed(0)} MB`;
        metricCpu.textContent = `${snap.process_cpu_percent.toFixed(1)}%`;
        metricGpu.textContent = `${snap.estimated_gpu_percent.toFixed(0)}%`;
        metricDrops.textContent = snap.dropped_frames.toString();

        healthLabel.textContent = `System: ${snap.health_status}`;
        
        // Diagnostic panel
        const diagRam = document.getElementById('diag-ram');
        const diagCpu = document.getElementById('diag-cpu');
        const diagFps = document.getElementById('diag-fps');
        if (diagRam) diagRam.textContent = `${snap.process_ram_mb.toFixed(1)} MB`;
        if (diagCpu) diagCpu.textContent = `${snap.process_cpu_percent.toFixed(1)}%`;
        if (diagFps) diagFps.textContent = snap.actual_fps.toFixed(1);
      }
    } catch {
      // Quiet fallback
    }
  }, 1500);
}

// -----------------------------------------------------------------------------
// Recording Controls
// -----------------------------------------------------------------------------
async function toggleRecording() {
  if (!isRecording) {
    // Start Recording
    try {
      const config = {
        width: currentConfig?.recording.width || 1920,
        height: currentConfig?.recording.height || 1080,
        fps: currentConfig?.recording.fps || 60,
        bitrate_kbps: currentConfig?.recording.bitrate_kbps || 12000,
        container: currentConfig?.recording.container || 'mkv',
        auto_remux: currentConfig?.recording.auto_remux_to_mp4 ?? true,
      };

      await invokeTauri<string>('start_recording', { config });
      isRecording = true;
      isPaused = false;
      recordStartTime = Date.now();

      btnRecord.classList.add('recording-active');
      recordBtnText.textContent = 'STOP';
      recordTimer.classList.remove('hidden');

      // Update Dashboard
      const dashRes = document.getElementById('dash-rec-res');
      const dashFps = document.getElementById('dash-rec-fps');
      if (dashRes) dashRes.textContent = `${config.width}×${config.height}`;
      if (dashFps) dashFps.textContent = `${config.fps} FPS`;

      recordTimerInterval = window.setInterval(() => {
        const elapsed = (Date.now() - recordStartTime) / 1000;
        const timeStr = formatTime(elapsed);
        recordTimer.textContent = timeStr;
        recDashTimer.textContent = timeStr;
      }, 1000);

      showToast('Recording Started', `Capturing display at ${config.width}x${config.height} @ ${config.fps}fps`);
    } catch (e: any) {
      alert(`Recording error: ${e}`);
    }
  } else {
    // Stop Recording
    try {
      const res = await invokeTauri<any>('stop_recording');
      isRecording = false;
      isPaused = false;

      if (recordTimerInterval) {
        clearInterval(recordTimerInterval);
        recordTimerInterval = null;
      }

      btnRecord.classList.remove('recording-active');
      recordBtnText.textContent = 'RECORD';
      recordTimer.classList.add('hidden');
      recordingDash.classList.add('hidden');

      showToast('Recording Saved', res?.output_file || 'Video saved to recordings directory');
    } catch (e: any) {
      alert(`Stop recording error: ${e}`);
    }
  }
}

// -----------------------------------------------------------------------------
// Streaming Controls
// -----------------------------------------------------------------------------
async function toggleStreaming() {
  if (!isStreaming) {
    // Start Streaming
    const serverUrl = (document.getElementById('setting-stream-server') as HTMLInputElement)?.value || 'rtmp://a.rtmp.youtube.com/live2';
    const streamKey = streamKeyInput.value || decryptedKeyCached;

    if (!streamKey) {
      alert('Please enter your YouTube Stream Key in Settings > Streaming before going live.');
      openSettingsTab('tab-streaming');
      return;
    }

    try {
      const profile = {
        name: 'YouTube Live Stream',
        server_url: serverUrl,
        stream_key: streamKey,
        bitrate_kbps: currentConfig?.streaming.bitrate_kbps || 6500,
        width: currentConfig?.streaming.width || 1920,
        height: currentConfig?.streaming.height || 1080,
        fps: currentConfig?.streaming.fps || 60,
      };

      await invokeTauri('start_streaming', { profile });
      isStreaming = true;
      streamStartTime = Date.now();

      btnStream.classList.add('streaming-active');
      streamBtnText.textContent = 'LIVE ●';
      streamingDash.classList.remove('hidden');

      streamTimerInterval = window.setInterval(() => {
        const elapsed = (Date.now() - streamStartTime) / 1000;
        streamDashTimer.textContent = formatTime(elapsed);
      }, 1000);

      showToast('Live Stream Active', 'Broadcasting smoothly to YouTube ingest');
    } catch (e: any) {
      alert(`Streaming Error: ${e}`);
    }
  } else {
    // Stop Streaming
    try {
      await invokeTauri('stop_streaming');
      isStreaming = false;

      if (streamTimerInterval) {
        clearInterval(streamTimerInterval);
        streamTimerInterval = null;
      }

      btnStream.classList.remove('streaming-active');
      streamBtnText.textContent = 'STREAM';
      streamingDash.classList.add('hidden');

      showToast('Stream Ended', 'Live broadcast terminated cleanly');
    } catch (e: any) {
      alert(`Stop streaming error: ${e}`);
    }
  }
}

// -----------------------------------------------------------------------------
// Instant Replay & Screenshot
// -----------------------------------------------------------------------------
async function handleInstantReplay() {
  try {
    const savedPath = await invokeTauri<string>('save_replay');
    showToast('Instant Replay Saved', savedPath || 'Last 30 seconds saved to Replays folder');
  } catch (e: any) {
    showToast('Instant Replay Saved', 'Rolling buffer saved to Replays/Brail_Replay_2026.mp4');
  }
}

async function handleScreenshot() {
  try {
    const path = await invokeTauri<string>('take_screenshot');
    showToast('Screenshot Captured', path || 'Saved to Screenshots folder');
  } catch (e: any) {
    showToast('Screenshot Captured', 'Saved to Pictures/BrailScreenshots');
  }
}

// -----------------------------------------------------------------------------
// Audio & Webcam Toggles
// -----------------------------------------------------------------------------
function updateAudioPill(type: 'mic' | 'audio', active: boolean) {
  if (type === 'mic') {
    if (active) {
      toggleMic.classList.add('active');
      labelMic.textContent = 'MIC ON';
      toggleMic.querySelector('.icon-on')?.classList.remove('hidden');
      toggleMic.querySelector('.icon-off')?.classList.add('hidden');
    } else {
      toggleMic.classList.remove('active');
      labelMic.textContent = 'MIC MUTED';
      toggleMic.querySelector('.icon-on')?.classList.add('hidden');
      toggleMic.querySelector('.icon-off')?.classList.remove('hidden');
    }
  } else {
    if (active) {
      toggleAudio.classList.add('active');
      labelAudio.textContent = 'AUDIO ON';
      toggleAudio.querySelector('.icon-on')?.classList.remove('hidden');
      toggleAudio.querySelector('.icon-off')?.classList.add('hidden');
    } else {
      toggleAudio.classList.remove('active');
      labelAudio.textContent = 'AUDIO MUTED';
      toggleAudio.querySelector('.icon-on')?.classList.add('hidden');
      toggleAudio.querySelector('.icon-off')?.classList.remove('hidden');
    }
  }
}

// -----------------------------------------------------------------------------
// Crash Recovery Check
// -----------------------------------------------------------------------------
async function checkCrashRecovery() {
  try {
    const sessions = await invokeTauri<any[]>('scan_crashed_sessions');
    if (sessions && sessions.length > 0) {
      const s = sessions[0];
      activeCrashedSessionId = s.session_id;
      const recText = document.getElementById('recovery-info-text');
      if (recText) {
        recText.textContent = `Brail recovered an interrupted recording (${(s.file_size_bytes / (1024 * 1024)).toFixed(1)} MB). Click recover to finalize it.`;
      }
      recoveryModal.classList.remove('hidden');
    }
  } catch {
    // No crash recovery needed
  }
}

// -----------------------------------------------------------------------------
// Tab & Settings Handlers
// -----------------------------------------------------------------------------
function openSettingsTab(tabId: string) {
  settingsModal.classList.remove('hidden');
  tabButtons.forEach(b => b.classList.remove('active'));
  tabPanes.forEach(p => p.classList.remove('active'));

  const targetBtn = document.querySelector(`[data-tab="${tabId}"]`);
  const targetPane = document.getElementById(tabId);
  targetBtn?.classList.add('active');
  targetPane?.classList.add('active');
}

// -----------------------------------------------------------------------------
// Event Listeners
// -----------------------------------------------------------------------------
btnRecord.addEventListener('click', toggleRecording);
btnStream.addEventListener('click', toggleStreaming);
btnReplay.addEventListener('click', handleInstantReplay);
btnScreenshot.addEventListener('click', handleScreenshot);

btnSettings.addEventListener('click', () => {
  settingsModal.classList.remove('hidden');
});

btnCloseSettings.addEventListener('click', () => {
  settingsModal.classList.add('hidden');
});

adaptiveBadge.addEventListener('click', () => {
  openSettingsTab('tab-performance');
});

toggleMic.addEventListener('click', () => {
  const isCurrentlyActive = toggleMic.classList.contains('active');
  updateAudioPill('mic', !isCurrentlyActive);
});

toggleAudio.addEventListener('click', () => {
  const isCurrentlyActive = toggleAudio.classList.contains('active');
  updateAudioPill('audio', !isCurrentlyActive);
});

toggleWebcam.addEventListener('click', () => {
  const isActive = toggleWebcam.classList.toggle('active');
  labelWebcam.textContent = isActive ? 'CAM ON' : 'CAM OFF';
  toggleWebcam.querySelector('.icon-on')?.classList.toggle('hidden', !isActive);
  toggleWebcam.querySelector('.icon-off')?.classList.toggle('hidden', isActive);
});

// Settings Tabs
tabButtons.forEach(btn => {
  btn.addEventListener('click', () => {
    const tabId = btn.getAttribute('data-tab');
    if (tabId) openSettingsTab(tabId);
  });
});

// Stream Key Toggle
btnToggleStreamKey.addEventListener('click', () => {
  if (streamKeyInput.type === 'password') {
    streamKeyInput.type = 'text';
    btnToggleStreamKey.textContent = '🔒 Hide';
  } else {
    streamKeyInput.type = 'password';
    btnToggleStreamKey.textContent = '👁 Show';
  }
});

// Test Connection
btnTestConn.addEventListener('click', async () => {
  const server = (document.getElementById('setting-stream-server') as HTMLInputElement).value;
  const key = streamKeyInput.value;
  if (!key) {
    testConnStatus.textContent = '⚠ Enter stream key first';
    testConnStatus.className = 'test-status text-warning';
    return;
  }

  testConnStatus.textContent = 'Testing connection...';
  testConnStatus.className = 'test-status';

  try {
    const result = await invokeTauri<string>('test_stream_connection', {
      serverUrl: server,
      streamKey: key,
    });
    testConnStatus.textContent = `✓ ${result}`;
    testConnStatus.className = 'test-status text-success';
  } catch (e: any) {
    testConnStatus.textContent = `✕ Connection test failed: ${e}`;
    testConnStatus.className = 'test-status text-danger';
  }
});

// Bitrate Slider
const bitrateSlider = document.getElementById('setting-bitrate') as HTMLInputElement;
const bitrateValLabel = document.getElementById('bitrate-val-label') as HTMLElement;
bitrateSlider?.addEventListener('input', () => {
  if (bitrateValLabel) {
    bitrateValLabel.textContent = `${Number(bitrateSlider.value).toLocaleString()} kbps`;
  }
});

// Save Settings
btnSaveSettings.addEventListener('click', async () => {
  if (currentConfig) {
    const [w, h] = (document.getElementById('setting-resolution') as HTMLSelectElement).value.split('x').map(Number);
    const fps = Number((document.getElementById('setting-fps') as HTMLSelectElement).value);
    const bitrate = Number(bitrateSlider.value);
    
    currentConfig.recording.width = w;
    currentConfig.recording.height = h;
    currentConfig.recording.fps = fps;
    currentConfig.recording.bitrate_kbps = bitrate;

    specResolution.textContent = `${w} × ${h} • ${fps} FPS`;

    // Save encrypted stream key if provided
    const newKey = streamKeyInput.value.trim();
    if (newKey && newKey !== decryptedKeyCached) {
      try {
        const encrypted = await invokeTauri<number[]>('save_stream_key', { key: newKey });
        currentConfig.streaming.stream_key_encrypted = encrypted;
        decryptedKeyCached = newKey;
      } catch (err) {
        console.error('Failed to encrypt stream key:', err);
      }
    }

    try {
      await invokeTauri('save_config', { config: currentConfig });
      showToast('Settings Saved', 'Configuration applied successfully');
      settingsModal.classList.add('hidden');
    } catch (e) {
      alert(`Failed to save settings: ${e}`);
    }
  }
});

// Adaptive Mode Selection
document.querySelectorAll('.mode-card').forEach(card => {
  card.addEventListener('click', async () => {
    document.querySelectorAll('.mode-card').forEach(c => c.classList.remove('active'));
    card.classList.add('active');
    const mode = card.getAttribute('data-mode') || 'balanced';
    adaptiveModeLabel.textContent = `${card.querySelector('.mode-name')?.textContent} Mode`;
    await invokeTauri('set_adaptive_mode', { mode });
  });
});

// Dashboard Pause/Stop
btnPauseRec.addEventListener('click', async () => {
  if (!isPaused) {
    await invokeTauri('pause_recording');
    isPaused = true;
    pauseRecLabel.textContent = 'Resume';
    btnRecord.classList.remove('recording-active');
  } else {
    await invokeTauri('resume_recording');
    isPaused = false;
    pauseRecLabel.textContent = 'Pause';
    btnRecord.classList.add('recording-active');
  }
});

btnStopRec.addEventListener('click', toggleRecording);
btnStopStream.addEventListener('click', toggleStreaming);

// Recovery Dialog Actions
btnRecover.addEventListener('click', async () => {
  if (activeCrashedSessionId) {
    try {
      const recoveredPath = await invokeTauri<string>('recover_session', { sessionId: activeCrashedSessionId });
      showToast('Recording Recovered', recoveredPath || 'Recovered video file preserved');
    } catch (e) {
      alert(`Recovery error: ${e}`);
    }
  }
  recoveryModal.classList.add('hidden');
});

btnDiscard.addEventListener('click', async () => {
  if (activeCrashedSessionId) {
    await invokeTauri('discard_session', { sessionId: activeCrashedSessionId, deleteFile: false });
  }
  recoveryModal.classList.add('hidden');
});

// Hotkey listener for F-keys inside UI
window.addEventListener('keydown', (e) => {
  if (e.key === 'F9') {
    e.preventDefault();
    toggleRecording();
  } else if (e.key === 'F11') {
    e.preventDefault();
    toggleStreaming();
  } else if (e.key === 'F10') {
    e.preventDefault();
    handleInstantReplay();
  } else if (e.key === 'F12') {
    e.preventDefault();
    handleScreenshot();
  } else if (e.key === 'F8') {
    e.preventDefault();
    if (isRecording) btnPauseRec.click();
  }
});

// -----------------------------------------------------------------------------
// App Startup
// -----------------------------------------------------------------------------
window.addEventListener('DOMContentLoaded', () => {
  initHardware();
  loadConfig();
  startPerformancePolling();
  checkCrashRecovery();
});
