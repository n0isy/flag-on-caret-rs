# flag-on-caret-rs

[![build-and-release](https://github.com/n0isy/flag-on-caret-rs/actions/workflows/release.yml/badge.svg)](https://github.com/n0isy/flag-on-caret-rs/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/n0isy/flag-on-caret-rs?sort=semver)](https://github.com/n0isy/flag-on-caret-rs/releases/latest)
[![License: LGPL v3](https://img.shields.io/badge/License-LGPL_v3-blue.svg)](LICENSE)

**🇷🇺 [Документация на русском — README.RU.md](README.RU.md)**

A tiny native-Windows tray utility that shows the **current keyboard-layout flag**:

1. **next to the text caret**, and
2. **on the mouse cursor** — the text I-beam and the arrow pointer get a small
   layout flag overlaid on them.

No settings. The whole release binary is **~300 KB** with no runtime dependency.

This is the **Rust rewrite** of [`n0isy/flag-on-caret`](https://github.com/n0isy/flag-on-caret)
(an AutoHotkey extraction of one feature from
[**LangBarXX**](https://github.com/Krot66/LangBarXX) by **Krot66**). The behaviour,
the flag/cursor image assets and the default parameters all come from there; only
the implementation language changed. See [CREDITS.md](CREDITS.md).

---

## Status — full feature parity with the AHK original

| Part | State |
|------|-------|
| Tray icon + **Exit** menu | ✅ (`trayicon`) |
| Active-window keyboard layout | ✅ (`GetKeyboardLayout`) |
| Caret flag — classic Win32 controls | ✅ (`GetGUIThreadInfo`) |
| Caret flag — **Chromium** browsers | ✅ MSAA `OBJID_CARET` + `accLocation`; hides on blur via `accState` |
| Caret flag — **UWP / modern Notepad** | ✅ UIA `TextPattern2.GetCaretRange`; hides on blur via `isActive` |
| Cursor flag on the **user's own cursors** | ✅ real arrow/I-beam captured at startup (`DrawIconEx` on black+white → true alpha) + hotspot, flag overlaid, `SetSystemCursor` (restored via `SPI_SETCURSORS` on exit) |
| **I-beam contrast** (white/black by background) | ✅ `GetPixel` sampling + GDI+ invert matrix (a static cursor can't XOR per-pixel like Windows) |
| **Console layout** (Win+Space in conhost) | ✅ `AttachConsole` + `GetConsoleKeyboardLayoutNameW`, cached per-window; non-conhost terminals (far2l) are detected and the flag is suppressed (see below) |
| Flag PNG per locale + **text fallback** | ✅ full LangBarXX `LangCode` table (287) + GDI+ gradient text flag |
| Guards: full screen, **#32768 menu**, **secure desktop** | ✅ |
| **Single instance**, per-monitor-v2 DPI, restore cursors before capture | ✅ |

The caret detection (`src/caret.rs`) is a faithful port of LangBarXX's
`GetCaretLocation.ahk`: it dispatches by window class to UIA → MSAA →
`GetGUIThreadInfo` with the same fall-through.

### Known limitations
- **far2l** (and other non-conhost terminals): the layout **can't be read from
  outside the process** — far2l switches via its own thread's TSF and never
  touches the legacy HKL, the console layout name, or any cross-thread-readable
  API (we tested every one, including the WinRT `CurrentInputMethodLanguageTag`,
  which is documented to only work on the focused thread). Rather than paint a
  *wrong* flag, the app **detects this case and shows no flag** there — a console
  window whose `GetConsoleKeyboardLayoutNameW` won't answer is treated as
  untrustworthy and both the caret and cursor flags are hidden over it.
  conhost-based terminals (cmd, mingw) and mintty work normally.
- The cursor flag is composited into a **static** system cursor, so the I-beam
  picks one contrast colour from the background rather than inverting per-pixel.

> The program replaces the **system** I-beam/arrow cursors while running and
> restores them on a clean exit (and resets them before capturing, so a crashed
> run can't poison the next one). If hard-killed, run **Control Panel → Mouse →
> OK** to restore.

## Why Rust here

We measured the trade-off against the AHK version (see the discussion in the
sibling repo): the genuinely hard part — caret detection across app types — is
identical work in any language, but a native build drops the AHK runtime and the
binary to ~300 KB. `trayicon` covers the "simplest Windows tray" need with a
pure-Win32 path; everything else is `windows-sys`.

## Build

Native (recommended), on Windows with the Rust toolchain:

```bash
cargo build --release      # -> target/release/FlagOnCaret.exe
```

Cross-compile from Linux (what CI-equivalent local checks use):

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt-get install -y gcc-mingw-w64-x86-64
cargo build --release --target x86_64-pc-windows-gnu
```

`FlagOnCaret.exe` is **self-contained** — the flag PNGs and cursor drafts are
baked into the binary with `include_bytes!` and decoded from memory via GDI+
(`SHCreateMemStream` + `GdipCreateBitmapFromStream`), so there are no external
files to ship.

Each release provides two downloads:

| File | What it is |
|------|------------|
| `FlagOnCaret_setup.exe` | Inno Setup installer (shortcuts, optional autostart, uninstall). |
| `FlagOnCaret_portable.zip` | Just the standalone `FlagOnCaret.exe`. |

The installer is built from [`installer/FlagOnCaret.iss`](installer/FlagOnCaret.iss)
with Inno Setup 6 (`ISCC`); CI builds it on every tagged release.

## Dependencies (freshest)

| Crate | Version | Why |
|-------|---------|-----|
| [`windows-sys`](https://crates.io/crates/windows-sys) | 0.61 | raw Win32 + GDI+ FFI (window, cursor, GDI+) |
| [`windows`](https://crates.io/crates/windows) | 0.62 | typed COM for UI Automation + MSAA caret |
| [`trayicon`](https://crates.io/crates/trayicon) | 0.4 | tray icon + menu (Windows path = `winapi` only) |

Rust edition **2024**.

## License

**LGPL-3.0** — matching LangBarXX. See [CREDITS.md](CREDITS.md) for third-party
library terms.
