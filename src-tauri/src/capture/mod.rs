// Brail Recorder — Screen Capture Engine
// Windows Graphics Capture API + Desktop Duplication fallback

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use tracing::{info, warn, error};
use crossbeam_channel::{Sender, Receiver, bounded};

use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Graphics::Capture::*;
use windows::Graphics::DirectX::*;
use windows::Graphics::DirectX::Direct3D11::*;
use windows::core::*;

/// Type of capture source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaptureSource {
    Monitor { hmonitor: isize, name: String },
    Window { hwnd: isize, title: String },
    Region { x: i32, y: i32, width: u32, height: u32 },
}

/// Capture configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    pub source: CaptureSource,
    pub output_width: u32,
    pub output_height: u32,
    pub fps: u32,
    pub capture_cursor: bool,
    pub highlight_cursor: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            source: CaptureSource::Monitor { hmonitor: 0, name: "Primary".into() },
            output_width: 1920,
            output_height: 1080,
            fps: 60,
            capture_cursor: true,
            highlight_cursor: false,
        }
    }
}

/// A captured frame with raw BGRA pixel data
#[derive(Clone)]
pub struct CapturedFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub timestamp_ns: u64,
}

/// The capture engine manages screen capture
pub struct CaptureEngine {
    config: CaptureConfig,
    running: Arc<AtomicBool>,
    frame_sender: Sender<CapturedFrame>,
    frame_receiver: Receiver<CapturedFrame>,
    d3d_device: Option<ID3D11Device>,
    d3d_context: Option<ID3D11DeviceContext>,
    capture_thread: Option<std::thread::JoinHandle<()>>,
}

impl CaptureEngine {
    pub fn new(config: CaptureConfig) -> Self {
        let (frame_sender, frame_receiver) = bounded(4); // Bounded buffer for backpressure

        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            frame_sender,
            frame_receiver,
            d3d_device: None,
            d3d_context: None,
            capture_thread: None,
        }
    }

    /// Initialize Direct3D device
    fn init_d3d(&mut self) -> Result<()> {
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;

            self.d3d_device = device;
            self.d3d_context = context;
            info!("D3D11 device initialized for capture");
            Ok(())
        }
    }

    /// Get the frame receiver channel
    pub fn frame_receiver(&self) -> Receiver<CapturedFrame> {
        self.frame_receiver.clone()
    }

    /// Start screen capture
    pub fn start(&mut self) -> anyhow::Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.init_d3d().map_err(|e| anyhow::anyhow!("Failed to init D3D: {}", e))?;

        let device = self.d3d_device.clone().ok_or_else(|| anyhow::anyhow!("No D3D device"))?;
        let context = self.d3d_context.clone().ok_or_else(|| anyhow::anyhow!("No D3D context"))?;
        let config = self.config.clone();
        let running = self.running.clone();
        let sender = self.frame_sender.clone();

        running.store(true, Ordering::SeqCst);

        let handle = std::thread::spawn(move || {
            if let Err(e) = run_capture_loop(device, context, &config, &running, &sender) {
                error!("Capture loop error: {}", e);
            }
            running.store(false, Ordering::SeqCst);
        });

        self.capture_thread = Some(handle);
        info!("Capture engine started");
        Ok(())
    }

    /// Stop screen capture
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.capture_thread.take() {
            let _ = handle.join();
        }
        info!("Capture engine stopped");
    }

    /// Check if capture is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Update capture configuration
    pub fn update_config(&mut self, config: CaptureConfig) {
        self.config = config;
    }
}

impl Drop for CaptureEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Main capture loop using Windows Graphics Capture
fn run_capture_loop(
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    config: &CaptureConfig,
    running: &AtomicBool,
    sender: &Sender<CapturedFrame>,
) -> anyhow::Result<()> {
    unsafe {
        // Get DXGI device for WinRT interop
        let dxgi_device: IDXGIDevice = device.cast()?;

        // Create WinRT Direct3D device
        let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)?;
        let winrt_device: IDirect3DDevice = inspectable.cast()?;

        // Create capture item based on source
        let capture_item = create_capture_item(config)?;
        let size = capture_item.Size()?;

        info!(
            "Capture item created: {}x{}",
            size.Width, size.Height
        );

        // Create frame pool with 2 buffers
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;

        // Create capture session
        let session = frame_pool.CreateCaptureSession(&capture_item)?;

        // Configure cursor capture
        if let Ok(session3) = session.cast::<IGraphicsCaptureSession3>() {
            let _ = session3.SetIsCursorCaptureEnabled(config.capture_cursor);
        }

        // Start capture
        session.StartCapture()?;
        info!("Windows Graphics Capture started");

        let frame_interval = std::time::Duration::from_nanos(1_000_000_000 / config.fps as u64);
        let mut last_frame_time = std::time::Instant::now();
        let start_time = std::time::Instant::now();

        // Frame processing loop
        while running.load(Ordering::SeqCst) {
            // Rate limit to target FPS
            let elapsed = last_frame_time.elapsed();
            if elapsed < frame_interval {
                std::thread::sleep(frame_interval - elapsed);
            }
            last_frame_time = std::time::Instant::now();

            // Try to get a frame
            if let Ok(frame) = frame_pool.TryGetNextFrame() {
                let surface = frame.Surface()?;
                let timestamp_ns = start_time.elapsed().as_nanos() as u64;

                // Get the D3D11 texture from the surface
                if let Ok(frame_data) = copy_frame_to_cpu(&device, &context, &surface, config) {
                    let captured = CapturedFrame {
                        data: frame_data.data,
                        width: frame_data.width,
                        height: frame_data.height,
                        stride: frame_data.stride,
                        timestamp_ns,
                    };

                    // Send frame, dropping if channel is full (backpressure)
                    let _ = sender.try_send(captured);
                }

                // Check for size changes
                let new_size = frame.ContentSize()?;
                if new_size.Width != size.Width || new_size.Height != size.Height {
                    frame_pool.Recreate(
                        &winrt_device,
                        DirectXPixelFormat::B8G8R8A8UIntNormalized,
                        2,
                        new_size,
                    )?;
                }

                drop(frame);
            }
        }

        // Cleanup
        session.Close()?;
        frame_pool.Close()?;

        Ok(())
    }
}

