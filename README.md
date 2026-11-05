<p align="center">
  <img src="assets/logo.svg" width="112" alt="RapidShot logo">
</p>

<h1 align="center">RapidShot</h1>

<p align="center">
  A fast screenshot tool for Linux: capture, annotate, pin, read text and pick colors.<br>
  Built in Rust with egui, made for GNOME on Wayland.
</p>

<p align="center">
  <img src="docs/screenshots/editor.png" alt="RapidShot editor">
</p>

## Features

- **Capture**: area, full screen, a single window, a delayed capture (3 s) or a repeat of the
  last area. The area selector has a magnifier, resize handles, arrow-key nudging and works
  across multiple monitors.
- **Floating thumbnail** after each capture: copy, save, edit, pin or drag it away. You choose
  the monitor, the corner and how long it stays.
- **Editor**: arrow, line, text, rectangle, oval, freehand draw, highlighter, blur / erase,
  spotlight, numbered counter, crop and resize. Shapes stay editable (select, move, restyle,
  undo / redo), and each tool remembers its last color, width and style.
- **Image info** in the toolbar: color under the cursor (copy it with <kbd>Tab</kbd> in HEX,
  RGB, HSL and more), image size and zoom. With an area selected: autoadjust, size label and the
  WCAG contrast between its two main colors.
- **Pin** any capture as an always-on-top window, with opacity and zoom.
- **Read text and QR codes** from an area (OCR with Tesseract, QR with ZBar) and copy them.
- **Color picker** and a **color palette library**: save, reorder, generate shades and
  import / export Adobe Swatch Exchange (`.ase`) and GIMP (`.gpl`) files.
- **Global shortcuts** registered in GNOME from the preferences window.
- **Tray menu** with every action, a single resident instance and a CLI.
- Interface in English, Spanish, Portuguese, French, German, Russian, Chinese and Japanese.

## Screenshots

| Color palettes | Preferences |
| --- | --- |
| ![Color palettes](docs/screenshots/palettes.png) | ![General preferences](docs/screenshots/preferences-general.png) |
| ![Global shortcuts](docs/screenshots/preferences-shortcuts.png) | ![Advanced preferences](docs/screenshots/preferences-advanced.png) |

## Requirements

- GNOME 45 or newer (developed and tested on Wayland). Captures go through the XDG desktop portal.
- [Rust](https://rustup.rs) (stable) and [just](https://github.com/casey/just) to build.
- Optional tools, used when present:

| Tool | Used for |
| --- | --- |
| `wl-clipboard` | Copy and paste images on Wayland |
| `tesseract` (+ language data) | Read text |
| `zbar` (`zbarimg`) | Read QR and bar codes |
| `zenity` | "Open File" dialog |
| `libnotify` (`notify-send`) | Notifications |

## Install

Install or update to the latest release (x86_64, no root needed):

```bash
curl -fsSL https://raw.githubusercontent.com/wfpaisa/rapidshot/main/install.sh | bash
```

It installs:

- `~/.local/bin/rapidshot`
- the launcher and its icon (`~/.local/share/applications/rapidshot.desktop`)
- the interface icons (`~/.local/share/rapidshot/icons`)
- the GNOME Shell extension `rapidshot@wfelipe.com`

Log out and back in once so GNOME loads the extension. It is needed for **Capture Window**,
for drawing the selector over the top bar and for capturing only the chosen area.

Pick a version with `bash -s -- --version v0.1.0`, or remove everything with
`bash -s -- --uninstall` (your settings in `~/.config/rapidshot` are kept). You can also
download the archive from [Releases](https://github.com/wfpaisa/rapidshot/releases), extract it
and run `./install.sh`.

### From source

```bash
git clone https://github.com/wfpaisa/rapidshot.git
cd rapidshot
just install
```

## Usage

Start the resident instance (it adds the tray icon):

```bash
rapidshot
```

Then use the tray menu, the global shortcuts (add them in **Preferences → Shortcuts**) or the
CLI:

```text
rapidshot [area | full | window | ocr | color | palettes | repeat | delayed |
           clipboard | open | prefs | edit <file> | pin <file> | quit | --version]
```

| Default shortcut | Action |
| --- | --- |
| <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>F</kbd> | Capture screen |
| <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>A</kbd> | Capture area |
| <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>R</kbd> | Repeat area capture |
| <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>D</kbd> | Capture in 3 seconds |
| <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>V</kbd> | Open from clipboard |
| <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>T</kbd> | Read text / QR |
| <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>W</kbd> | Capture window |
| <kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>C</kbd> | Color picker |

Settings live in `~/.config/rapidshot/`.

## Development

```bash
just dev area   # run a debug build with any subcommand
just test       # cargo test
just lint       # rustfmt + clippy
just release    # portable build in Docker, packaged in dist/
```

The workspace is split into `crates/core` (capture, settings, image, clipboard, OCR),
`crates/ui` (egui windows: selector, editor, thumbnail, pin, preferences, palettes, tray) and
`apps/rapidshot` (binary, single instance and CLI). The GNOME extension is in `extension/`.

The interface icons are SVGs in `assets/icons/`; after editing one, run `just icons` and
restart the app, no rebuild needed. The logo (`assets/logo.svg`) is embedded in the binary, so
it needs `just install`.


## Relelease

First commit and push to github all.
```sh
./release.sh 1.0.1
``` 

## License

[MIT](LICENSE)
