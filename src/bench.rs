//! Timings for the work behind each thing the user waits on: the laser turning on, a frame while
//! drawing, a shape snapping. Runs against a hidden overlay, so nothing appears on screen.
//! `cargo test --release perf_report -- --ignored --nocapture`

use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::graphics::Graphics;
use crate::stroke::{Point, Stroke};
use crate::{settings, shapes, virtual_screen, App, CursorKey, Drawn};

extern "system" fn quiet(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Median and worst of `runs` timings of `work`, in milliseconds.
fn time(label: &str, runs: usize, mut work: impl FnMut()) {
    report(label, runs, || {
        let started = Instant::now();
        work();
        started.elapsed()
    });
}

/// The same, for work that times only its own part.
fn report(label: &str, runs: usize, mut work: impl FnMut() -> Duration) {
    let mut times: Vec<Duration> = (0..runs).map(|_| work()).collect();
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!("{label:<44} median {:>7.3} ms   worst {:>7.3} ms", ms(times[runs / 2]), ms(times[runs - 1]));
}

fn circle(n: usize, r: f32, at: Instant) -> Vec<Point> {
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU * 1.05;
            Point { x: 900.0 + r * t.cos(), y: 600.0 + r * t.sin(), at }
        })
        .collect()
}

#[test]
#[ignore = "prints timings"]
fn perf_report() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None).unwrap();
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(quiet),
            hInstance: instance.into(),
            lpszClassName: w!("LaserPointerBench"),
            ..Default::default()
        });
        let (x, y, width, height) = virtual_screen();
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("LaserPointerBench"),
            w!("bench"),
            WS_POPUP,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        eprintln!("virtual screen {width} x {height}");
        let size = (width as u32, height as u32);
        let started = Instant::now();
        let gfx = Graphics::new(hwnd, size.0, size.1).unwrap();
        eprintln!("{:<44} {:>7.3} ms", "Graphics::new", started.elapsed().as_secs_f64() * 1000.0);
        let mut app = App::new(hwnd, gfx, (x, y), size);

        // Turning on after the overlay has been idle: buffers back, toast, a whole-screen frame.
        time("untrim (screen-sized buffers back)", 10, || {
            let gfx = app.gfx.as_mut().unwrap();
            gfx.trim().unwrap();
            gfx.untrim().unwrap();
        });
        time("make_toast", 20, || {
            app.gfx.as_ref().unwrap().make_toast("Laser on", 800.0, 1000.0, 1.0).unwrap();
        });
        let mut step = 0u8;
        time("render a cursor frame (uncached)", 20, || {
            step = step.wrapping_add(1) % 40;
            let key = CursorKey { style: 0, color: 0xff2a2a, dpi: 96, step, turn: 0 };
            let cursor = app.render_cursor(key).unwrap();
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyCursor(cursor);
        });
        time("tray icon", 20, || {
            let svg = crate::tray_svg(true, 0xff2a2a);
            let icon = app.gfx.as_ref().unwrap().svg_icon(&svg, 24, None, 1.0, 0.0).unwrap();
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(icon);
        });
        let waitable = app.waitable().unwrap();
        // Waits for the swap chain as the message loop does, then times the frame alone.
        let frame = |app: &mut App| {
            WaitForSingleObject(waitable, 100);
            let started = Instant::now();
            app.render();
            started.elapsed()
        };
        report("whole-screen first frame", 10, || {
            app.presented = false;
            frame(&mut app)
        });

        // How long the GPU takes over a whole-screen frame: from presenting it until the swap
        // chain is ready for the next.
        report("whole-screen frame, present to GPU done", 10, || {
            WaitForSingleObject(waitable, 100);
            app.presented = false;
            app.render();
            let started = Instant::now();
            WaitForSingleObject(waitable, 100);
            let done = started.elapsed();
            app.dirty = true;
            app.render();
            done
        });

        time("games::exclusive_fullscreen", 20, || {
            std::hint::black_box(crate::games::exclusive_fullscreen());
        });
        let screen = windows::Win32::Foundation::RECT { left: x, top: y, right: x + width, bottom: y + height };
        time("games::mouse_capturing_window", 20, || {
            std::hint::black_box(crate::games::mouse_capturing_window(hwnd, screen));
        });
        time("sync_hotkeys", 20, crate::sync_hotkeys);

        // Explorer updating a tray icon, as on every toggle. Hidden, so nothing shows.
        {
            use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_STATE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIS_HIDDEN, NOTIFYICONDATAW};
            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 77,
                uFlags: NIF_STATE,
                dwState: NIS_HIDDEN,
                dwStateMask: NIS_HIDDEN,
                ..Default::default()
            };
            if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                let svg = crate::tray_svg(true, 0xff2a2a);
                time("Shell_NotifyIcon modify (hidden icon)", 20, || {
                    let icon = app.gfx.as_ref().unwrap().svg_icon(&svg, 24, None, 1.0, 0.0).unwrap();
                    data.uFlags = NIF_ICON | NIF_TIP | NIF_STATE;
                    data.hIcon = icon;
                    let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
                    let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(icon);
                });
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            } else {
                eprintln!("no hidden tray icon");
            }
        }

        // Drawing: a laser stroke growing a point a frame, as the hand moves.
        let look = settings::get().look;
        let now = Instant::now();
        app.strokes.push(Drawn { id: 1, stroke: Stroke::new(Point { x: 300.0, y: 300.0, at: now }, look, 1.0), baked: false, morph: None });
        app.drawing = true;
        let mut i = 0;
        report("frame while drawing (laser, glow)", 120, || {
            i += 1;
            let t = i as f32 * 0.05;
            let stroke = &mut app.strokes[0].stroke;
            stroke.points.push(Point { x: 900.0 + 300.0 * t.cos(), y: 600.0 + 300.0 * t.sin(), at: Instant::now() });
            app.dirty = true;
            frame(&mut app)
        });

        // Snapped shapes fading out whole: three circles and three boxes across the screen, with a
        // laser trail crossing them. First with every point the morph used, then thinned to the
        // points that bend each shape, as they are drawn once the morph ends.
        app.drawing = false;
        app.strokes.clear();
        let drawn_shapes: Vec<Vec<(f32, f32)>> = (0..6)
            .map(|k| {
                let (cx, cy) = (500.0 + 900.0 * (k % 3) as f32, 500.0 + 900.0 * (k / 3) as f32);
                let outline: Vec<(f32, f32)> = if k % 2 == 0 {
                    circle(400, 260.0, now).iter().map(|p| (p.x - 900.0 + cx, p.y - 600.0 + cy)).collect()
                } else {
                    let corners = [(cx - 300.0, cy - 200.0), (cx + 300.0, cy - 200.0), (cx + 300.0, cy + 200.0), (cx - 300.0, cy + 200.0), (cx - 300.0, cy - 200.0)];
                    corners.windows(2).flat_map(|w| (0..100).map(move |i| { let t = i as f32 / 100.0; (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t) })).collect()
                };
                outline
            })
            .collect();
        for thinned in [false, true] {
            app.strokes.clear();
            for (k, outline) in drawn_shapes.iter().enumerate() {
                let correction = shapes::correct(outline, 1.0, 0.35).expect("a clean shape snaps");
                let parts = if thinned { shapes::lean(&correction.parts) } else { correction.parts.clone() };
                let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, look, 1.0);
                crate::set_parts(&mut stroke, &parts, now);
                stroke.whole = true;
                stroke.look.smoothing = 0;
                app.strokes.push(Drawn { id: 10 + k as u64, stroke, baked: false, morph: None });
            }
            let points: usize = app.strokes.iter().map(|d| d.stroke.points.len()).sum();
            let mut trail = Stroke::new(Point { x: 200.0, y: 200.0, at: now }, look, 1.0);
            trail.points = (0..200).map(|i| Point { x: 200.0 + i as f32 * 6.0, y: 300.0 + (i as f32 * 0.1).sin() * 300.0, at: Instant::now() }).collect();
            app.strokes.push(Drawn { id: 20, stroke: trail, baked: false, morph: None });
            let label = format!("six snapped shapes fading, {points} points");
            report(&label, 40, || {
                for drawn in &mut app.strokes {
                    drawn.stroke.points.iter_mut().for_each(|p| p.at = Instant::now());
                }
                app.dirty = true;
                frame(&mut app)
            });
        }
        app.strokes.clear();

        // Recognising shapes of different lengths.
        for n in [200, 1000, 4096] {
            let path: Vec<(f32, f32)> = circle(n, 300.0, now).iter().map(|p| (p.x, p.y)).collect();
            time(&format!("shapes::correct, circle of {n} points"), 50, || {
                std::hint::black_box(shapes::correct(&path, 1.0, 0.35));
            });
            let scribble: Vec<(f32, f32)> = (0..n).map(|i| { let t = i as f32 * 0.37; (500.0 + 200.0 * (t * 1.3).sin() + t, 400.0 + 150.0 * (t * 0.7).cos()) }).collect();
            time(&format!("shapes::correct, scribble of {n} points"), 50, || {
                std::hint::black_box(shapes::correct(&scribble, 1.0, 0.35));
            });
        }
    }
}
