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

## Status

| Part | State |
|------|-------|
| Tray icon + **Exit** menu | ✅ (`trayicon`) |
| Active-window keyboard layout | ✅ (`GetKeyboardLayout`) |
| Caret flag — classic Win32 controls | ✅ (`GetGUIThreadInfo` + GDI+ color-key layered window) |
| Cursor flag — I-beam & arrow | ✅ (`SetSystemCursor`, restored via `SPI_SETCURSORS` on exit) |
| Flag PNG per locale + placeholder | ✅ (GDI+ `GdipCreateBitmapFromFile`) |
| Caret flag — **UWP / Chromium** (UIA + MSAA) | ⏳ TODO — the hard part (see `caret_pos`) |
| I-beam brightness inversion / text-flag fallback | ⏳ TODO |

> The program replaces the **system** I-beam/arrow cursors while running (same as
> the original) and restores them on a clean exit. If killed, run
> **Control Panel → Mouse → OK** to restore.

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

Ship `FlagOnCaret.exe` together with the `flags/` and `cursors/` folders (the
program loads them from its own directory at runtime). Releases bundle them as
`FlagOnCaret_portable.zip`.

## Dependencies (freshest)

| Crate | Version | Why |
|-------|---------|-----|
| [`windows-sys`](https://crates.io/crates/windows-sys) | 0.61 | raw Win32 + GDI+ FFI |
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
