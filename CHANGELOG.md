# Changelog

All notable changes to this project are documented here. The format is loosely
based on [Keep a Changelog](https://keepachangelog.com/), and the project follows
[Semantic Versioning](https://semver.org/).

## [0.5.1] - 2026-06-01

### Fixed
- **No more false flag over far2l** (and other non-conhost terminals). The
  foreground layout in such a terminal can't be read from outside its process;
  the app previously fell back to the window-thread `HKL`, which is *frozen*
  there, and painted a wrong flag. Now a console window whose
  `GetConsoleKeyboardLayoutNameW` won't answer is treated as untrustworthy:
  `current_layout()` reports "unknown", the caret flag is hidden, and the flagged
  system cursor is torn down once (cursors restored) so nothing lingers over it.

### Changed
- For console windows the app trusts **only** the authoritative console-layout
  probe and never falls through to the legacy `HKL`.

### Docs
- Added a Russian README (`README.RU.md`) and linked it from the top of
  `README.md`; removed the outdated inline Russian section.
- Closed the far2l investigation: empirically verified that far2l exposes its
  layout through **no** external API — legacy `HKL`, `GetConsoleKeyboardLayoutNameW`,
  cross-thread TSF `GetCurrentLanguage`, WinRT `CurrentInputMethodLanguageTag`,
  the `AttachThreadInput` trick, and the `HSHELL_LANGUAGE` shell hook were all
  dead. Recorded as a known limitation.

## [0.5.0] - 2026-06-01

### Added
- **Real user cursors**: the actual arrow and I-beam are captured at startup with
  `DrawIconEx` over black + white backgrounds to reconstruct true straight-alpha
  BGRA, then the layout flag is composited and installed via `SetSystemCursor`.
- **Blur detection** for the caret flag: UIA `isActive` (UWP / modern Notepad)
  and MSAA `accState` invisible (Chromium) hide the flag when the text field
  loses focus.
- **Single-instance** guard via a named mutex.

### Changed
- Console-layout decision is cached per foreground window with a short TTL,
  cutting `AttachConsole` churn (far2l no longer hammered every tick).
- I-beam contrast picks white/black from background brightness.
- Caret window only moves/repaints when the caret actually moves; the cursor is
  not re-baked unless its kind/layout/inversion changes.

### Fixed
- Cursor transparency and lifetime bugs (`GdiFlush` after `DrawIconEx`; keep the
  pixel buffer alive behind the GDI+ bitmap, which does not copy scan0).

## [0.4.0] - 2026-05-31
- Composite the layout flag onto the user's **real** cursors instead of bundled
  PNG drafts; make the on-cursor flag smaller, with per-cursor offsets.

## [0.3.1] - 2026-05-31
- Fix cursor DPI sizing and console-layout detection.

## [0.3.0] - 2026-05-31
- Embed all assets into the exe (`include_bytes!`); add the Inno Setup installer
  and the tagged-release CI that builds it.

## [0.2.0] - 2026-05-31
- Full feature parity with the AHK original: UIA + MSAA + `GetGUIThreadInfo`
  caret detection, I-beam inversion, text-flag fallback, guards, DPI awareness.

## [0.1.0] - 2026-05-31
- Initial native Rust port: tray icon, active-window layout, caret + cursor flag.
