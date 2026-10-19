# Laser Pointer

A screen-wide laser pointer and annotation tool for presenting on Windows. Press a shortcut and
draw over anything — slides, browsers, code — with a smooth, fading laser trail. Right-clicks and
the scroll wheel still reach the app underneath, so you never have to switch it off to drive your
presentation.

[Try the interactive demo](docs/index.html) — it runs the app's actual stroke code in the browser.

## Features

- **Smooth laser trail** that tapers by distance and fades over time, drawn as one anti-aliased
  shape per stroke. Solid, Glow and Neon styles; any colour; adjustable thickness, opacity, fade
  and trail length.
- **Presets** — Laser, Highlighter and Pen to start with; create, rename, update and delete your
  own (up to eight).
- **Keep strokes** — annotations stay on screen, even with the laser off and clicks passing
  through them, until you clear them. Undo the last one at any time.
- **Shortcuts for everything** — rebind or remove any action, including single keys. Keys that
  only make sense while drawing (Esc, Undo) are claimed only while the laser is on.
- **Laser-dot cursor**, **click ripples** where right-clicks land, and an **on-screen indicator**
  that confirms toggles, preset changes and size changes.
- **Settings panel** in the Windows 11 style, following your light/dark theme and accent colour,
  with a live preview. Left-click the tray icon to open it.
- Light on resources: the overlay is hidden while there is nothing to show, and only the parts of
  the screen that change are redrawn.

## Default shortcuts

| Action | Shortcut | Active |
| --- | --- | --- |
| Turn laser on or off | Alt+L | always |
| Turn laser off | Esc | while the laser is on |
| Undo last stroke | Ctrl+Z | while the laser is on |
| Clear strokes | Alt+X | while strokes are on screen |
| Keep strokes on or off | Alt+K | always |
| Next / previous preset | Alt+] / Alt+[ | always |
| Quit | Alt+Shift+L | always |

While the laser is on: drag to draw, right-click to click through, scroll to scroll the app
underneath, Ctrl+scroll to change thickness.

## Running it

Build it (below) and run `target\release\laser-pointer.exe`; it sits in the notification area.
Only one copy runs at a time; starting it again opens the settings of the running copy.

Settings are stored in `%APPDATA%\LaserPointer\settings.ini`.

## Build from source

Requires Windows 10 1809 or later and a Rust toolchain.

```powershell
cargo build --release
```

The default MSVC toolchain needs the Visual Studio Build Tools. With the GNU toolchain
(`x86_64-pc-windows-gnu`), put a MinGW-w64 `bin` folder (for `dlltool` and `as`) on `PATH`.

## Licence

[MIT](LICENSE). The cursor artwork comes from [Excalidraw](https://github.com/excalidraw/excalidraw)
under the MIT licence; see [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).
