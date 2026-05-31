# Credits & attribution

This is a **Rust rewrite**, not original design work. The feature, behaviour and
assets come from upstream; only the implementation language is new.

## Lineage

- **[LangBarXX](https://github.com/Krot66/LangBarXX)** — © **Krot66**. The full
  keyboard-layout assistant the feature was taken from.
- **[n0isy/flag-on-caret](https://github.com/n0isy/flag-on-caret)** — the AutoHotkey
  extraction of just the caret/cursor flag, which this project ports to Rust.

## What was carried over from the AHK version

- The **two mechanisms**: a color-keyed top-most layered window at the caret, and
  `SetSystemCursor`-based flagged I-beam/arrow cursors (composed with GDI+,
  hotspot at the canvas center via the 3× canvas trick).
- The **default parameters**: caret flag `22×16` at offset `(16, −12)`; cursor
  `32 px` with an `18×12` flag at offset `(4, 22)`; color-key `#3A3B3C`.
- The **image assets**: `assets/flags/*.png` and `assets/cursors/{cursor,arrow}.png`
  (the latter recovered from LangBarXX git history). These are redistributed under
  the same terms as LangBarXX.

## What is new in this repo

- The implementation in Rust over `windows-sys` + `windows` + `trayicon`.
- `src/caret.rs` — faithful port of `GetCaretLocation.ahk` (UIA `TextPattern2`
  + MSAA `OBJID_CARET` + `GetGUIThreadInfo`, same class dispatch/fall-through).
- `src/langcode.rs` — the LangBarXX `LangCode.ahk` table (287 entries),
  generated verbatim from the original.
- The Cargo build, cross-compile setup and CI/CD.

## Third-party crates

- [`windows-sys`](https://crates.io/crates/windows-sys) (MIT/Apache-2.0) — Microsoft.
- [`trayicon`](https://crates.io/crates/trayicon) (MIT) — Jari Pennanen (Ciantic).

## License

**LGPL-3.0**, matching LangBarXX (which distributes the LGPL-3.0 text as its
installer EULA; the upstream repo has no standalone `LICENSE` file).
