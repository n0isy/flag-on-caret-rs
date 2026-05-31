// FlagOnCaret — Rust port of the LangBarXX caret/cursor-flag feature (Krot66).
//
// Shows the current keyboard-layout flag (1) at the text caret and (2) overlaid
// on the mouse cursor (I-beam + arrow). Faithful port: caret detection does
// UIA + MSAA + GetGUIThreadInfo (see `caret.rs`); the I-beam is colour-inverted
// on dark backgrounds; layouts without a PNG get a gradient text flag; and the
// guards (#32768 menu, console window, secure desktop, full screen) and DPI
// awareness from the original are reproduced.

#![windows_subsystem = "windows"]

mod caret;
mod langcode;

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use trayicon::{MenuBuilder, TrayIconBuilder};

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Globalization::LCIDToLocaleName;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, GetDC, GetPixel,
    InvalidateRect, ReleaseDC, SelectObject, HBITMAP, HDC, PAINTSTRUCT, SRCCOPY,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipAddPathArcI, GdipClosePathFigure, GdipCreateBitmapFromScan0, GdipCreateBitmapFromStream,
    GdipCreateFont, GdipCreateFontFamilyFromName, GdipCreateHBITMAPFromBitmap,
    GdipCreateHICONFromBitmap, GdipCreateImageAttributes, GdipCreateLineBrushFromRectI,
    GdipCreatePath, GdipCreateSolidFill, GdipCreateStringFormat, GdipDeleteBrush, GdipDeleteFont,
    GdipDeleteFontFamily, GdipDeleteGraphics, GdipDeletePath, GdipDeleteStringFormat,
    GdipDisposeImage, GdipDisposeImageAttributes, GdipDrawImageRectI, GdipDrawImageRectRectI,
    GdipCreateBitmapFromHICON, GdipDrawString, GdipFillPath, GdipGetImageGraphicsContext,
    GdipGetImageHeight, GdipGetImageWidth, GdipGraphicsClear, GdipSetImageAttributesColorMatrix,
    GdipSetInterpolationMode, GdipSetSmoothingMode, GdipSetStringFormatAlign,
    GdipSetStringFormatLineAlign, GdiplusStartup, GdiplusStartupInput, ColorMatrix, GpBitmap,
    GpGraphics, Rect as GpRect, RectF,
};
use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop};
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;

use windows::core::Interface;
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetCursorInfo, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowThreadProcessId, LoadCursorW,
    GetClassNameW, GetIconInfo, PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes,
    SetSystemCursor, SetTimer, SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage,
    WindowFromPoint, CURSORINFO, ICONINFO, IDC_ARROW, IDC_IBEAM, MSG, WNDCLASSW,
};

// ---- Win32 constants not always re-exported by feature ----
const WM_PAINT: u32 = 0x000F;
const WM_DESTROY: u32 = 0x0002;
const WS_POPUP: u32 = 0x8000_0000;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_TOPMOST: u32 = 0x0000_0008;
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const SW_HIDE: i32 = 0;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_SHOWWINDOW: u32 = 0x0040;
const LWA_COLORKEY: u32 = 0x0000_0001;
const HWND_TOPMOST: isize = -1;
const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;
const SPI_SETCURSORS: u32 = 0x0057;
const OCR_NORMAL: u32 = 32512;
const OCR_IBEAM: u32 = 32513;
const PIXELFORMAT_32BPP_ARGB: i32 = 0x0026_200A;
const CLR_INVALID: u32 = 0xFFFF_FFFF;
const UNIT_PIXEL: i32 = 2;

// ---- Fixed parameters (LangBarXX defaults) ----
const DX: i32 = 16;
const DY: i32 = -12;
const FLAG_W: i32 = 22;
const FLAG_H: i32 = (FLAG_W * 3) / 4; // 16
const INVERT_THRESHOLD: f64 = 100.0;
// Cursor flag geometry, as ratios of the (DPI-scaled) cursor size. The flag is a
// small badge on the user's own cursor — deliberately smaller than the caret flag.
const CFLAG_W_RATIO: f32 = 0.42;
const CFLAG_ASPECT: f32 = 12.0 / 18.0; // height / width
const CFLAG_X_RATIO: f32 = 0.12;
const CFLAG_Y_RATIO: f32 = 0.66;
const KEY_ARGB: u32 = 0xFF3A_3B3C;
const KEY_COLORREF: COLORREF = 0x003C_3B3A;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
enum TrayEvent {
    Exit,
}

