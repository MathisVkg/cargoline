//! Windows-only capture (measured in the Task 06 spike):
//! - Windows.Graphics.Capture of the Star Citizen window (works while it is covered or
//!   unfocused), with the yellow border and the cursor off;
//! - `PrintWindow(PW_RENDERFULLCONTENT)` only if WGC fails;
//! - WGC of a whole monitor for the "Full display" setting.
//!
//! Nothing here focuses, moves or resizes a window or sends input. Every session,
//! frame pool, texture and GDI object is released before returning.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::core::{factory, Interface, BOOL, PWSTR};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{CloseHandle, HMODULE, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    MonitorFromPoint, MonitorFromWindow, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HMONITOR, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::HiDpi::{
    GetAwarenessFromDpiAwarenessContext, GetThreadDpiAwarenessContext,
    DPI_AWARENESS_PER_MONITOR_AWARE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowDisplayAffinity, GetWindowRect, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    WINDOW_DISPLAY_AFFINITY,
};

use super::select::{any_star_citizen, select_star_citizen, WindowCandidate, STAR_CITIZEN_CLASS};
use super::{CaptureBackend, CaptureError, CaptureSourceKind, Frame};

/// How long to wait for the first WGC frame.
pub const FRAME_TIMEOUT: Duration = Duration::from_secs(2);
/// `PW_RENDERFULLCONTENT`: ask DWM for the composed content (DirectX windows).
const PW_RENDERFULLCONTENT: u32 = 2;

type R<T> = Result<T, CaptureError>;

fn failed(context: &str, e: impl std::fmt::Display) -> CaptureError {
    CaptureError::Failed(format!("{context}: {e}"))
}

/// Initialises WinRT (multithreaded) for the current thread for the guard's lifetime.
pub struct WinRtThread(bool);

impl WinRtThread {
    pub fn init() -> Self {
        Self(unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for WinRtThread {
    fn drop(&mut self) {
        if self.0 {
            unsafe { RoUninitialize() };
        }
    }
}

/* ----------------------------------------------------------- enumeration */

fn wide_to_string(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

/// Full path of a process's executable (read-only query).
fn image_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn exe_name(pid: u32) -> String {
    image_path(pid)
        .and_then(|full| full.rsplit('\\').next().map(str::to_string))
        .unwrap_or_default()
}

/// `SystemProcessIdInformation`: a process's image name by pid, without a process handle.
const SYSTEM_PROCESS_ID_INFORMATION:
    windows::Wdk::System::SystemInformation::SYSTEM_INFORMATION_CLASS =
    windows::Wdk::System::SystemInformation::SYSTEM_INFORMATION_CLASS(88);

#[repr(C)]
struct ProcessIdInformation {
    process_id: windows::Win32::Foundation::HANDLE,
    image_name: windows::Win32::Foundation::UNICODE_STRING,
}

/// `C:\…\StarCitizen.exe` for a pid, via `NtQuerySystemInformation`: read-only, **no process
/// handle**, so anti-cheat protection of the game process does not matter.
pub fn image_path_without_handle(pid: u32) -> Option<String> {
    let mut buf = vec![0u16; 2048];
    let mut info = ProcessIdInformation {
        process_id: windows::Win32::Foundation::HANDLE(pid as usize as *mut c_void),
        image_name: windows::Win32::Foundation::UNICODE_STRING {
            Length: 0,
            MaximumLength: (buf.len() * 2) as u16,
            Buffer: PWSTR(buf.as_mut_ptr()),
        },
    };
    let status = unsafe {
        windows::Wdk::System::SystemInformation::NtQuerySystemInformation(
            SYSTEM_PROCESS_ID_INFORMATION,
            &mut info as *mut ProcessIdInformation as *mut c_void,
            std::mem::size_of::<ProcessIdInformation>() as u32,
            std::ptr::null_mut(),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = (info.image_name.Length as usize / 2).min(buf.len());
    nt_to_dos_path(&String::from_utf16_lossy(&buf[..len]))
}

/// The drive letters present (`GetLogicalDrives`).
pub fn logical_drives() -> Vec<char> {
    let mask = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| (b'A' + i) as char)
        .collect()
}

/// `\Device\HarddiskVolume3\Program Files\…` → `C:\Program Files\…`.
fn nt_to_dos_path(nt: &str) -> Option<String> {
    for drive in logical_drives() {
        let name: Vec<u16> = format!("{drive}:").encode_utf16().chain([0]).collect();
        let mut target = [0u16; 512];
        let n = unsafe {
            windows::Win32::Storage::FileSystem::QueryDosDeviceW(
                windows::core::PCWSTR(name.as_ptr()),
                Some(&mut target),
            )
        };
        if n == 0 {
            continue;
        }
        let device = wide_to_string(&target);
        if let Some(rest) = nt.strip_prefix(&device) {
            if rest.starts_with('\\') {
                return Some(format!("{drive}:{rest}"));
            }
        }
    }
    None
}

/// The executable of the running Star Citizen game window, e.g.
/// `…\StarCitizen\LIVE\Bin64\StarCitizen.exe` (Game.log detection). Found from the game's
/// window and pid, then [`image_path_without_handle`]: the game's process is never opened.
/// None if not running.
pub fn star_citizen_exe_path() -> Option<std::path::PathBuf> {
    let mut found = None;
    for window in star_citizen_windows().ok()? {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd(window.handle), Some(&mut pid)) };
        if let Some(path) = image_path_without_handle(pid) {
            let is_game = std::path::Path::new(&path).file_name().is_some_and(|n| {
                n.to_string_lossy()
                    .eq_ignore_ascii_case(super::select::STAR_CITIZEN_EXE)
            });
            if is_game {
                found = Some(std::path::PathBuf::from(path));
                break;
            }
        }
    }
    found
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam is the &mut Vec passed by `star_citizen_windows` for this call only.
    let out = unsafe { &mut *(lparam.0 as *mut Vec<WindowCandidate>) };
    let mut class = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut class) };
    let class = wide_to_string(&class[..len.max(0) as usize]);
    // Cheap filter first: only CryENGINE windows need the process lookup.
    if !class.eq_ignore_ascii_case(STAR_CITIZEN_CLASS) {
        return BOOL(1);
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let mut rect = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    let mut cloaked = 0u32;
    let _ = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            4,
        )
    };
    out.push(WindowCandidate {
        handle: hwnd.0 as isize,
        exe: exe_name(pid),
        class,
        visible: unsafe { IsWindowVisible(hwnd) }.as_bool(),
        minimized: unsafe { IsIconic(hwnd) }.as_bool(),
        cloaked: cloaked != 0,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    });
    BOOL(1)
}

