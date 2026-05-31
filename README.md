# flag-on-caret-rs

[![build-and-release](https://github.com/n0isy/flag-on-caret-rs/actions/workflows/release.yml/badge.svg)](https://github.com/n0isy/flag-on-caret-rs/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/n0isy/flag-on-caret-rs?sort=semver)](https://github.com/n0isy/flag-on-caret-rs/releases/latest)
[![License: LGPL v3](https://img.shields.io/badge/License-LGPL_v3-blue.svg)](LICENSE)

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
| Caret flag — **Chromium** browsers | ✅ MSAA `OBJID_CARET` + `IAccessible::accLocation` |
| Caret flag — **UWP / modern Notepad** | ✅ UI Automation `TextPattern2.GetCaretRange` |
| Cursor flag on the **user's own cursors** | ✅ real arrow/I-beam captured at startup (`GdipCreateBitmapFromHICON` + hotspot), flag overlaid, `SetSystemCursor` (restored via `SPI_SETCURSORS` on exit) |
| **Cursor size scales with monitor DPI** | ✅ `GetDpiForWindow` (matches the native cursor on HiDPI) |
| **I-beam colour inversion** on dark backgrounds | ✅ `GetPixel` sampling + GDI+ invert colour matrix |
| **Console layout** (Win+Space in conhost) | ✅ `AttachConsole` + `GetConsoleKeyboardLayoutNameW` |
| Flag PNG per locale | ✅ full LangBarXX `LangCode` table (287 entries) |
| **Text flag fallback** (no PNG) | ✅ GDI+ gradient rounded-rect + 2-letter code |
| Guards: full screen, **#32768 menu**, **console window**, **secure desktop** | ✅ |
| **Per-monitor-v2 DPI awareness** | ✅ `SetProcessDpiAwarenessContext` |

The caret detection (`src/caret.rs`) is a faithful port of LangBarXX's
`GetCaretLocation.ahk`: it dispatches by window class to UIA → MSAA →
`GetGUIThreadInfo` with the same fall-through.

> The program replaces the **system** I-beam/arrow cursors while running (same as
> the original) and restores them on a clean exit. If killed, run
> **Control Panel → Mouse → OK** to restore.
>
> ⚠️ Built and statically checked (clippy `-D warnings`, native MSVC + gnu
> cross). Not yet smoke-tested on real hardware — runtime verification on
> Windows is the remaining step.

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

---

## По-русски

Нативная Windows-утилита (~300 КБ, без рантайма): флажок текущей раскладки у
каретки и на курсоре мыши. Это **порт на Rust** проекта
[`n0isy/flag-on-caret`](https://github.com/n0isy/flag-on-caret) (который, в свою
очередь, — вырезанная одна функция из [LangBarXX](https://github.com/Krot66/LangBarXX),
автор Krot66). Поведение, картинки флажков/курсоров и значения по умолчанию взяты
оттуда — сменился только язык реализации. Каретка определяется через
`GetGUIThreadInfo` (классический Win32); UIA/MSAA для UWP/Chromium — в TODO.