#[derive(Clone, Copy, PartialEq)]
enum CursorKind {
    IBeam,
    Arrow,
}

/// A snapshot of one of the user's real system cursors (taken before we start
/// replacing cursors), with its hotspot — so flagged cursors look native.
#[derive(Clone, Copy)]
struct CapturedCursor {
    bmp: *mut GpBitmap,
    w: i32,
    h: i32,
    hx: i32,
    hy: i32,
}

struct State {
    caret_hwnd: HWND,
    flag_dc: HDC,
    flag_bmp: HBITMAP,
    flag_w: i32,
    flag_h: i32,
    last_caret_layout: u32,
    src_cache: HashMap<u32, *mut GpBitmap>,
    arrow_cur: Option<CapturedCursor>,
    ibeam_cur: Option<CapturedCursor>,
    cursor_kind: Option<CursorKind>,
    cursor_layout: u32,
    cursor_dark: bool,
    cursor_dpi: u32,
    cursor_time: Instant,
}

impl State {
    fn new() -> Self {
        State {
            caret_hwnd: std::ptr::null_mut(),
            flag_dc: std::ptr::null_mut(),
            flag_bmp: std::ptr::null_mut(),
            flag_w: FLAG_W,
            flag_h: FLAG_H,
            last_caret_layout: 0,
            src_cache: HashMap::new(),
            arrow_cur: None,
            ibeam_cur: None,
            cursor_kind: None,
            cursor_layout: 0,
            cursor_dark: false,
            cursor_dpi: 0,
            cursor_time: Instant::now(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::new());
    // Console layout is expensive to query (AttachConsole), so cache it briefly.
    static CONSOLE_CACHE: RefCell<(u32, Instant)> = RefCell::new((0, Instant::now()));
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Assets are baked into the exe (no external files needed at runtime).
mod assets {
    macro_rules! flag {
        ($name:literal) => {
            ($name, include_bytes!(concat!("../assets/flags/", $name, ".png")) as &[u8])
        };
    }
    static FLAGS: &[(&str, &[u8])] = &[
        flag!("be-BY"),
        flag!("de-DE"),
        flag!("en-GB"),
        flag!("en-US"),
        flag!("fr-FR"),
        flag!("pl-PL"),
        flag!("ru-RU"),
        flag!("uk-UA"),
    ];
    pub fn flag(locale: &str) -> Option<&'static [u8]> {
        FLAGS.iter().find(|(n, _)| *n == locale).map(|(_, b)| *b)
    }
}

/// Decode an embedded PNG into a GDI+ bitmap via an in-memory IStream.
/// GDI+ keeps its own reference to the stream, so dropping ours is fine.
fn gp_from_png(bytes: &[u8]) -> *mut GpBitmap {
    unsafe {
        let stream = match SHCreateMemStream(Some(bytes)) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let mut bmp: *mut GpBitmap = std::ptr::null_mut();
        GdipCreateBitmapFromStream(stream.as_raw(), &mut bmp);
        bmp
    }
}

fn class_of(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, buf.as_mut_ptr(), 256);
        if n <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buf[..n as usize])
        }
    }
}

/// LANGID of the focused thread's HKL, with the console special-case from
/// LangBarXX `InputLayout.ahk` (`GetConsoleKeyboardLayoutName` via AttachConsole).
fn current_layout() -> u32 {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return 0;
        }
        if class_of(fg) == "ConsoleWindowClass" {
            return CONSOLE_CACHE.with(|c| {
                let mut c = c.borrow_mut();
                if c.0 != 0 && c.1.elapsed().as_millis() < 200 {
                    return c.0;
                }
                if let Some(l) = console_layout(fg) {
                    *c = (l, Instant::now());
                    return l;
                }
                c.0
            });
        }
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let hkl = GetKeyboardLayout(tid);
        (hkl as usize as u32) & 0xFFFF
    }
}