/// Copy a captured surface to CPU memory
unsafe fn copy_frame_to_cpu(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    surface: &IDirect3DSurface,
    config: &CaptureConfig,
) -> anyhow::Result<CapturedFrame> {
    // Get the D3D11 texture through Direct3D interop
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    let texture: ID3D11Texture2D = access.GetInterface()?;

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    texture.GetDesc(&mut desc);

    // Create a staging texture for CPU access
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: desc.Width,
        Height: desc.Height,
        MipLevels: 1,
        ArraySize: 1,
        Format: desc.Format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: D3D11_BIND_FLAG(0),
        CPUAccessFlags: D3D11_CPU_ACCESS_READ,
        MiscFlags: D3D11_RESOURCE_MISC_FLAG(0),
    };

    let staging_texture = device.CreateTexture2D(&staging_desc, None)?;

    // Copy the frame to staging
    context.CopyResource(&staging_texture, &texture);

    // Map the staging texture
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    context.Map(
        &staging_texture,
        0,
        D3D11_MAP_READ,
        0,
        Some(&mut mapped),
    )?;

    let stride = mapped.RowPitch;
    let height = desc.Height;
    let data_size = (stride * height) as usize;

    let mut data = vec![0u8; data_size];
    std::ptr::copy_nonoverlapping(
        mapped.pData as *const u8,
        data.as_mut_ptr(),
        data_size,
    );

    context.Unmap(&staging_texture, 0);

    Ok(CapturedFrame {
        data,
        width: desc.Width,
        height: desc.Height,
        stride,
        timestamp_ns: 0,
    })
}

/// Create a capture item from the configured source
unsafe fn create_capture_item(config: &CaptureConfig) -> anyhow::Result<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;

    match &config.source {
        CaptureSource::Monitor { hmonitor, .. } => {
            let hmon = HMONITOR(*hmonitor as *mut std::ffi::c_void);
            let item: GraphicsCaptureItem = interop.CreateForMonitor(hmon)?;
            Ok(item)
        }
        CaptureSource::Window { hwnd, .. } => {
            let hwnd = HWND(*hwnd as *mut std::ffi::c_void);
            let item: GraphicsCaptureItem = interop.CreateForWindow(hwnd)?;
            Ok(item)
        }
        CaptureSource::Region { x, y, width, height } => {
            // For region capture, we capture the primary monitor and crop
            // Get primary monitor
            let hmon = windows::Win32::Graphics::Gdi::MonitorFromPoint(
                POINT { x: *x, y: *y },
                MONITOR_DEFAULTTOPRIMARY,
            );
            let item: GraphicsCaptureItem = interop.CreateForMonitor(hmon)?;
            Ok(item)
        }
    }
}

/// Enumerate capturable windows
pub fn enumerate_windows() -> Vec<(isize, String)> {
    let mut windows_list: Vec<(isize, String)> = Vec::new();

    unsafe {
        let callback_data = &mut windows_list as *mut Vec<(isize, String)>;
        let _ = EnumWindows(
            Some(enum_window_callback),
            LPARAM(callback_data as isize),
        );
    }

    windows_list
}

unsafe extern "system" fn enum_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows_list = &mut *(lparam.0 as *mut Vec<(isize, String)>);

    // Only include visible windows with titles
    if !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    let mut title = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut title);
    if len == 0 {
        return TRUE;
    }

    let title_str = String::from_utf16_lossy(&title[..len as usize]);
    if title_str.is_empty() {
        return TRUE;
    }

    // Skip certain system windows
    let skip = ["Program Manager", "Settings", "Microsoft Text Input"];
    if skip.iter().any(|s| title_str.contains(s)) {
        return TRUE;
    }

    windows_list.push((hwnd.0 as isize, title_str));

    TRUE
}

/// COM interface for D3D interop
#[windows::core::interface("A9B3D012-3DF2-4EE3-B8D1-8695F457D3C1")]
pub unsafe trait IDirect3DDxgiInterfaceAccess: IUnknown {
    unsafe fn GetInterface<T: Interface>(&self) -> Result<T>;
}

/// IGraphicsCaptureSession3 for cursor control (Windows 10 2004+)
#[windows::core::interface("F2CDD966-22AE-5EA1-9596-3A289344C3BE")]
pub unsafe trait IGraphicsCaptureSession3: IInspectable {
    fn SetIsCursorCaptureEnabled(&self, value: bool) -> Result<()>;
    fn IsCursorCaptureEnabled(&self) -> Result<bool>;
}
