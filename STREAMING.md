# Brail Recorder — Streaming Engine & YouTube Ingest Guide

## 1. Overview

Brail Recorder implements a real, low-latency RTMP/RTMPS client pipeline designed to transmit compressed video and audio directly to YouTube Live, Twitch, and custom broadcast ingest servers without intermediary processing.

---

## 2. Ingest Architecture

```
GPU Direct3D 11 Surface
          │
          ▼
Hardware Encoder (NVENC / AMF / QSV)
          │
          ▼ [H.264 / AV1 NAL Units]
Packet Transmit Queue (Bounded Ring)
          │
          ▼
RTMP Protocol Handshake & Chunk Streamer
          │
          ▼ [TLS / TCP Socket Direct Connect]
YouTube Ingest: rtmp://a.rtmp.youtube.com/live2/{stream_key}
```

---

## 3. YouTube Optimized Defaults

| Preset | Resolution | FPS | Target Bitrate | Keyframe Interval | Audio |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Ultra 4K** | 3840 × 2160 | 60 | 28,000 kbps | 2.0s (120 frames) | AAC-LC 320 kbps |
| **High** | 2560 × 1440 | 60 | 13,000 kbps | 2.0s (120 frames) | AAC-LC 256 kbps |
| **Full HD (Recommended)** | 1920 × 1080 | 60 | 6,500 kbps | 2.0s (120 frames) | AAC-LC 192 kbps |
| **Balanced** | 1280 × 720 | 60 | 4,500 kbps | 2.0s (120 frames) | AAC-LC 160 kbps |
| **Low Internet** | 854 × 480 | 30 | 1,500 kbps | 2.0s (60 frames) | AAC-LC 128 kbps |

---

## 4. Connection Testing & Diagnostics

Before broadcasting live, Brail Recorder provides a **[TEST CONNECTION]** button in the Streaming settings tab:
1. Opens a direct non-blocking TCP socket to the ingest server host on port 1935 (RTMP) or 443 (RTMPS).
2. Performs the RTMP C0/C1 handshake verification.
3. Tests round-trip latency and confirms endpoint availability without polluting your live channel feed.

---

## 5. Network Resiliency & Auto-Reconnect

- **Adaptive Backoff**: When network congestion or transient dropouts occur, Brail Recorder attempts automatic reconnection (configurable up to 5 retries with exponential backoff: 2s, 4s, 8s, 16s).
- **Zero-Copy Re-queuing**: Frame buffers remain bounded during disconnections to prevent memory bloating. If a reconnection succeeds, a fresh IDR keyframe is immediately generated to synchronize the stream.
- **Dropped Frame Counter**: Tracks packet transmission latency and displays real-time frame drop percentages on the live dashboard.