/// Console keyboard layout via AttachConsole + kernel32!GetConsoleKeyboardLayoutNameW.
fn console_layout(hwnd: HWND) -> Option<u32> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || AttachConsole(pid) == 0 {
            return None;
        }
        let k32 = GetModuleHandleW(wide("kernel32.dll").as_ptr());
        let proc = GetProcAddress(k32, c"GetConsoleKeyboardLayoutNameW".as_ptr() as *const u8);
        let mut klid = [0u16; 16];
        let ok = match proc {
            Some(p) => {
                let f: unsafe extern "system" fn(*mut u16) -> i32 = std::mem::transmute(p);
                f(klid.as_mut_ptr())
            }
            None => 0,
        };
        FreeConsole();
        if ok == 0 {
            return None;
        }
        let end = klid.iter().position(|&c| c == 0).unwrap_or(klid.len());
        let s = String::from_utf16_lossy(&klid[..end]);
        u32::from_str_radix(s.trim(), 16).ok().map(|v| v & 0xFFFF)
    }
}

/// DPI of the monitor under the mouse cursor (per-monitor aware).
fn cursor_dpi() -> u32 {
    unsafe {
        let mut pt: POINT = std::mem::zeroed();
        GetCursorPos(&mut pt);
        let hw = WindowFromPoint(pt);
        let d = if hw.is_null() { 0 } else { GetDpiForWindow(hw) };
        if d == 0 { 96 } else { d }
    }
}

/// Locale code: LangBarXX table first, LCIDToLocaleName as a backstop.
fn locale_name(langid: u32) -> Option<String> {
    if let Some(s) = langcode::lookup(langid) {
        return Some(s.to_string());
    }
    unsafe {
        let mut buf = [0u16; 85];
        let n = LCIDToLocaleName(langid, buf.as_mut_ptr(), buf.len() as i32, 0);
        if n <= 1 {
            None
        } else {
            Some(String::from_utf16_lossy(&buf[..(n as usize - 1)]))
        }
    }
}

/// 2-letter code for a text flag, mirroring LangBarXX's `lt` derivation.
fn text_code(langid: u32) -> String {
    let code = locale_name(langid).unwrap_or_else(|| format!("{langid:04X}"));
    let parts: Vec<&str> = code.split('-').collect();
    let mut lt = *parts.last().unwrap_or(&code.as_str());
    if matches!(lt, "Cyrl" | "Latn" | "Arab" | "tradnl") || lt.len() > 3 {
        lt = parts[0];
    }
    lt.chars().take(2).collect::<String>().to_uppercase()
}

/// Source flag bitmap for a langid (cached): PNG file, else a text flag.
fn flag_src(st: &mut State, langid: u32) -> *mut GpBitmap {
    if let Some(p) = st.src_cache.get(&langid) {
        return *p;
    }
    let mut bmp: *mut GpBitmap = std::ptr::null_mut();
    if let Some(loc) = locale_name(langid) {
        if let Some(png) = assets::flag(&loc) {
            bmp = gp_from_png(png);
        }
    }
    if bmp.is_null() {
        bmp = make_text_flag(langid);
    }
    st.src_cache.insert(langid, bmp);
    bmp
}

