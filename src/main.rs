// FlagOnCaret — Rust port.
//
// Shows the current keyboard-layout flag (1) at the text caret and (2) overlaid
// on the mouse cursor (I-beam + arrow). Rust port of the single feature
// extracted from LangBarXX (Krot66); see the AHK original for lineage.
//
// Design: everything runs on the main thread. trayicon owns its window; we run
// the Win32 message loop and drive two thread-timers (SetTimer with NULL hwnd +
// a TIMERPROC) for the caret (40 ms) and the cursor (100 ms). On "Exit" the tray
// sender calls PostQuitMessage; on shutdown we restore the system cursors.
//
// NOTE: caret detection here uses GetGUIThreadInfo (covers classic Win32 edit
// controls). The UIA/MSAA fallbacks for UWP/Chromium (the hard part discussed in
// the AHK version) are a documented TODO — see `caret_pos`.

#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use trayicon::{MenuBuilder, TrayIconBuilder};

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Globalization::LCIDToLocaleName;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, GetDC,
    InvalidateRect, ReleaseDC, SelectObject, HBITMAP, HDC, PAINTSTRUCT, SRCCOPY,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipCreateBitmapFromFile, GdipCreateBitmapFromScan0, GdipCreateHBITMAPFromBitmap,
    GdipCreateHICONFromBitmap, GdipDeleteGraphics, GdipDisposeImage, GdipDrawImageRectI,
    GdipGetImageGraphicsContext, GdipGraphicsClear, GdipSetInterpolationMode, GdipSetSmoothingMode,
    GdiplusStartup, GdiplusStartupInput, GpBitmap, GpGraphics,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorInfo, GetForegroundWindow,
    GetGUIThreadInfo, GetMessageW, GetSystemMetrics, GetWindowThreadProcessId, LoadCursorW,
    PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes, SetSystemCursor, SetTimer,
    SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage, CURSORINFO, GUITHREADINFO,
    IDC_ARROW, IDC_IBEAM, MSG, WNDCLASSW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;

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
// Color-key background (matches AHK TransColor 3A3B3C).
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
    flag_dc: HDC,           // memory DC holding the scaled flag bitmap
    flag_bmp: HBITMAP,
    flag_w: i32,
    flag_h: i32,
    last_caret_layout: u32, // langid currently rendered into the caret flag
    src_cache: HashMap<u32, *mut GpBitmap>, // source flag bitmaps by langid
    ibeam_draft: *mut GpBitmap,
    arrow_draft: *mut GpBitmap,
    cursor_kind: Option<CursorKind>,
    cursor_layout: u32,
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

/// HKL of the focused window's thread; returns the LANGID (low word).
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

/// "ru-RU" etc. from a LANGID.
fn locale_name(langid: u32) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 85];
        let n = LCIDToLocaleName(langid, buf.as_mut_ptr(), buf.len() as i32, 0);
        if n <= 1 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..(n as usize - 1)]))
    }
}

/// Screen caret position via GetGUIThreadInfo (classic Win32 path).
/// TODO: UIA TextPattern2 + MSAA OBJID_CARET fallbacks for UWP/Chromium.
fn caret_pos() -> Option<(i32, i32)> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return None;
        }
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let mut gti: GUITHREADINFO = std::mem::zeroed();
        gti.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(tid, &mut gti) == 0 || gti.hwndCaret.is_null() {
            return None;
        }
        let mut pt = POINT {
            x: gti.rcCaret.left,
            y: gti.rcCaret.bottom,
        };
        // rcCaret is client-relative to hwndCaret.
        windows_sys::Win32::Graphics::Gdi::ClientToScreen(gti.hwndCaret, &mut pt);
        if pt.x == 0 && pt.y == 0 {
            None
        } else {
            Some((pt.x, pt.y))
        }
    }
}

/// Source flag bitmap for a langid, cached. Falls back to a solid placeholder.
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
        // Placeholder: a solid 64x48 steel-blue bitmap.
        unsafe {
            GdipCreateBitmapFromScan0(64, 48, 0, PIXELFORMAT_32BPP_ARGB, std::ptr::null(), &mut bmp);
            let mut g: *mut GpGraphics = std::ptr::null_mut();
            if GdipGetImageGraphicsContext(bmp as *mut _, &mut g) == 0 {
                GdipGraphicsClear(g, 0xFF33_4B63);
                GdipDeleteGraphics(g);
            }
        }
    }
    st.src_cache.insert(langid, bmp);
    bmp
}

/// Build a scaled HBITMAP (FLAG_W x FLAG_H) from a source bitmap, with the
/// color-key background, for the color-keyed caret window.
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

