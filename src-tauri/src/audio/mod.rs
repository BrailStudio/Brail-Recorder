// Brail Recorder — Audio Engine
// WASAPI-based system audio and microphone capture

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use crossbeam_channel::{Sender, Receiver, bounded};
use tracing::{info, warn, error};

use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::Foundation::*;
use windows::core::*;

/// Audio configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub capture_system_audio: bool,
    pub capture_microphone: bool,
    pub system_volume: f32,       // 0.0 to 1.0
    pub mic_volume: f32,          // 0.0 to 1.0
    pub system_muted: bool,
    pub mic_muted: bool,
    pub sample_rate: u32,         // 44100 or 48000
    pub channels: u16,            // 1 or 2
    pub bits_per_sample: u16,     // 16 or 32
    pub system_device_id: String,
    pub mic_device_id: String,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            capture_system_audio: true,
            capture_microphone: false,
            system_volume: 1.0,
            mic_volume: 1.0,
            system_muted: false,
            mic_muted: false,
            sample_rate: 48000,
            channels: 2,
            bits_per_sample: 16,
            system_device_id: "default".to_string(),
            mic_device_id: "default".to_string(),
        }
    }
}

/// An audio packet with PCM data
#[derive(Clone)]
pub struct AudioPacket {
    pub data: Vec<u8>,
    pub samples: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub timestamp_ns: u64,
    pub is_microphone: bool,
}

/// Audio capture engine managing system audio and microphone
pub struct AudioEngine {
    config: AudioConfig,
    running: Arc<AtomicBool>,
    audio_sender: Sender<AudioPacket>,
    audio_receiver: Receiver<AudioPacket>,
    system_thread: Option<std::thread::JoinHandle<()>>,
    mic_thread: Option<std::thread::JoinHandle<()>>,
    system_peak: Arc<parking_lot::Mutex<f32>>,
    mic_peak: Arc<parking_lot::Mutex<f32>>,
}