/// Gradient text flag (64x48) with the 2-letter code — fallback when no PNG.
fn make_text_flag(langid: u32) -> *mut GpBitmap {
    let lt = wide(&text_code(langid));
    unsafe {
        let mut bmp: *mut GpBitmap = std::ptr::null_mut();
        GdipCreateBitmapFromScan0(64, 48, 0, PIXELFORMAT_32BPP_ARGB, std::ptr::null(), &mut bmp);
        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipGetImageGraphicsContext(bmp as *mut _, &mut g) != 0 {
            return bmp;
        }
        GdipSetSmoothingMode(g, 4);

        // Vertical gradient background.
        let rect = GpRect { X: 0, Y: 0, Width: 64, Height: 48 };
        let mut brush = std::ptr::null_mut();
        GdipCreateLineBrushFromRectI(&rect, 0xFF33_4B63, 0xFF22_323F, 1, 1, &mut brush);
        // Rounded-rect path (radius 6).
        let mut path = std::ptr::null_mut();
        GdipCreatePath(0, &mut path);
        let (w, h, r) = (63i32, 47i32, 6i32);
        GdipAddPathArcI(path, 0, 0, 2 * r, 2 * r, 180.0, 90.0);
        GdipAddPathArcI(path, w - 2 * r, 0, 2 * r, 2 * r, 270.0, 90.0);
        GdipAddPathArcI(path, w - 2 * r, h - 2 * r, 2 * r, 2 * r, 0.0, 90.0);
        GdipAddPathArcI(path, 0, h - 2 * r, 2 * r, 2 * r, 90.0, 90.0);
        GdipClosePathFigure(path);
        GdipFillPath(g, brush as *mut _, path);
        GdipDeletePath(path);
        GdipDeleteBrush(brush as *mut _);

        // Centered bold text.
        let family_name = wide("Arial");
        let mut family = std::ptr::null_mut();
        if GdipCreateFontFamilyFromName(family_name.as_ptr(), std::ptr::null_mut(), &mut family) == 0
        {
            let mut font = std::ptr::null_mut();
            GdipCreateFont(family as *const _, 30.0, 1, UNIT_PIXEL, &mut font); // FontStyleBold=1
            let mut fmt = std::ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut fmt);
            GdipSetStringFormatAlign(fmt, 1); // Center
            GdipSetStringFormatLineAlign(fmt, 1); // Center
            let mut text_brush = std::ptr::null_mut();
            GdipCreateSolidFill(0xFFEE_EEEE, &mut text_brush);
            let layout = RectF { X: 0.0, Y: 2.0, Width: 64.0, Height: 46.0 };
            GdipDrawString(
                g,
                lt.as_ptr(),
                -1,
                font as *const _,
                &layout,
                fmt as *const _,
                text_brush as *const _,
            );
            GdipDeleteBrush(text_brush as *mut _);
            GdipDeleteStringFormat(fmt);
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
        }
        GdipDeleteGraphics(g);
        bmp
    }
}

/// Scaled HBITMAP (color-key bg) from a source bitmap, for the caret window.
fn scaled_flag_hbitmap(src: *mut GpBitmap, w: i32, h: i32) -> HBITMAP {
    unsafe {
        let mut dst: *mut GpBitmap = std::ptr::null_mut();
        GdipCreateBitmapFromScan0(w, h, 0, PIXELFORMAT_32BPP_ARGB, std::ptr::null(), &mut dst);
        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipGetImageGraphicsContext(dst as *mut _, &mut g) == 0 {
            GdipSetSmoothingMode(g, 4);
            GdipSetInterpolationMode(g, 7);
            GdipGraphicsClear(g, KEY_ARGB);
            GdipDrawImageRectI(g, src as *mut _, 0, 0, w, h);
            GdipDeleteGraphics(g);
        }
        let mut hbm: HBITMAP = std::ptr::null_mut();
        GdipCreateHBITMAPFromBitmap(dst, &mut hbm, KEY_ARGB);
        GdipDisposeImage(dst as *mut _);
        hbm
    }
}

/// Capture one of the user's current system cursors (by IDC_*) as a GDI+ bitmap
/// plus its hotspot. Call before any SetSystemCursor replacement.
fn capture_cursor(idc: windows_sys::core::PCWSTR) -> Option<CapturedCursor> {
    unsafe {
        let hcur = LoadCursorW(std::ptr::null_mut(), idc);
        if hcur.is_null() {
            return None;
        }
        let mut bmp: *mut GpBitmap = std::ptr::null_mut();
        if GdipCreateBitmapFromHICON(hcur as _, &mut bmp) != 0 || bmp.is_null() {
            return None;
        }
        let (mut w, mut h) = (0u32, 0u32);
        GdipGetImageWidth(bmp as *mut _, &mut w);
        GdipGetImageHeight(bmp as *mut _, &mut h);
        let (mut hx, mut hy) = (w as i32 / 2, h as i32 / 2);
        let mut ii: ICONINFO = std::mem::zeroed();
        if GetIconInfo(hcur, &mut ii) != 0 {
            hx = ii.xHotspot as i32;
            hy = ii.yHotspot as i32;
            if !ii.hbmMask.is_null() {
                DeleteObject(ii.hbmMask);
            }
            if !ii.hbmColor.is_null() {
                DeleteObject(ii.hbmColor);
            }
        }
        Some(CapturedCursor { bmp, w: w as i32, h: h as i32, hx, hy })
    }
}