/// Load a cursor draft PNG (cursor.png / arrow.png), cached on first use.
fn cursor_draft(path: &str) -> *mut GpBitmap {
    let p = exe_dir().join("cursors").join(path);
    let wp = wide(&p.to_string_lossy());
    let mut bmp: *mut GpBitmap = std::ptr::null_mut();
    unsafe {
        GdipCreateBitmapFromFile(wp.as_ptr(), &mut bmp);
    }
    bmp
}

/// Compose a flagged cursor HICON (96x96, hotspot = center).
fn build_cursor_hicon(draft: *mut GpBitmap, flag: *mut GpBitmap, kind: CursorKind) -> isize {
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
                GdipDrawImageRectI(g, draft as *mut _, cs, cs, cs, cs);
                GdipDrawImageRectI(g, flag as *mut _, fx, fy, fw, fh);
            }
            CursorKind::Arrow => {
                GdipDrawImageRectI(g, flag as *mut _, fx, fy, fw, fh);
                let off = (cs as f32 * 1.5) as i32;
                GdipDrawImageRectI(g, draft as *mut _, off, off, cs, cs);
            }
        }
        GdipDeleteGraphics(g);
        let mut hicon: isize = 0;
        GdipCreateHICONFromBitmap(canvas, &mut hicon as *mut isize as *mut _);
        GdipDisposeImage(canvas as *mut _);
        hicon
    }
}

/// Current global cursor type (after our replacement, LoadCursorW still maps).
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

// ---- Caret window proc: paints the cached flag with color-key transparency ----
unsafe extern "system" fn caret_wndproc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
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
    if langid == 0 || is_fullscreen() {
        STATE.with(|s| ShowWindow(s.borrow().caret_hwnd, SW_HIDE));
        return;
    }
    match caret_pos() {
        None => {
            STATE.with(|s| ShowWindow(s.borrow().caret_hwnd, SW_HIDE));
        }
        Some((cx, cy)) => STATE.with(|s| {
            let mut st = s.borrow_mut();
            if st.last_caret_layout != langid || st.flag_dc.is_null() {
                let src = flag_src(&mut st, langid);
                let hbm = scaled_flag_hbitmap(src, FLAG_W, FLAG_H);
                // rebuild memory DC
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
            SetWindowPos(
                hwnd,
                HWND_TOPMOST as HWND,
                cx + DX,
                cy + DY,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(hwnd, std::ptr::null(), 1);
        }),
    }
  }
}

unsafe extern "system" fn cursor_timer(_h: HWND, _m: u32, _id: usize, _t: u32) {
  unsafe {
    if is_fullscreen() {
        return;
    }
    let kind = match a_cursor() {
        Some(k) => k,
        None => return,
    };
    let langid = current_layout();
    if langid == 0 {
        return;
    }
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        // throttle: skip if same kind+layout within 300 ms
        if st.cursor_kind == Some(kind)
            && st.cursor_layout == langid
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
        let hicon = build_cursor_hicon(draft, flag, kind);
        if hicon != 0 {
            let id = match kind {
                CursorKind::IBeam => OCR_IBEAM,
                CursorKind::Arrow => OCR_NORMAL,
            };
            SetSystemCursor(hicon as HWND, id);
            st.cursor_kind = Some(kind);
            st.cursor_layout = langid;
            st.cursor_time = Instant::now();
        }
    });
  }
}

fn main() {
    unsafe {
        // GDI+
        let mut token: usize = 0;
        let mut input: GdiplusStartupInput = std::mem::zeroed();
        input.GdiplusVersion = 1;
        GdiplusStartup(&mut token, &input, std::ptr::null_mut());

        // Register + create the color-keyed caret window.
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

        // Tray icon (Exit only). on_right_click=None -> menu auto-shows.
        let _tray = TrayIconBuilder::new()
            .sender(|e: &TrayEvent| match e {
                TrayEvent::Exit => PostQuitMessage(0),
            })
            .icon_from_buffer(include_bytes!("../assets/App.ico"))
            .tooltip("FlagOnCaret")
            .menu(MenuBuilder::new().item("Выход", TrayEvent::Exit))
            .build()
            .expect("tray build");

        // Thread-timers (NULL hwnd + TIMERPROC).
        SetTimer(std::ptr::null_mut(), 1, 40, Some(caret_timer));
        SetTimer(std::ptr::null_mut(), 2, 100, Some(cursor_timer));

        // Message loop.
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        restore_cursors();
        windows_sys::Win32::Graphics::GdiPlus::GdiplusShutdown(token);
    }
}
