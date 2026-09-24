# Brail Recorder — Architecture Specification

## 1. High-Level Architecture Overview

Brail Recorder is designed with a strict decoupled architecture where the user interface, capture subsystem, video encoding pipeline, audio multiplexer, and network stream dispatcher operate as isolated, asynchronous pipelines.

```
┌─────────────────────────────────────────────────────────────┐
│                 TAURI DESKTOP USER INTERFACE                │
│    (HTML5 / Vanilla TypeScript / CSS Glassmorphism)         │
└──────────────────────────────┬──────────────────────────────┘
                               │ IPC Command Bridge
┌──────────────────────────────▼──────────────────────────────┐
│                    BRAIL CORE RUST ENGINE                   │
├───────────────┬──────────────────────────────┬──────────────┤
│  Hardware &   │    Brail Adaptive Engine     │  Storage &   │
│  Detectors    │ (Lightweight Win32 Counters) │  DPAPI Vault │
└───────┬───────┴──────────────┬───────────────┴──────┬───────┘
        │                      │                      │
┌───────▼──────────────┐ ┌─────▼───────────────┐ ┌────▼───────┐
│ CAPTURE SUBSYSTEM    │ │ AUDIO ENGINE        │ │ INSTANT    │
│ • Windows Graphics   │ │ • WASAPI Loopback   │ │ REPLAY     │
│   Capture (WGC)      │ │ • WASAPI Mic Capture│ │ • Rolling  │
│ • DXGI Desktop Dup   │ │ • Bounded Ring Buf  │ │   Buffer   │
└───────┬──────────────┘ └─────┬───────────────┘ └────┬───────┘
        │                      │                      │
        │ Bounded Channel      │                      │
┌───────▼──────────────────────▼──────────────────────▼───────┐
│               HARDWARE ACCELERATED ENCODER                  │
│       • NVIDIA NVENC  • AMD AMF  • Intel QSV  • CPU         │
└──────────────────────────────┬──────────────────────────────┘
                               │ Zero-Copy Packets
        ┌──────────────────────┴──────────────────────┐
        │                                             │
┌───────▼──────────────────────────┐   ┌──────────────▼──────────────┐
│       RECORDING PIPELINE         │   │      STREAMING ENGINE       │
│ • MKV/MP4 Container Muxer        │   │ • YouTube RTMP/RTMPS Ingest │
│ • Session Marker / Crash Guard   │   │ • Adaptive Auto-Reconnect   │
│ • Zero-Reencode MP4 Remuxing     │   │ • Bandwidth & Jitter Guard  │
└──────────────────────────────────┘   └─────────────────────────────┘
```

---

## 2. Component Breakdown

### 2.1 Hardware Detection (`hardware/mod.rs`)
- Direct DXGI adapter enumeration (`IDXGIFactory1::EnumAdapters1`).
- Queries vendor ID (0x10DE for NVIDIA, 0x1002 for AMD, 0x8086 for Intel).
- Enforces hardware encoder auto-selection hierarchy: `NVENC > AMF > QSV > Software CPU`.
- Enumerates multi-monitors with native DPI scaling factors and refresh rates via Win32 GDI.

### 2.2 Screen Capture Engine (`capture/mod.rs`)
- **Primary Path**: Windows Graphics Capture (WGC) API (`GraphicsCaptureItem`). Zero-copy GPU surface access via Direct3D 11 swapchain textures.
- **Fallback Path**: DXGI Desktop Duplication API (`IDXGIOutputDuplication::AcquireNextFrame`) for legacy or exclusive full-screen applications.
- **Backpressure & Bounded Buffers**: Bounded crossbeam channels prevent frame queuing latency when the system is heavily loaded.

### 2.3 Audio Engine (`audio/mod.rs`)
- Windows Audio Session API (WASAPI) in low-latency event-driven mode.
- Desktop audio captured via `AUDCLNT_STREAMFLAGS_LOOPBACK`.
- Independent microphone channel with per-stream software volume gain and instant mute without restarting the audio device.
- Mixed to standard 48,000 Hz, 16-bit stereo PCM for video synchronization.

### 2.4 Encoding Subsystem (`encoder/mod.rs`)
- Hardware encoders initialize with low-latency tuning:
  - NVENC: Preset `P1` (Ultra-fast) / `P4` (Balanced), rate control CBR/CQP.
  - AMF: Low latency profile with B-frame disabling for streaming.
  - QSV: Target usage `TU7` (Fastest) / `TU4` (Balanced).
- Software Fallback: Multi-threaded H.264 CPU encoder tuned with `ultrafast` preset to keep CPU consumption <10%.

### 2.5 Streaming Engine (`streaming/mod.rs`)
- Real RTMP/RTMPS streaming directly to YouTube ingest (`rtmp://a.rtmp.youtube.com/live2`).
- Asynchronous connection verification (`test_stream_connection`) performs socket handshake before initiating live broadcasting.
- Auto-reconnect with exponential backoff and connection health rating (Optimal, Moderate, Strained).

### 2.6 Instant Replay (`replay/mod.rs`)
- Rolling ring-buffer storing the last N seconds (15s to 300s) of captured frames in memory.
- Bounded memory footprint (approx. 20–45 MB for keyframes in RingBuffer).
- Saves directly to disk without interfering with active recording sessions.

### 2.7 Performance & Adaptive Engine (`performance/mod.rs`)
- Win32 native queries: `GetProcessMemoryInfo`, `GetSystemTimes`, `GetProcessTimes`, `GlobalMemoryStatusEx`.
- Sampling interval throttled to 1.5 seconds to guarantee the monitor itself consumes <0.1% CPU.