/// Top-level `CryENGINE` windows (EnumWindows only reports top-level windows).
pub fn star_citizen_windows() -> R<Vec<WindowCandidate>> {
    let mut list: Vec<WindowCandidate> = Vec::new();
    unsafe { EnumWindows(Some(collect_window), LPARAM(&mut list as *mut _ as isize)) }
        .map_err(|e| failed("listing windows", e))?;
    Ok(list)
}

fn hwnd(handle: isize) -> HWND {
    HWND(handle as *mut c_void)
}

/// Whether this thread is per-monitor DPI aware (Tauri/tao sets V2 for the process).
pub fn per_monitor_dpi_aware() -> bool {
    unsafe {
        GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext())
            == DPI_AWARENESS_PER_MONITOR_AWARE
    }
}

/* --------------------------------------------------------------- public */

/// Captures the Star Citizen window: WGC first, `PrintWindow` only if WGC fails.
pub fn capture_star_citizen_window() -> R<Frame> {
    let windows = star_citizen_windows()?;
    let target = select_star_citizen(&windows)?.handle;
    match wgc_window(hwnd(target)) {
        Ok(frame) => Ok(frame),
        Err(wgc_error) => {
            eprintln!("[cargo-line] WGC window capture failed ({wgc_error}); trying PrintWindow");
            // Re-check: the window may have been minimised in the meantime.
            if unsafe { IsIconic(hwnd(target)) }.as_bool() {
                return Err(CaptureError::Minimized);
            }
            print_window(hwnd(target))
                .map_err(|e| failed("both capture methods failed", format!("{wgc_error}; {e}")))
        }
    }
}

/// Captures the monitor Star Citizen is on, or the primary monitor if it is not running.
pub fn capture_display() -> R<Frame> {
    let windows = star_citizen_windows().unwrap_or_default();
    let monitor = match any_star_citizen(&windows) {
        Some(w) => unsafe { MonitorFromWindow(hwnd(w.handle), MONITOR_DEFAULTTOPRIMARY) },
        None => unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) },
    };
    wgc_monitor(monitor)
}

/// Excludes `window` from screen capture and returns its previous affinity.
pub fn exclude_from_capture(window: isize) -> R<u32> {
    let mut previous = 0u32;
    unsafe {
        GetWindowDisplayAffinity(hwnd(window), &mut previous)
            .map_err(|e| failed("reading display affinity", e))?;
        SetWindowDisplayAffinity(hwnd(window), WDA_EXCLUDEFROMCAPTURE)
            .map_err(|e| failed("excluding Cargo Line from capture", e))?;
    }
    Ok(previous)
}

/// Restores the affinity returned by [`exclude_from_capture`].
pub fn restore_affinity(window: isize, previous: u32) {
    if let Err(e) =
        unsafe { SetWindowDisplayAffinity(hwnd(window), WINDOW_DISPLAY_AFFINITY(previous)) }
    {
        eprintln!("[cargo-line] could not restore the display affinity: {e}");
    }
}

/* ------------------------------------------------------------------ WGC */

fn d3d_device() -> R<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .map_err(|e| failed("creating the Direct3D device", e))?;
    match (device, context) {
        (Some(d), Some(c)) => Ok((d, c)),
        _ => Err(CaptureError::Failed("no Direct3D device".into())),
    }
}

/// Closes the session and the frame pool on every path.
struct WgcSession {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Drop for WgcSession {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

fn wgc_item_for_window(window: HWND) -> R<GraphicsCaptureItem> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|e| failed("WGC", e))?;
    unsafe { interop.CreateForWindow(window) }.map_err(|e| failed("WGC window item", e))
}