/// Invert color matrix (LangBarXX GenerateColorMatrix modus 6).
fn invert_matrix() -> ColorMatrix {
    ColorMatrix {
        m: [
            -1.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, -1.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, -1.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, 0.0, //
            1.0, 1.0, 1.0, 0.0, 1.0,
        ],
    }
}

/// Compose a flagged cursor HICON from the user's real cursor + a small flag.
/// The cursor is scaled by the monitor DPI; the canvas is centred on the real
/// hotspot so GdipCreateHICONFromBitmap (centre hotspot) preserves it.
fn build_flagged_cursor(
    cur: &CapturedCursor,
    flag: *mut GpBitmap,
    kind: CursorKind,
    dark: bool,
    dpi: u32,
) -> isize {
    let f = dpi as f32 / 96.0;
    let sw = (cur.w as f32 * f).round() as i32;
    let sh = (cur.h as f32 * f).round() as i32;
    let shx = (cur.hx as f32 * f).round() as i32;
    let shy = (cur.hy as f32 * f).round() as i32;
    if sw <= 0 || sh <= 0 {
        return 0;
    }
    let fw = (sw as f32 * CFLAG_W_RATIO).round().max(1.0) as i32;
    let fh = (fw as f32 * CFLAG_ASPECT).round().max(1.0) as i32;
    let fx = (sw as f32 * CFLAG_X_RATIO).round() as i32;
    let fy = (sh as f32 * CFLAG_Y_RATIO).round() as i32;

    let xs = [-shx, sw - shx, fx - shx, fx + fw - shx];
    let ys = [-shy, sh - shy, fy - shy, fy + fh - shy];
    let ext = xs.iter().chain(ys.iter()).map(|v| v.abs()).max().unwrap_or(sw);
    let s = 2 * ext + 4;
    let center = s / 2;
    let (cx, cy) = (center - shx, center - shy);

    unsafe {
        let mut canvas: *mut GpBitmap = std::ptr::null_mut();
        GdipCreateBitmapFromScan0(s, s, 0, PIXELFORMAT_32BPP_ARGB, std::ptr::null(), &mut canvas);
        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipGetImageGraphicsContext(canvas as *mut _, &mut g) != 0 {
            return 0;
        }
        GdipSetSmoothingMode(g, 2);
        GdipSetInterpolationMode(g, 7);
        match kind {
            CursorKind::IBeam => {
                draw_cursor(g, cur, cx, cy, sw, sh, dark);
                GdipDrawImageRectI(g, flag as *mut _, cx + fx, cy + fy, fw, fh);
            }
            CursorKind::Arrow => {
                GdipDrawImageRectI(g, flag as *mut _, cx + fx, cy + fy, fw, fh);
                draw_cursor(g, cur, cx, cy, sw, sh, false);
            }
        }
        GdipDeleteGraphics(g);
        let mut hicon: isize = 0;
        GdipCreateHICONFromBitmap(canvas, &mut hicon as *mut isize as *mut _);
        GdipDisposeImage(canvas as *mut _);
        hicon
    }
}

/// Draw the captured cursor scaled to dw×dh at (dx,dy), optionally colour-inverted.
unsafe fn draw_cursor(
    g: *mut GpGraphics,
    cur: &CapturedCursor,
    dx: i32,
    dy: i32,
    dw: i32,
    dh: i32,
    dark: bool,
) {
    unsafe {
        if !dark {
            GdipDrawImageRectI(g, cur.bmp as *mut _, dx, dy, dw, dh);
            return;
        }
        let mut attr = std::ptr::null_mut();
        GdipCreateImageAttributes(&mut attr);
        let m = invert_matrix();
        GdipSetImageAttributesColorMatrix(attr, 0, 1, &m, std::ptr::null(), 0);
        GdipDrawImageRectRectI(
            g, cur.bmp as *mut _, dx, dy, dw, dh, 0, 0, cur.w, cur.h, UNIT_PIXEL, attr, 0,
            std::ptr::null_mut(),
        );
        GdipDisposeImageAttributes(attr);
    }
}

