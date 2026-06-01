//! Caret location — faithful port of LangBarXX `Lib/GetCaretLocation.ahk`.
//!
//! Strategy dispatch by the foreground window class, with fall-through:
//!   1. UWP / new Notepad  -> UI Automation `TextPattern2.GetCaretRange`
//!   2. Chromium browsers  -> MSAA `OBJID_CARET` + `IAccessible::accLocation`
//!   3. anything else      -> `GetGUIThreadInfo` (classic Win32 caret)
//!
//! Each step only "wins" if it yields non-zero coordinates, otherwise we fall
//! through to the next — exactly like the AHK original.

use std::cell::RefCell;
use std::ffi::c_void;

use windows::core::{Interface, BOOL};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, SAFEARRAY};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetUBound, SafeArrayUnaccessData,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VT_I4};
use windows::Win32::UI::Accessibility::{
    AccessibleObjectFromWindow, CUIAutomation, IAccessible, IUIAutomation,
    IUIAutomationTextPattern2, UIA_TextPattern2Id,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    OBJID_CARET,
};

thread_local! {
    static UIA: RefCell<Option<IUIAutomation>> = const { RefCell::new(None) };
}

/// Screen position of the text caret, or `None` if it can't be determined.
pub fn caret_pos() -> Option<(i32, i32)> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let class = window_class(hwnd);

        // 1. UWP shell / modern Notepad -> UI Automation.
        if class == "ApplicationFrameWindow" || class == "Notepad" {
            if let Some(p) = uia_caret() {
                return Some(p);
            }
        }
        // 2. Chromium-family -> MSAA caret object.
        if class.contains("Chrome_WidgetWin")
            || class.contains("Maxthon")
            || class.contains("Slimjet")
            || class.contains("Vivaldi")
        {
            if let Some(p) = msaa_caret(hwnd) {
                return Some(p);
            }
        }
        // 3. Classic Win32 fallback.
        gti_caret(hwnd)
    }
}

unsafe fn window_class(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buf[..n as usize])
        }
    }
}

/// UIA: focused element -> TextPattern2 -> caret range -> bounding rectangle.
unsafe fn uia_caret() -> Option<(i32, i32)> {
    unsafe {
        UIA.with(|cell| {
            {
                let mut opt = cell.borrow_mut();
                if opt.is_none() {
                    let uia: IUIAutomation =
                        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
                    *opt = Some(uia);
                }
            }
            let opt = cell.borrow();
            let uia = opt.as_ref()?;

            let el = uia.GetFocusedElement().ok()?;
            let pat: IUIAutomationTextPattern2 =
                el.GetCurrentPatternAs(UIA_TextPattern2Id).ok()?;
            let mut active = BOOL::default();
            let range = pat.GetCaretRange(&mut active).ok()?;
            if active.0 == 0 {
                return None; // the text control no longer has keyboard focus
            }
            let psa = range.GetBoundingRectangles().ok()?;
            let rect = read_safearray_rect(psa)?;
            let (x, y) = (rect[0] as i32, rect[1] as i32);
            if x != 0 && y != 0 { Some((x, y)) } else { None }
        })
    }
}

/// Read [x, y, w, h] from a SAFEARRAY(VT_R8); frees the array.
unsafe fn read_safearray_rect(psa: *mut SAFEARRAY) -> Option<[f64; 4]> {
    unsafe {
        let ubound = SafeArrayGetUBound(psa, 1).ok()?;
        if ubound < 3 {
            let _ = SafeArrayDestroy(psa);
            return None;
        }
        let mut data: *mut c_void = std::ptr::null_mut();
        if SafeArrayAccessData(psa, &mut data).is_err() {
            let _ = SafeArrayDestroy(psa);
            return None;
        }
        let p = data as *const f64;
        let out = [*p, *p.add(1), *p.add(2), *p.add(3)];
        let _ = SafeArrayUnaccessData(psa);
        let _ = SafeArrayDestroy(psa);
        Some(out)
    }
}

/// MSAA: caret object of the window -> accLocation(CHILDID_SELF).
unsafe fn msaa_caret(hwnd: HWND) -> Option<(i32, i32)> {
    unsafe {
        let mut pacc: *mut c_void = std::ptr::null_mut();
        AccessibleObjectFromWindow(hwnd, OBJID_CARET.0 as u32, &IAccessible::IID, &mut pacc)
            .ok()?;
        if pacc.is_null() {
            return None;
        }
        let acc = IAccessible::from_raw(pacc);
        let (mut l, mut t, mut w, mut h) = (0i32, 0i32, 0i32, 0i32);
        let child = childid_self(); // CHILDID_SELF (VT_I4 = 0)
        acc.accLocation(&mut l, &mut t, &mut w, &mut h, &child).ok()?;
        // Chromium keeps returning the *last* caret rect after the field loses
        // focus, so accLocation alone never hides. Use the caret's state: if it's
        // invisible, there's no active caret -> hide. (Blink is an OS render
        // effect, not a state change, so this doesn't flicker.)
        if let Ok(state) = acc.get_accState(&child) {
            if let Some(s) = variant_i4(&state) {
                const STATE_SYSTEM_INVISIBLE: i32 = 0x8000;
                if s & STATE_SYSTEM_INVISIBLE != 0 {
                    return None;
                }
            }
        }
        if l != 0 || t != 0 { Some((l, t)) } else { None }
    }
}

/// Read an i32 out of a VT_I4 VARIANT (e.g. an MSAA state bitmask).
unsafe fn variant_i4(v: &VARIANT) -> Option<i32> {
    unsafe {
        let inner = &*v.Anonymous.Anonymous;
        if inner.vt == VT_I4 {
            Some(inner.Anonymous.lVal)
        } else {
            None
        }
    }
}

/// A `CHILDID_SELF` VARIANT (VT_I4, value 0).
unsafe fn childid_self() -> VARIANT {
    unsafe {
        let mut inner: VARIANT_0_0 = std::mem::zeroed();
        inner.vt = VT_I4;
        VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(inner),
            },
        }
    }
}

/// Classic Win32 caret via GetGUIThreadInfo + ClientToScreen.
unsafe fn gti_caret(hwnd: HWND) -> Option<(i32, i32)> {
    unsafe {
        let tid = GetWindowThreadProcessId(hwnd, None);
        let mut gti = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(tid, &mut gti).is_err() || gti.hwndCaret.0.is_null() {
            return None;
        }
        let mut pt = POINT {
            x: gti.rcCaret.left,
            y: gti.rcCaret.bottom,
        };
        let _ = ClientToScreen(gti.hwndCaret, &mut pt);
        if pt.x == 0 && pt.y == 0 {
            None
        } else {
            Some((pt.x, pt.y))
        }
    }
}