fn wgc_window(window: HWND) -> R<Frame> {
    wgc_capture(&wgc_item_for_window(window)?, CaptureSourceKind::Window)
}

fn wgc_monitor(monitor: HMONITOR) -> R<Frame> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|e| failed("WGC", e))?;
    let item: GraphicsCaptureItem =
        unsafe { interop.CreateForMonitor(monitor) }.map_err(|e| failed("WGC monitor item", e))?;
    wgc_capture(&item, CaptureSourceKind::Display)
}

fn wgc_capture(item: &GraphicsCaptureItem, source: CaptureSourceKind) -> R<Frame> {
    if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
        return Err(CaptureError::Unsupported);
    }
    let started = Instant::now();
    let (device, context) = d3d_device()?;
    let dxgi: IDXGIDevice = device.cast().map_err(|e| failed("DXGI device", e))?;
    let winrt_device: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .and_then(|d| d.cast())
        .map_err(|e| failed("WinRT device", e))?;
    let size = item.Size().map_err(|e| failed("item size", e))?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )
    .map_err(|e| failed("frame pool", e))?;
    let session = pool
        .CreateCaptureSession(item)
        .map_err(|e| failed("capture session", e))?;
    let wgc = WgcSession { pool, session };
    // Both are best effort: older Windows builds reject them without failing the capture.
    let _ = wgc.session.SetIsBorderRequired(false);
    let _ = wgc.session.SetIsCursorCaptureEnabled(false);
    wgc.session
        .StartCapture()
        .map_err(|e| failed("starting capture", e))?;

    let frame = loop {
        if let Ok(frame) = wgc.pool.TryGetNextFrame() {
            break frame;
        }
        if started.elapsed() > FRAME_TIMEOUT {
            return Err(CaptureError::Failed("no frame within 2 s".into()));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let content = frame.ContentSize().map_err(|e| failed("content size", e))?;
    let access: IDirect3DDxgiInterfaceAccess = frame
        .Surface()
        .and_then(|s| s.cast())
        .map_err(|e| failed("frame surface", e))?;
    let texture: ID3D11Texture2D =
        unsafe { access.GetInterface() }.map_err(|e| failed("frame texture", e))?;
    let pixels = read_texture(&device, &context, &texture, content.Width, content.Height);
    let _ = frame.Close();
    drop(wgc);
    let (width, height, bgra) = pixels?;
    Ok(Frame {
        width,
        height,
        bgra,
        source,
        backend: CaptureBackend::Wgc,
    })
}

/// Copies a BGRA8 texture to the CPU, cropped to the frame's content size.
fn read_texture(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    content_w: i32,
    content_h: i32,
) -> R<(u32, u32, Vec<u8>)> {
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(CaptureError::Failed(format!(
                "unsupported pixel format {:?}",
                desc.Format
            )));
        }
        let width = desc.Width.min(content_w.max(1) as u32);
        let height = desc.Height.min(content_h.max(1) as u32);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut staging = None;
        device
            .CreateTexture2D(&desc, None, Some(&mut staging))
            .map_err(|e| failed("staging texture", e))?;
        let staging = staging.ok_or_else(|| CaptureError::Failed("no staging texture".into()))?;
        context.CopyResource(&staging, texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| failed("mapping the frame", e))?;
        let pitch = mapped.RowPitch as usize;
        let row_bytes = width as usize * 4;
        let src =
            std::slice::from_raw_parts(mapped.pData as *const u8, pitch * desc.Height as usize);
        let mut bgra = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height as usize {
            bgra.extend_from_slice(&src[y * pitch..y * pitch + row_bytes]);
        }
        context.Unmap(&staging, 0);
        Ok((width, height, bgra))
    }
}

/* ---------------------------------------------------------- PrintWindow */

fn print_window(window: HWND) -> R<Frame> {
    unsafe {
        let mut r = RECT::default();
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .map_err(|e| failed("window bounds", e))?;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if w <= 0 || h <= 0 {
            return Err(CaptureError::Failed("the window has no size".into()));
        }
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(memory, bitmap.into());
        let printed =
            PrintWindow(window, memory, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();

        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h; // top-down rows
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        let mut bgra = vec![0u8; w as usize * h as usize * 4];
        let lines = GetDIBits(
            memory,
            bitmap,
            0,
            h as u32,
            Some(bgra.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);

        if !printed || lines != h {
            return Err(CaptureError::Failed("PrintWindow returned no image".into()));
        }
        // GDI leaves alpha at 0; a fully black image means DirectX content was not captured.
        if bgra
            .as_chunks::<4>()
            .0
            .iter()
            .step_by(97)
            .all(|[b, g, r, _]| *b < 4 && *g < 4 && *r < 4)
        {
            return Err(CaptureError::Failed(
                "PrintWindow returned a black image".into(),
            ));
        }
        Ok(Frame {
            width: w as u32,
            height: h as u32,
            bgra,
            source: CaptureSourceKind::Window,
            backend: CaptureBackend::PrintWindow,
        })
    }
}