impl AudioEngine {
    pub fn new(config: AudioConfig) -> Self {
        let (audio_sender, audio_receiver) = bounded(32);

        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            audio_sender,
            audio_receiver,
            system_thread: None,
            mic_thread: None,
            system_peak: Arc::new(parking_lot::Mutex::new(0.0)),
            mic_peak: Arc::new(parking_lot::Mutex::new(0.0)),
        }
    }

    /// Get the audio packet receiver channel
    pub fn audio_receiver(&self) -> Receiver<AudioPacket> {
        self.audio_receiver.clone()
    }

    /// Get current audio levels for meters
    pub fn get_levels(&self) -> (f32, f32) {
        (*self.system_peak.lock(), *self.mic_peak.lock())
    }

    /// Start audio capture
    pub fn start(&mut self) -> anyhow::Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.running.store(true, Ordering::SeqCst);

        // Start system audio capture (loopback)
        if self.config.capture_system_audio {
            let running = self.running.clone();
            let sender = self.audio_sender.clone();
            let config = self.config.clone();
            let peak = self.system_peak.clone();

            let handle = std::thread::spawn(move || {
                if let Err(e) = capture_system_audio(&running, &sender, &config, &peak) {
                    error!("System audio capture error: {}", e);
                }
            });
            self.system_thread = Some(handle);
            info!("System audio capture started");
        }

        // Start microphone capture
        if self.config.capture_microphone {
            let running = self.running.clone();
            let sender = self.audio_sender.clone();
            let config = self.config.clone();
            let peak = self.mic_peak.clone();

            let handle = std::thread::spawn(move || {
                if let Err(e) = capture_microphone(&running, &sender, &config, &peak) {
                    error!("Microphone capture error: {}", e);
                }
            });
            self.mic_thread = Some(handle);
            info!("Microphone capture started");
        }

        Ok(())
    }

    /// Stop audio capture
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.system_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.mic_thread.take() {
            let _ = handle.join();
        }
        info!("Audio engine stopped");
    }

    /// Update volume
    pub fn set_system_volume(&mut self, volume: f32) {
        self.config.system_volume = volume.clamp(0.0, 1.0);
    }

    pub fn set_mic_volume(&mut self, volume: f32) {
        self.config.mic_volume = volume.clamp(0.0, 1.0);
    }

    pub fn set_system_muted(&mut self, muted: bool) {
        self.config.system_muted = muted;
    }

    pub fn set_mic_muted(&mut self, muted: bool) {
        self.config.mic_muted = muted;
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Capture system audio via WASAPI loopback
fn capture_system_audio(
    running: &AtomicBool,
    sender: &Sender<AudioPacket>,
    config: &AudioConfig,
    peak: &parking_lot::Mutex<f32>,
) -> anyhow::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)?;

        let enumerator: IMMDeviceEnumerator = CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        )?;

        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

        // Get mix format
        let mix_format = client.GetMixFormat()?;
        let format = &*mix_format;

        info!(
            "System audio format: {} Hz, {} ch, {} bits",
            format.nSamplesPerSec, format.nChannels, format.wBitsPerSample
        );

        // Initialize in loopback mode (captures what you hear)
        let buffer_duration = 200_000; // 20ms in 100ns units
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            buffer_duration,
            0,
            mix_format,
            None,
        )?;

        // Create event for frame notification
        let event = windows::Win32::System::Threading::CreateEventW(
            None,
            false,
            false,
            None,
        )?;
        client.SetEventHandle(event)?;

        let capture_client: IAudioCaptureClient = client.GetService()?;

        client.Start()?;
        info!("WASAPI loopback capture started");

        let start_time = std::time::Instant::now();

        while running.load(Ordering::SeqCst) {
            // Wait for audio data
            let wait_result = windows::Win32::System::Threading::WaitForSingleObject(event, 100);
            if wait_result == WAIT_OBJECT_0 {
                loop {
                    let mut buffer: *mut u8 = std::ptr::null_mut();
                    let mut frames_available = 0u32;
                    let mut flags = 0u32;

                    let hr = capture_client.GetBuffer(
                        &mut buffer,
                        &mut frames_available,
                        &mut flags,
                        None,
                        None,
                    );

                    if hr.is_err() || frames_available == 0 {
                        break;
                    }

                    let frame_size = format.nBlockAlign as usize;
                    let data_size = frames_available as usize * frame_size;

                    if !config.system_muted && !buffer.is_null() {
                        let mut audio_data = vec![0u8; data_size];

                        if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                            // Silent buffer — already zeroed
                        } else {
                            std::ptr::copy_nonoverlapping(buffer, audio_data.as_mut_ptr(), data_size);

                            // Apply volume
                            if (config.system_volume - 1.0).abs() > 0.01 {
                                apply_volume(&mut audio_data, config.system_volume, format.wBitsPerSample);
                            }

                            // Calculate peak level
                            let level = calculate_peak(&audio_data, format.wBitsPerSample);
                            *peak.lock() = level;
                        }

                        let packet = AudioPacket {
                            data: audio_data,
                            samples: frames_available,
                            sample_rate: format.nSamplesPerSec,
                            channels: format.nChannels,
                            bits_per_sample: format.wBitsPerSample,
                            timestamp_ns: start_time.elapsed().as_nanos() as u64,
                            is_microphone: false,
                        };

                        let _ = sender.try_send(packet);
                    }

                    let _ = capture_client.ReleaseBuffer(frames_available);
                }
            }
        }

        client.Stop()?;
        let _ = CloseHandle(event);
        info!("WASAPI loopback capture stopped");

        Ok(())
    }
}

