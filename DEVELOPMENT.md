# Brail Recorder — Developer & Contributor Guide

## 1. Prerequisites

- **Rust**: Version 1.80+ (`x86_64-pc-windows-msvc`).
- **Node.js**: Version 18+ and `npm`.
- **Tauri CLI**: Version 2.0+.
- **Linker / SDK**: Windows 10/11 SDK or LLVM `clang-cl` / `lld-link` (supported out-of-the-box via `cargo-xwin`).

---

## 2. Repository Layout

```text
brail-recorder/
├── src/                  # Vanilla TypeScript & CSS Frontend
│   ├── assets/           # Icons and logos
│   ├── main.ts           # IPC controller, state, dashboards
│   └── styles.css        # Dark-mode design system & micro-animations
├── src-tauri/            # Rust native backend
│   ├── src/
│   │   ├── audio/        # WASAPI system audio loopback & mic capture
│   │   ├── capture/      # Windows Graphics Capture (WGC) + DXGI fallback
│   │   ├── encoder/      # NVENC, AMF, QSV, and CPU H.264/AV1 pipelines
│   │   ├── hardware/     # DXGI GPU detection & capability profiler
│   │   ├── hotkeys/      # Win32 RegisterHotKey manager
│   │   ├── performance/  # Brail Adaptive Engine & low-overhead counters
│   │   ├── recording/    # Recording pipeline, muxer, crash markers
│   │   ├── recovery/     # Unfinished session recovery & remuxing
│   │   ├── replay/       # Rolling circular buffer for Instant Replay
│   │   ├── security/     # DPAPI encryption & log secret redactor
│   │   ├── storage/      # Config persistence & collision-free naming
│   │   ├── lib.rs        # Tauri IPC command definitions & registration
│   │   └── main.rs       # Entrypoint
│   ├── Cargo.toml        # Rust dependencies & Windows API features
│   └── tauri.conf.json   # Tauri window, security, and bundle configuration
├── index.html            # Main UI document
└── package.json          # Node dependencies
```

---

## 3. Running Locally in Development Mode

```bash
# In brail-recorder directory:
npm install

# Run frontend with Vite HMR + Tauri desktop shell:
npm run tauri dev
```

---

## 4. Building Production Installer

```bash
# Compiles optimized release binary and builds NSIS installer:
npm run tauri build
```
Output binaries are generated in `src-tauri/target/release/bundle/nsis/`.
