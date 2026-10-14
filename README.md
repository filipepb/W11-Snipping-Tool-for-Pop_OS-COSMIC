# W11 Snipping Tool for Pop!_OS COSMIC

**English** | [Português](README.pt-BR.md)

A clone of the **W11 Snipping Tool** for **Pop!_OS COSMIC**, written in Rust
directly on top of COSMIC's Wayland protocols. The program is called `recorte`
("snip" in Portuguese).

## Install

**Ready to use** (Pop!_OS 24.04 COSMIC, x86_64): download the package from the
[latest release](https://github.com/filipepb/W11-Snipping-Tool-for-Pop_OS-COSMIC/releases/latest) and run:

```bash
tar xzf recorte-*-x86_64.tar.gz && cd recorte-*-x86_64 && ./install.sh
```

**From source** (requires Rust, `libgtk-4-dev` and `libadwaita-1-dev`):

```bash
git clone https://github.com/filipepb/W11-Snipping-Tool-for-Pop_OS-COSMIC recorte
cd recorte
./install.sh                # builds, installs to ~/.local and binds Print / Super+Shift+S
./install.sh --no-shortcuts # installs without touching the shortcuts
```

The installer puts the binary in `~/.local/bin/recorte`, adds "Snipping Tool"
to the app menu (with a "New snip" action) and sets up the COSMIC keyboard
shortcuts. The previous shortcuts file is kept as
`~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom.bak-DATE`.

To create the shortcut by hand: **Settings › Keyboard › Custom shortcuts**,
command `/home/YOUR_USER/.local/bin/recorte`.

Requires COSMIC (uses `ext-image-copy-capture`, `wlr-layer-shell`,
`zcosmic-toplevel-info` and `ext-data-control`).

## Usage

| Action | How |
|---|---|
| New snip | `Print` or `Super + Shift + S` |
| Modes | Floating toolbar: rectangle, freeform, window, full screen |
| Cancel | `Esc` (while dragging, `Esc` or right click drops the selection) |
| Paste | `Ctrl + V` in any app — the image is already copied |
| Edit | Click the "Snip copied to clipboard" notification |

The screen freezes and dims as soon as the snip starts; the selected area stays
bright. The last mode picked on the toolbar is remembered. With several
monitors, the selection can span screens with different scales (the image uses
the largest one).

From a terminal: `recorte --mode rectangle|freeform|window|screen`,
`recorte --delay SECONDS`, `recorte edit [FILE]`, `recorte --help`.

### Editor

`recorte edit [FILE]` (or the app menu icon). Tools sit in the middle of the
title bar, as in W11:

- Ballpoint pen and highlighter (click the tool again to pick color and
  thickness); an eraser that removes whole strokes and "Erase all ink"
- Shapes: rectangle, ellipse, line and arrow (`Shift` snaps/squares; optional fill)
- Text (`Enter` confirms, `Shift + Enter` breaks the line)
- Image crop with handles (`Enter` applies, `Esc` cancels)
- `Ctrl+Z` / `Ctrl+Y` undo and redo, `Ctrl + mouse wheel` / `Ctrl +/−/0` zoom
- `Ctrl+S` save as PNG, `Ctrl+C` copy, `Ctrl+N` new snip (with mode and a 3,
  5 or 10 s delay), `Ctrl+O` open, or drop an image onto the window

Every edit is copied to the clipboard automatically.

### Settings (⋮ menu › Settings)

Automatically save PNGs (default: `~/Pictures/Screenshots`), dim the screen,
notification, open the editor after each snip, border on each screenshot, ask
before discarding edits and automatically copy changes. Stored in
`~/.config/recorte/recorte.conf`.

## Language

The program follows the system language: Portuguese on a Portuguese system and
English everywhere else. To force a language for one run: `LANGUAGE=en recorte`
or `LANGUAGE=pt_BR recorte`.

### Translating

Translations live in `po/` and are embedded in the binary, so nothing extra is
installed. To add a language:

1. Copy `po/pt_BR.po` to `po/<code>.po` (for example `es.po` or `fr.po`) and
   translate every `msgstr`, keeping each `{}` placeholder.
2. Add the file to `CATALOGS` in `src/i18n.rs`.
3. Run `cargo test --release`; a test checks that every text has a translation.

## How it works

| Module | Role |
|---|---|
| `snip.rs`, `wlimpl.rs` | Freezes each monitor (`ext-image-copy-capture`), draws the overlay on `layer-shell` surfaces in software (tiny-skia) and redraws only the damaged regions |
| `snip.rs` (windows) | Geometry and workspace via `zcosmic-toplevel-info`; since Wayland does not report stacking order, each visible window is captured and compared with the frozen screen to find which one is on top |
| `clip.rs` | On Wayland the clipboard lives in the process offering it: each copy starts `recorte __clip`, which serves the PNG via `ext-data-control` until another app copies something |
| `notify.rs` | Notification with an action and an activation token (opens the editor focused) |
| `editor.rs` | GTK4/libadwaita editor — inherits the COSMIC theme |
| `i18n.rs` | Translations embedded from `po/*.po` |
| `shortcuts.rs` | Merges the shortcuts into the COSMIC file without touching the others |

Nothing runs in the background: each snip is a short-lived process. Measured on
the author's computer (1920×1080): full screen to saved PNG in 0.11 s with
0.08 s of CPU and a 46 MB peak; the open overlay uses ~54 MB and almost no CPU
while idle.

## Tests

```bash
cargo test --release
```

## Known differences from Windows

- No video recording and no text actions (OCR).
- No sharing ("Share") and no printing.
- The window mode highlight appears once the mouse moves (COSMIC only reports
  the pointer position after a movement).

## License

MIT
