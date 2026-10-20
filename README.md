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
  through them, until you clear them. Undo and redo anything, clearing included.
- **Straight lines** — hold Alt while drawing for a perfectly horizontal line, Shift+Alt for a
  vertical one.
- **Shape snapping** — let go of a rough circle, box, triangle, line or arrow and it settles into
  a clean one. How readily it snaps is set per preset (Off, Gentle, Medium, Strong); hold Ctrl as
  you let go to keep a stroke freehand.
- **Shortcuts for everything** — rebind or remove any action, including single keys. Keys that
  only make sense while drawing (Esc, Undo) are claimed only while the laser is on.
- **Cursor styles** — a soft glow, segment ring, classic cross, cross and dot, circle and dot,
  chevron, brackets or micro dot, in the laser colour. Each moves its own way: dots swell in and
  squeeze, crosshairs spin in and twist, rings pulse, the chevron tilts.
- **Click ripples** where right-clicks land, and an **on-screen indicator** that confirms toggles,
  preset changes and size changes.
- **Settings panel** in the Windows 11 style, following your light/dark theme and accent colour,
  with a live preview. Left-click the tray icon to open it.
- Light on resources: the overlay is hidden while there is nothing to show, only the parts of the
  screen that change are redrawn, kept strokes are drawn once, idle memory is handed back, and it
  uses no CPU while nothing moves. It follows each monitor's scaling, keeps your annotations when
  displays change, and recovers on its own if the graphics driver resets.

## Default shortcuts

| Action | Shortcut | Active |
| --- | --- | --- |
| Turn laser on or off | Alt+L | always |
| Turn laser off | Esc | while the laser is on |
| Undo | Ctrl+Z | while the laser is on |
| Redo | Ctrl+Y | while the laser is on |
| Clear strokes | Alt+X | while strokes are on screen |
| Keep strokes on or off | Alt+K | always |
| Next / previous preset | Alt+] / Alt+[ | always |
| Quit | Alt+Shift+L | always |

While the laser is on: drag to draw, right-click to click through, middle and side buttons and
both scroll wheels reach the app underneath, Ctrl+scroll changes thickness. While drawing, hold
Alt for a horizontal line, Shift+Alt for a vertical one, and Ctrl as you let go to skip snapping.

## Games

The laser can draw over games running in borderless or windowed mode. Games in exclusive
fullscreen own the display, so nothing can appear over them; you'll get a notice saying so.

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

### Store package

`packaging\package.ps1` builds the MSIX the Microsoft Store takes (it needs the Windows SDK), with
the identity Partner Center assigns; run `Get-Help packaging\package.ps1` for the details. Pushing
a `v*` tag runs the same on GitHub Actions for x64 and Arm64, publishes the exes as a release and
keeps the Store bundle as an artifact. Images under `packaging\Assets` and `docs\img` come from
`assets\brand\logo.svg`: `cd tools; npm install; npm run icons`.

## Licence

[MIT](LICENSE).
