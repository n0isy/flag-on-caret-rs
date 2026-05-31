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
    GdipAddPathArcI, GdipClosePathFigure, GdipCreateBitmapFromFile, GdipCreateBitmapFromScan0,
    GdipCreateFont, GdipCreateFontFamilyFromName, GdipCreateHBITMAPFromBitmap,
    GdipCreateHICONFromBitmap, GdipCreateImageAttributes, GdipCreateLineBrushFromRectI,
    GdipCreatePath, GdipCreateSolidFill, GdipCreateStringFormat, GdipDeleteBrush, GdipDeleteFont,
    GdipDeleteFontFamily, GdipDeleteGraphics, GdipDeletePath, GdipDeleteStringFormat,
    GdipDisposeImage, GdipDisposeImageAttributes, GdipDrawImageRectI, GdipDrawImageRectRectI,
    GdipDrawString, GdipFillPath, GdipGetImageGraphicsContext, GdipGraphicsClear,
    GdipSetImageAttributesColorMatrix, GdipSetInterpolationMode, GdipSetSmoothingMode,
    GdipSetStringFormatAlign, GdipSetStringFormatLineAlign, GdiplusStartup, GdiplusStartupInput,
    ColorMatrix, GpBitmap, GpGraphics, Rect as GpRect, RectF,
};
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop};
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetCursorInfo, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowThreadProcessId, LoadCursorW,
    PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes, SetSystemCursor, SetTimer,
    SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage, CURSORINFO, IDC_ARROW,
    IDC_IBEAM, MSG, WNDCLASSW,
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
const CURSOR_SIZE: i32 = 32;
const CFLAG_W: i32 = 18;
const CFLAG_H: i32 = 12;
const CFLAG_X: i32 = 4;
const CFLAG_Y: i32 = 22;
const INVERT_THRESHOLD: f64 = 100.0;
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

struct State {
    caret_hwnd: HWND,
    flag_dc: HDC,
    flag_bmp: HBITMAP,
    flag_w: i32,
    flag_h: i32,
    last_caret_layout: u32,
    src_cache: HashMap<u32, *mut GpBitmap>,
    ibeam_draft: *mut GpBitmap,
    arrow_draft: *mut GpBitmap,
    cursor_kind: Option<CursorKind>,
    cursor_layout: u32,
    cursor_dark: bool,
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
            ibeam_draft: std::ptr::null_mut(),
            arrow_draft: std::ptr::null_mut(),
            cursor_kind: None,
            cursor_layout: 0,
            cursor_dark: false,
            cursor_time: Instant::now(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::new());
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// LANGID (low word of the focused thread's HKL).
fn current_layout() -> u32 {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return 0;
        }
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let hkl = GetKeyboardLayout(tid);
        (hkl as usize as u32) & 0xFFFF
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
        let path = exe_dir().join("flags").join(format!("{loc}.png"));
        let wpath = wide(&path.to_string_lossy());
        unsafe {
            GdipCreateBitmapFromFile(wpath.as_ptr(), &mut bmp);
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

fn cursor_draft(path: &str) -> *mut GpBitmap {
    let p = exe_dir().join("cursors").join(path);
    let wp = wide(&p.to_string_lossy());
    let mut bmp: *mut GpBitmap = std::ptr::null_mut();
    unsafe {
        GdipCreateBitmapFromFile(wp.as_ptr(), &mut bmp);
    }
    bmp
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

/// Compose a flagged cursor HICON (96x96, hotspot = center).
fn build_cursor_hicon(draft: *mut GpBitmap, flag: *mut GpBitmap, kind: CursorKind, dark: bool) -> isize {
    let cs = CURSOR_SIZE;
    let fx = (cs as f32 * (1.5 + CFLAG_X as f32 / 32.0)) as i32;
    let fy = (cs as f32 * (1.5 + CFLAG_Y as f32 / 32.0)) as i32;
    let fw = cs * CFLAG_W / 32;
    let fh = cs * CFLAG_H / 32;
    unsafe {
        let mut canvas: *mut GpBitmap = std::ptr::null_mut();
        GdipCreateBitmapFromScan0(cs * 3, cs * 3, 0, PIXELFORMAT_32BPP_ARGB, std::ptr::null(), &mut canvas);
        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipGetImageGraphicsContext(canvas as *mut _, &mut g) != 0 {
            return 0;
        }
        GdipSetSmoothingMode(g, 2);
        GdipSetInterpolationMode(g, 7);
        match kind {
            CursorKind::IBeam => {
                draw_draft(g, draft, cs, cs, cs, cs, dark);
                GdipDrawImageRectI(g, flag as *mut _, fx, fy, fw, fh);
            }
            CursorKind::Arrow => {
                GdipDrawImageRectI(g, flag as *mut _, fx, fy, fw, fh);
                let off = (cs as f32 * 1.5) as i32;
                draw_draft(g, draft, off, off, cs, cs, false);
            }
        }
        GdipDeleteGraphics(g);
        let mut hicon: isize = 0;
        GdipCreateHICONFromBitmap(canvas, &mut hicon as *mut isize as *mut _);
        GdipDisposeImage(canvas as *mut _);
        hicon
    }
}

/// Draw the cursor draft, optionally colour-inverted (dark backgrounds).
unsafe fn draw_draft(g: *mut GpGraphics, img: *mut GpBitmap, x: i32, y: i32, w: i32, h: i32, dark: bool) {
    unsafe {
        if !dark {
            GdipDrawImageRectI(g, img as *mut _, x, y, w, h);
            return;
        }
        let mut attr = std::ptr::null_mut();
        GdipCreateImageAttributes(&mut attr);
        let m = invert_matrix();
        GdipSetImageAttributesColorMatrix(attr, 0, 1, &m, std::ptr::null(), 0);
        GdipDrawImageRectRectI(
            g, img as *mut _, x, y, w, h, 0, 0, w, h, UNIT_PIXEL, attr, 0, std::ptr::null_mut(),
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
            return String::new();
        }
        let mut buf = [0u16; 256];
        let n = windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW(fg, buf.as_mut_ptr(), 256);
        if n <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buf[..n as usize])
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
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if st.cursor_kind == Some(kind)
            && st.cursor_layout == langid
            && st.cursor_dark == dark
            && st.cursor_time.elapsed().as_millis() < 300
        {
            return;
        }
        if st.ibeam_draft.is_null() {
            st.ibeam_draft = cursor_draft("cursor.png");
        }
        if st.arrow_draft.is_null() {
            st.arrow_draft = cursor_draft("arrow.png");
        }
        let draft = match kind {
            CursorKind::IBeam => st.ibeam_draft,
            CursorKind::Arrow => st.arrow_draft,
        };
        if draft.is_null() {
            return;
        }
        let flag = flag_src(&mut st, langid);
        let hicon = build_cursor_hicon(draft, flag, kind, dark);
        if hicon != 0 {
            let id = match kind {
                CursorKind::IBeam => OCR_IBEAM,
                CursorKind::Arrow => OCR_NORMAL,
            };
            SetSystemCursor(hicon as HWND, id);
            st.cursor_kind = Some(kind);
            st.cursor_layout = langid;
            st.cursor_dark = dark;
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