/// Capture microphone via WASAPI
fn capture_microphone(
    running: &AtomicBool,
    sender: &Sender<AudioPacket>,
    config: &AudioConfig,
    peak: &parking_lot::Mutex<f32>,
) -> anyhow::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)?;

        let enumerator: IMMDeviceEnumerator = CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        )?;

        let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

        let mix_format = client.GetMixFormat()?;
        let format = &*mix_format;

        info!(
            "Microphone format: {} Hz, {} ch, {} bits",
            format.nSamplesPerSec, format.nChannels, format.wBitsPerSample
        );

        let buffer_duration = 200_000; // 20ms
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            buffer_duration,
            0,
            mix_format,
            None,
        )?;

        let event = windows::Win32::System::Threading::CreateEventW(
            None,
            false,
            false,
            None,
        )?;
        client.SetEventHandle(event)?;

        let capture_client: IAudioCaptureClient = client.GetService()?;

        client.Start()?;
        info!("WASAPI microphone capture started");

        let start_time = std::time::Instant::now();

        while running.load(Ordering::SeqCst) {
            let wait_result = windows::Win32::System::Threading::WaitForSingleObject(event, 100);
            if wait_result == WAIT_OBJECT_0 {
                loop {
                    let mut buffer: *mut u8 = std::ptr::null_mut();
                    let mut frames_available = 0u32;
                    let mut flags = 0u32;

                    let hr = capture_client.GetBuffer(
                        &mut buffer,
                        &mut frames_available,
                        &mut flags,
                        None,
                        None,
                    );

                    if hr.is_err() || frames_available == 0 {
                        break;
                    }

                    let frame_size = format.nBlockAlign as usize;
                    let data_size = frames_available as usize * frame_size;

                    if !config.mic_muted && !buffer.is_null() {
                        let mut audio_data = vec![0u8; data_size];
                        std::ptr::copy_nonoverlapping(buffer, audio_data.as_mut_ptr(), data_size);

                        // Apply volume
                        if (config.mic_volume - 1.0).abs() > 0.01 {
                            apply_volume(&mut audio_data, config.mic_volume, format.wBitsPerSample);
                        }

                        let level = calculate_peak(&audio_data, format.wBitsPerSample);
                        *peak.lock() = level;

                        let packet = AudioPacket {
                            data: audio_data,
                            samples: frames_available,
                            sample_rate: format.nSamplesPerSec,
                            channels: format.nChannels,
                            bits_per_sample: format.wBitsPerSample,
                            timestamp_ns: start_time.elapsed().as_nanos() as u64,
                            is_microphone: true,
                        };

                        let _ = sender.try_send(packet);
                    }

                    let _ = capture_client.ReleaseBuffer(frames_available);
                }
            }
        }

        client.Stop()?;
        let _ = CloseHandle(event);
        info!("WASAPI microphone capture stopped");

        Ok(())
    }
}

/// Apply volume scaling to audio data
fn apply_volume(data: &mut [u8], volume: f32, bits_per_sample: u16) {
    match bits_per_sample {
        16 => {
            let samples: &mut [i16] = bytemuck::cast_slice_mut(data);
            for sample in samples.iter_mut() {
                *sample = ((*sample as f32) * volume) as i16;
            }
        }
        32 => {
            let samples: &mut [f32] = bytemuck::cast_slice_mut(data);
            for sample in samples.iter_mut() {
                *sample *= volume;
            }
        }
        _ => {}
    }
}

/// Calculate peak audio level (0.0 to 1.0)
fn calculate_peak(data: &[u8], bits_per_sample: u16) -> f32 {
    match bits_per_sample {
        16 => {
            if data.len() < 2 {
                return 0.0;
            }
            let samples: &[i16] = bytemuck::cast_slice(data);
            let max = samples.iter().map(|s| s.unsigned_abs() as f32).fold(0.0f32, f32::max);
            (max / i16::MAX as f32).min(1.0)
        }
        32 => {
            if data.len() < 4 {
                return 0.0;
            }
            let samples: &[f32] = bytemuck::cast_slice(data);
            samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max).min(1.0)
        }
        _ => 0.0,
    }
}