/// Current global cursor type (after replacement, LoadCursorW still maps).
fn a_cursor() -> Option<CursorKind> {
    unsafe {
        let mut ci: CURSORINFO = std::mem::zeroed();
        ci.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
        if GetCursorInfo(&mut ci) == 0 {
            return None;
        }
        let ibeam = LoadCursorW(std::ptr::null_mut(), IDC_IBEAM);
        let arrow = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        if ci.hCursor == ibeam {
            Some(CursorKind::IBeam)
        } else if ci.hCursor == arrow {
            Some(CursorKind::Arrow)
        } else {
            None
        }
    }
}

/// Average background brightness under the mouse is below the invert threshold.
fn cursor_bg_dark() -> bool {
    unsafe {
        let mut pt: POINT = std::mem::zeroed();
        GetCursorPos(&mut pt);
        let hdc = GetDC(std::ptr::null_mut());
        let mut sum = 0.0f64;
        let mut n = 0.0f64;
        for (dx, dy) in [(0, 0), (-10, -10), (10, 10)] {
            let c = GetPixel(hdc, pt.x + dx, pt.y + dy);
            if c == CLR_INVALID {
                continue;
            }
            let r = (c & 0xFF) as f64;
            let gg = ((c >> 8) & 0xFF) as f64;
            let b = ((c >> 16) & 0xFF) as f64;
            sum += (0.241 * r * r + 0.691 * gg * gg + 0.068 * b * b).sqrt();
            n += 1.0;
        }
        ReleaseDC(std::ptr::null_mut(), hdc);
        n > 0.0 && (sum / n) < INVERT_THRESHOLD
    }
}

fn restore_cursors() {
    unsafe {
        SystemParametersInfoW(SPI_SETCURSORS, 0, std::ptr::null_mut(), 0);
    }
}

fn is_fullscreen() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        let mut r: RECT = std::mem::zeroed();
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(fg, &mut r);
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        (r.right - r.left) >= sw && (r.bottom - r.top) >= sh
    }
}

/// A context menu (#32768) is open somewhere.
fn menu_open() -> bool {
    unsafe { !FindWindowW(wide("#32768").as_ptr(), std::ptr::null()).is_null() }
}

/// Input desktop is accessible (not a secure/locked desktop).
fn input_desktop_ok() -> bool {
    unsafe {
        let hd = OpenInputDesktop(0, 0, 0x0001); // DESKTOP_READOBJECTS
        if hd.is_null() {
            false
        } else {
            CloseDesktop(hd);
            true
        }
    }
}

fn foreground_class() -> String {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            String::new()
        } else {
            class_of(fg)
        }
    }
}

// ---- Caret window proc ----
unsafe extern "system" fn caret_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                STATE.with(|s| {
                    let st = s.borrow();
                    let mut ps: PAINTSTRUCT = std::mem::zeroed();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    if !st.flag_dc.is_null() {
                        BitBlt(hdc, 0, 0, st.flag_w, st.flag_h, st.flag_dc, 0, 0, SRCCOPY);
                    }
                    EndPaint(hwnd, &ps);
                });
                0
            }
            WM_DESTROY => 0,
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

// ---- Timers ----
unsafe extern "system" fn caret_timer(_h: HWND, _m: u32, _id: usize, _t: u32) {
  unsafe {
    let langid = current_layout();
    if langid == 0 || is_fullscreen() || menu_open() {
        STATE.with(|s| ShowWindow(s.borrow().caret_hwnd, SW_HIDE));
        return;
    }
    match caret::caret_pos() {
        None => {
            STATE.with(|s| ShowWindow(s.borrow().caret_hwnd, SW_HIDE));
        }
        Some((cx, cy)) => STATE.with(|s| {
            let mut st = s.borrow_mut();
            if st.last_caret_layout != langid || st.flag_dc.is_null() {
                let src = flag_src(&mut st, langid);
                let hbm = scaled_flag_hbitmap(src, FLAG_W, FLAG_H);
                if !st.flag_dc.is_null() {
                    DeleteDC(st.flag_dc);
                }
                if !st.flag_bmp.is_null() {
                    DeleteObject(st.flag_bmp);
                }
                let screen = GetDC(std::ptr::null_mut());
                let mdc = CreateCompatibleDC(screen);
                ReleaseDC(std::ptr::null_mut(), screen);
                SelectObject(mdc, hbm);
                st.flag_dc = mdc;
                st.flag_bmp = hbm;
                st.flag_w = FLAG_W;
                st.flag_h = FLAG_H;
                st.last_caret_layout = langid;
            }
            let hwnd = st.caret_hwnd;
            let (w, h) = (st.flag_w, st.flag_h);
            drop(st);
            SetWindowPos(hwnd, HWND_TOPMOST as HWND, cx + DX, cy + DY, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            InvalidateRect(hwnd, std::ptr::null(), 1);
        }),
    }
  }
}

unsafe extern "system" fn cursor_timer(_h: HWND, _m: u32, _id: usize, _t: u32) {
  unsafe {
    if is_fullscreen() || !input_desktop_ok() {
        return;
    }
    let kind = match a_cursor() {
        Some(k) => k,
        None => return,
    };
    // Arrow flag is skipped over console windows (matches the original).
    if kind == CursorKind::Arrow && foreground_class() == "ConsoleWindowClass" {
        return;
    }
    let langid = current_layout();
    if langid == 0 {
        return;
    }
    let dark = matches!(kind, CursorKind::IBeam) && cursor_bg_dark();
    let dpi = cursor_dpi();
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if st.cursor_kind == Some(kind)
            && st.cursor_layout == langid
            && st.cursor_dark == dark
            && st.cursor_dpi == dpi
            && st.cursor_time.elapsed().as_millis() < 300
        {
            return;
        }
        let cur = match kind {
            CursorKind::IBeam => st.ibeam_cur,
            CursorKind::Arrow => st.arrow_cur,
        };
        let cur = match cur {
            Some(c) => c,
            None => return,
        };
        let flag = flag_src(&mut st, langid);
        let hicon = build_flagged_cursor(&cur, flag, kind, dark, dpi);
        if hicon != 0 {
            let id = match kind {
                CursorKind::IBeam => OCR_IBEAM,
                CursorKind::Arrow => OCR_NORMAL,
            };
            SetSystemCursor(hicon as HWND, id);
            st.cursor_kind = Some(kind);
            st.cursor_layout = langid;
            st.cursor_dark = dark;
            st.cursor_dpi = dpi;
            st.cursor_time = Instant::now();
        }
    });
  }
}

fn main() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );

        let mut token: usize = 0;
        let mut input: GdiplusStartupInput = std::mem::zeroed();
        input.GdiplusVersion = 1;
        GdiplusStartup(&mut token, &input, std::ptr::null_mut());

        // Snapshot the user's real cursors now, before we start replacing them.
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.arrow_cur = capture_cursor(IDC_ARROW);
            st.ibeam_cur = capture_cursor(IDC_IBEAM);
        });

        let cls = wide("FlagOnCaretWnd");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(caret_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: cls.as_ptr(),
        };
        RegisterClassW(&wc);
        let caret_hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            cls.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            FLAG_W,
            FLAG_H,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        SetLayeredWindowAttributes(caret_hwnd, KEY_COLORREF, 0, LWA_COLORKEY);
        STATE.with(|s| s.borrow_mut().caret_hwnd = caret_hwnd);

        let _tray = TrayIconBuilder::new()
            .sender(|e: &TrayEvent| match e {
                TrayEvent::Exit => PostQuitMessage(0),
            })
            .icon_from_buffer(include_bytes!("../assets/App.ico"))
            .tooltip("FlagOnCaret")
            .menu(MenuBuilder::new().item("Выход", TrayEvent::Exit))
            .build()
            .expect("tray build");

        SetTimer(std::ptr::null_mut(), 1, 40, Some(caret_timer));
        SetTimer(std::ptr::null_mut(), 2, 100, Some(cursor_timer));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        restore_cursors();
        windows_sys::Win32::Graphics::GdiPlus::GdiplusShutdown(token);
    }
}
