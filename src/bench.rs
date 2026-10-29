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

/// A window like the overlay that is never shown, to draw into.
unsafe fn hidden_window(x: i32, y: i32, width: i32, height: i32) -> HWND {
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let instance = GetModuleHandleW(None).unwrap();
    RegisterClassW(&WNDCLASSW {
        lpfnWndProc: Some(quiet),
        hInstance: instance.into(),
        lpszClassName: w!("LaserPointerBench"),
        ..Default::default()
    });
    CreateWindowExW(
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
    .unwrap()
}

/// A long kept stroke drawn in settled pieces looks the same as drawn in one go: no seams where
/// the pieces meet, nothing darker where they overlap, and so nothing shifts as a stretch
/// settles. Also compared with plain `draw`, which kept strokes used before.
#[test]
#[ignore = "needs a GPU"]
fn kept_pieces_look_like_the_whole_stroke() {
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_SIZE_U};
    use windows::Win32::Graphics::Direct2D::{
        D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET, D2D1_MAP_OPTIONS_READ,
    };
    use crate::graphics::bitmap_properties;
    use crate::settings::{STYLE_GLOW, STYLE_NEON};
    use crate::stroke::{self, Pieces, Scratch};
    unsafe {
        let size = 900u32;
        let hwnd = hidden_window(0, 0, size as i32, size as i32);
        let gfx = Graphics::new(hwnd, size, size).unwrap();
        let extent = D2D_SIZE_U { width: size, height: size };
        let pixels = |draw: &dyn Fn()| -> Vec<u8> {
            let canvas = gfx.dc.CreateBitmap(extent, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET)).unwrap();
            let readback = gfx.dc.CreateBitmap(extent, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW)).unwrap();
            gfx.dc.SetTarget(&canvas);
            gfx.dc.BeginDraw();
            gfx.dc.Clear(Some(&D2D1_COLOR_F::default()));
            draw();
            gfx.dc.EndDraw(None, None).unwrap();
            readback.CopyFromBitmap(None, &canvas, None).unwrap();
            let mapped = readback.Map(D2D1_MAP_OPTIONS_READ).unwrap();
            let mut out = Vec::new();
            for row in 0..size as usize {
                out.extend_from_slice(std::slice::from_raw_parts(mapped.bits.add(row * mapped.pitch as usize), size as usize * 4));
            }
            readback.Unmap().unwrap();
            out
        };
        let now = Instant::now();
        let everywhere = crate::stroke::Bounds { left: 0.0, top: 0.0, right: size as f32, bottom: size as f32 };
        // A spiral with a wobble, sampled as a hand moves: near and far apart. And a ruler-straight
        // line, which thinning cuts down to its ends, so a piece that lost one would show.
        let spiral = |i: usize| {
            let t = i as f32 * 0.021 + 0.004 * (i as f32 * 0.37).sin() * i as f32 / 100.0;
            let r = 40.0 + t * 13.0 + 3.0 * (t * 7.0).sin();
            (450.0 + r * t.cos(), 450.0 + r * t.sin())
        };
        let straight = |i: usize| (60.0 + i as f32 * 0.55, 100.0 + i as f32 * 0.5);
        type Path = fn(usize) -> (f32, f32);
        let paths: [(u32, u32, Path); 4] = [(STYLE_GLOW, 2, spiral), (STYLE_NEON, 0, spiral), (0, 4, spiral), (STYLE_GLOW, 2, straight)];
        for (style, smoothing, path) in paths {
            let mut look = settings::get().look;
            (look.ink, look.style, look.smoothing, look.width) = (true, style, smoothing, 6);
            let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, look, 1.0);
            stroke.points = (0..1400).map(|i| { let (x, y) = path(i); Point { x, y, at: now } }).collect();
            let whole = pixels(&|| {
                stroke::draw_kept(&gfx.factory, &gfx.dc1, &gfx.brush, &stroke, &mut Pieces::default(), everywhere, false, &mut Scratch::default()).unwrap();
            });
            let plain = pixels(&|| {
                let mut copy = stroke.clone();
                stroke::draw(&gfx.factory, &gfx.dc, &gfx.brush, &mut copy, now, 1.0, &mut Scratch::default()).unwrap();
            });
            let pieced = pixels(&|| {
                stroke::draw_kept(&gfx.factory, &gfx.dc1, &gfx.brush, &stroke, &mut Pieces::default(), everywhere, true, &mut Scratch::default()).unwrap();
            });
            if let Ok(directory) = std::env::var("PIECES_DUMP") {
                for (name, data) in [("whole", &whole), ("plain", &plain), ("pieced", &pieced)] {
                    std::fs::write(format!("{directory}/{name}-{style}.raw"), data).unwrap();
                }
            }
            // Compared as seen, over 3 x 3 pixels: the pieces' tessellation may put an edge a
            // fraction of a pixel from where a live fill does, but a seam or a darker overlap
            // spans more than that.
            let seen = |pixels: &[u8], x: usize, y: usize| -> f32 {
                let n = size as usize;
                (0..9).map(|k| f32::from(pixels[((y + k / 3 - 1) * n + x + k % 3 - 1) * 4 + 3])).sum::<f32>() / 9.0
            };
            let compare = |a: &[u8], b: &[u8]| {
                let (mut visible, mut worst) = (0, 0.0f32);
                for y in 1..size as usize - 1 {
                    for x in 1..size as usize - 1 {
                        let difference = (seen(a, x, y) - seen(b, x, y)).abs();
                        worst = worst.max(difference);
                        if difference > 24.0 {
                            visible += 1;
                        }
                    }
                }
                (visible, worst)
            };
            let painted = whole.chunks(4).filter(|p| p[3] > 0).count();
            let (visible, worst) = compare(&whole, &pieced);
            let (plainly, plain_worst) = compare(&plain, &pieced);
            eprintln!("style {style}: against one go, {visible} of {painted} painted pixels look different, worst by {worst:.0} of 255; against plain draw, {plainly}, worst {plain_worst:.0}");
            assert!(worst < 32.0 && visible * 1000 < painted, "style {style}: {visible} of {painted}, worst {worst}");
            // Plain draw puts some flat edges a pixel lighter; nothing more than that.
            assert!(plain_worst < 64.0 && plainly * 100 < painted, "style {style}: {plainly} of {painted}, worst {plain_worst}");
        }
    }
}

/// A settled shape drawn from its tessellated parts looks as plain `draw` makes it, holding and
/// then halfway through fading out.
#[test]
#[ignore = "needs a GPU"]
fn settled_shapes_drawn_once_look_like_plain_draw() {
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_SIZE_U};
    use windows::Win32::Graphics::Direct2D::{
        D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET, D2D1_MAP_OPTIONS_READ,
    };
    use crate::graphics::bitmap_properties;
    use crate::settings::{STYLE_GLOW, STYLE_NEON};
    use crate::stroke::{self, Pieces, Scratch, SHAPE_FADE_MS};
    unsafe {
        let size = 600u32;
        let hwnd = hidden_window(0, 0, size as i32, size as i32);
        let gfx = Graphics::new(hwnd, size, size).unwrap();
        let extent = D2D_SIZE_U { width: size, height: size };
        let pixels = |draw: &dyn Fn()| -> Vec<u8> {
            let canvas = gfx.dc.CreateBitmap(extent, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET)).unwrap();
            let readback = gfx.dc.CreateBitmap(extent, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW)).unwrap();
            gfx.dc.SetTarget(&canvas);
            gfx.dc.BeginDraw();
            gfx.dc.Clear(Some(&D2D1_COLOR_F::default()));
            draw();
            gfx.dc.EndDraw(None, None).unwrap();
            readback.CopyFromBitmap(None, &canvas, None).unwrap();
            let mapped = readback.Map(D2D1_MAP_OPTIONS_READ).unwrap();
            let mut out = Vec::new();
            for row in 0..size as usize {
                out.extend_from_slice(std::slice::from_raw_parts(mapped.bits.add(row * mapped.pitch as usize), size as usize * 4));
            }
            readback.Unmap().unwrap();
            out
        };
        let at = Instant::now();
        for style in [0, STYLE_GLOW, STYLE_NEON] {
            let mut look = settings::get().look;
            (look.ink, look.style, look.smoothing, look.width) = (false, style, 0, 6);
            // A ring and a stroke through it, as an arrow's parts are.
            let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at }, look, 1.0);
            stroke.points = (0..=90).map(|i| { let t = i as f32 / 90.0 * std::f32::consts::TAU; Point { x: 300.0 + 150.0 * t.cos(), y: 300.0 + 150.0 * t.sin(), at } }).collect();
            stroke.breaks.push(stroke.points.len());
            stroke.points.extend([Point { x: 100.0, y: 500.0, at }, Point { x: 500.0, y: 100.0, at }]);
            stroke.whole = true;
            let mut pieces = Pieces::default();
            for age in [0, look.fade_ms + SHAPE_FADE_MS / 2] {
                let now = at + Duration::from_millis(u64::from(age));
                let plain = pixels(&|| {
                    let mut copy = stroke.clone();
                    stroke::draw(&gfx.factory, &gfx.dc, &gfx.brush, &mut copy, now, 1.0, &mut Scratch::default()).unwrap();
                });
                let cached = std::cell::RefCell::new(std::mem::take(&mut pieces));
                let shape = pixels(&|| {
                    stroke::draw_shape(&gfx.factory, &gfx.dc1, &gfx.brush, &stroke, &mut cached.borrow_mut(), now, &mut Scratch::default()).unwrap();
                });
                pieces = cached.into_inner();
                if let Ok(directory) = std::env::var("PIECES_DUMP") {
                    std::fs::write(format!("{directory}/plain-{style}-{age}.raw"), &plain).unwrap();
                    std::fs::write(format!("{directory}/shape-{style}-{age}.raw"), &shape).unwrap();
                }
                // Compared as seen, over 3 x 3 pixels: a tessellated edge may sit a fraction of a
                // pixel from where a live fill puts it, and steps along a nearly flat edge, as
                // kept strokes' pieces do. A missing part or a doubled pass shows far more.
                // Every channel, so a halo drawn over a part's white centre, which tints it
                // without changing its coverage, shows too.
                let n = size as usize;
                let seen = |pixels: &[u8], x: usize, y: usize, channel: usize| -> f32 {
                    (0..9).map(|k| f32::from(pixels[((y + k / 3 - 1) * n + x + k % 3 - 1) * 4 + channel])).sum::<f32>() / 9.0
                };
                let (mut differ, mut worst) = (0, 0.0f32);
                for y in 1..n - 1 {
                    for x in 1..n - 1 {
                        let difference = (0..4).map(|channel| (seen(&plain, x, y, channel) - seen(&shape, x, y, channel)).abs()).fold(0.0, f32::max);
                        worst = worst.max(difference);
                        differ += usize::from(difference > 24.0);
                    }
                }
                let painted = plain.chunks(4).filter(|p| p[3] > 0).count();
                eprintln!("style {style}, {age} ms: {differ} of {painted} painted pixels look different, worst by {worst:.0}");
                assert!(painted > 5000 && worst < 80.0 && differ * 200 < painted, "style {style}, {age} ms: {differ} of {painted}, worst {worst}");
            }
        }
    }
}

/// Drawing a kept stroke repaints only where it's still growing. What's left on screen once it
/// stops must be what a full repaint shows: nothing missing, nothing smeared.
#[test]
#[ignore = "needs a GPU"]
fn a_growing_kept_stroke_leaves_what_a_full_repaint_shows() {
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    unsafe {
        let size = 900u32;
        let hwnd = hidden_window(0, 0, size as i32, size as i32);
        let gfx = Graphics::new(hwnd, size, size).unwrap();
        let mut app = App::new(hwnd, gfx, (0, 0), (size, size));
        app.live_cursor = || None;
        let waitable = app.waitable().unwrap();
        let frame = |app: &mut App| {
            let _ = WaitForSingleObject(waitable, 100) == WAIT_OBJECT_0;
            app.dirty = true;
            app.render();
        };
        let now = Instant::now();
        let mut look = settings::get().look;
        look.ink = true;
        app.strokes.push(Drawn::new(1, Stroke::new(Point { x: 450.0, y: 450.0, at: now }, look, 1.0)));
        app.drawing = true;
        for i in 1..1500 {
            let t = i as f32 * 0.02;
            let r = 20.0 + t * 13.0;
            app.strokes[0].stroke.points.push(Point { x: 450.0 + r * t.cos(), y: 450.0 + r * t.sin(), at: now });
            if i % 4 == 0 {
                frame(&mut app);
            }
        }
        assert!(app.strokes[0].pieces.settled() > 0, "the stroke never settled a piece");
        for _ in 0..3 {
            frame(&mut app);
        }
        let grown = app.gfx.as_ref().unwrap().read_back();
        app.presented = false;
        frame(&mut app);
        app.presented = false;
        frame(&mut app);
        let repainted = app.gfx.as_ref().unwrap().read_back();
        let painted = repainted.chunks(4).filter(|p| p[3] > 0).count();
        let differ = grown.chunks(4).zip(repainted.chunks(4)).filter(|(a, b)| a[3].abs_diff(b[3]) > 24).count();
        eprintln!("{differ} of {painted} painted pixels differ");
        assert!(painted > 10_000 && differ * 1000 < painted, "{differ} of {painted}");
    }
}

/// The head of a stroke being drawn follows the mouse between its messages: drawn where the
/// cursor is, even when the stroke has as many points as last frame, and never kept.
#[test]
#[ignore = "needs a GPU"]
fn the_head_follows_the_cursor_and_is_never_kept() {
    use std::sync::atomic::{AtomicI32, Ordering};
    use windows::Win32::Foundation::{POINT, WAIT_OBJECT_0};
    static CURSOR_Y: AtomicI32 = AtomicI32::new(0);
    unsafe {
        let size = 600u32;
        let hwnd = hidden_window(0, 0, size as i32, size as i32);
        let gfx = Graphics::new(hwnd, size, size).unwrap();
        let mut app = App::new(hwnd, gfx, (0, 0), (size, size));
        app.live_cursor = || Some(POINT { x: 400, y: CURSOR_Y.load(Ordering::Relaxed) });
        let waitable = app.waitable().unwrap();
        let frame = |app: &mut App| {
            assert_eq!(WaitForSingleObject(waitable, 100), WAIT_OBJECT_0, "a frame went without a present");
            app.dirty = true;
            app.render();
        };
        let painted_at = |app: &App, y: usize| {
            let pixels = app.gfx.as_ref().unwrap().read_back();
            pixels[(y * size as usize + 400) * 4 + 3] > 0
        };
        // A short stroke, fading and kept, and a kept one long enough to have settled pieces,
        // coiled up away from where the head goes.
        let coil: Vec<(f32, f32)> = (0..900).map(|i| { let t = i as f32 * 0.05; let r = 10.0 + t * 1.6; (100.0 + r * t.cos(), 100.0 + r * t.sin()) }).collect();
        for (ink, path) in [(false, vec![(100.0, 300.0), (200.0, 300.0)]), (true, vec![(100.0, 300.0), (200.0, 300.0)]), (true, coil)] {
            let now = Instant::now();
            let mut look = settings::get().look;
            look.ink = ink;
            let mut stroke = Stroke::new(Point { x: path[0].0, y: path[0].1, at: now }, look, 1.0);
            stroke.points = path.iter().map(|&(x, y)| Point { x, y, at: now }).collect();
            let length = stroke.points.len();
            app.strokes = vec![Drawn::new(1, stroke)];
            app.drawing = true;
            // Straight on from the stroke, then off to one side, the stroke as long as before.
            for y in [300, 500] {
                CURSOR_Y.store(y, Ordering::Relaxed);
                // The frame before last is the one read back.
                for _ in 0..3 {
                    frame(&mut app);
                }
                assert!(painted_at(&app, y as usize), "ink {ink}, {length} points: no head at the cursor, {y}");
                assert_eq!(app.strokes[0].stroke.points.len(), length, "the head was kept");
            }
            assert!(!painted_at(&app, 300), "ink {ink}, {length} points: the head drawn before stayed");
            if length > 600 {
                assert!(app.strokes[0].pieces.settled() > 0, "the long stroke never settled a piece");
            }
            app.drawing = false;
            app.strokes.clear();
            for _ in 0..3 {
                frame(&mut app);
            }
        }
    }
}

/// Undoing a kept stroke that a fading one still holds back from the ink layer leaves nothing
/// of it on screen, though only its growing end was repainted while it was drawn.
#[test]
#[ignore = "needs a GPU"]
fn undoing_a_kept_stroke_leaves_nothing_behind() {
    unsafe {
        let size = 900u32;
        let hwnd = hidden_window(0, 0, size as i32, size as i32);
        let gfx = Graphics::new(hwnd, size, size).unwrap();
        let mut app = App::new(hwnd, gfx, (0, 0), (size, size));
        app.live_cursor = || None;
        let waitable = app.waitable().unwrap();
        let frame = |app: &mut App| {
            WaitForSingleObject(waitable, 100);
            app.dirty = true;
            app.render();
        };
        let now = Instant::now();
        let mut laser = settings::get().look;
        (laser.ink, laser.fade_ms) = (false, 5000);
        let mut fading = Stroke::new(Point { x: 100.0, y: 100.0, at: now }, laser, 1.0);
        fading.points.push(Point { x: 200.0, y: 120.0, at: now });
        app.strokes.push(Drawn::new(1, fading));
        let mut ink = laser;
        ink.ink = true;
        app.strokes.push(Drawn::new(2, Stroke::new(Point { x: 450.0, y: 450.0, at: now }, ink, 1.0)));
        app.undo.push(crate::Edit::Added(2));
        app.drawing = true;
        for i in 1..900 {
            let t = i as f32 * 0.02;
            let r = 20.0 + t * 13.0;
            app.strokes[1].stroke.points.push(Point { x: 450.0 + r * t.cos(), y: 450.0 + r * t.sin(), at: now });
            if i % 4 == 0 {
                frame(&mut app);
            }
        }
        app.drawing = false;
        frame(&mut app);
        assert!(!app.strokes[1].baked, "the fading stroke should still hold it back");
        app.undo();
        for _ in 0..3 {
            frame(&mut app);
        }
        let after = app.gfx.as_ref().unwrap().read_back();
        app.presented = false;
        frame(&mut app);
        app.presented = false;
        frame(&mut app);
        let repainted = app.gfx.as_ref().unwrap().read_back();
        let differ = after.chunks(4).zip(repainted.chunks(4)).filter(|(a, b)| a[3].abs_diff(b[3]) > 24).count();
        eprintln!("{differ} pixels left behind");
        assert_eq!(differ, 0);
    }
}

/// Clearing kept strokes fades the ink layer out as one picture: once the fade is over nothing
/// is left, and undoing the clear brings every stroke back.
#[test]
#[ignore = "needs a GPU"]
fn a_clear_fades_away_and_undoes() {
    unsafe {
        let size = 900u32;
        let hwnd = hidden_window(0, 0, size as i32, size as i32);
        let gfx = Graphics::new(hwnd, size, size).unwrap();
        let mut app = App::new(hwnd, gfx, (0, 0), (size, size));
        app.live_cursor = || None;
        let waitable = app.waitable().unwrap();
        let frame = |app: &mut App| {
            WaitForSingleObject(waitable, 100);
            app.dirty = true;
            app.render();
        };
        let now = Instant::now();
        let mut ink = settings::get().look;
        ink.ink = true;
        for k in 0..5u64 {
            let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, ink, 1.0);
            stroke.points = (0..300).map(|i| Point { x: 100.0 + i as f32 * 2.3, y: 150.0 + k as f32 * 140.0 + (i as f32 * 0.05).sin() * 40.0, at: now }).collect();
            app.strokes.push(Drawn::new(k + 1, stroke));
        }
        let shown = |app: &mut App| {
            for _ in 0..3 {
                frame(app);
            }
            app.gfx.as_ref().unwrap().read_back()
        };
        let repainted = |app: &mut App| {
            app.presented = false;
            frame(app);
            app.presented = false;
            frame(app);
            app.gfx.as_ref().unwrap().read_back()
        };
        let painted = |pixels: &[u8]| pixels.chunks(4).filter(|p| p[3] > 0).count();
        let before = shown(&mut app);
        assert!(app.strokes.iter().all(|drawn| drawn.baked));

        // A clear whose fade gets no frames at all, as when a menu holds the thread, still
        // leaves nothing once frames come again.
        app.clear(|_| true);
        std::thread::sleep(Duration::from_secs_f32(crate::CLEAR_SECS + 0.05));
        assert_eq!(painted(&shown(&mut app)), 0, "ink left after a clear with no frames");
        app.undo();
        let _ = shown(&mut app);

        app.clear(|_| true);
        // The back buffer holds the frame before last.
        frame(&mut app);
        frame(&mut app);
        let fading = app.gfx.as_ref().unwrap().read_back();
        let alpha = |pixels: &[u8]| pixels.chunks(4).map(|p| u64::from(p[3])).sum::<u64>();
        assert!(alpha(&fading) < alpha(&before) && alpha(&fading) > 0, "the ink should be fading, not gone or whole");
        std::thread::sleep(Duration::from_secs_f32(crate::CLEAR_SECS));
        let gone = shown(&mut app);
        assert_eq!(painted(&gone), 0, "ink left after the fade");
        app.undo();
        let back = shown(&mut app);
        let full = repainted(&mut app);
        let differ = back.chunks(4).zip(full.chunks(4)).filter(|(a, b)| a[3].abs_diff(b[3]) > 24).count();
        eprintln!("{} painted before, {} after undo, {differ} differ from a full repaint", painted(&before), painted(&back));
        assert!(differ == 0 && painted(&back) * 100 > painted(&before) * 99);

        // Undoing one kept stroke that crosses the others repairs only where it was, and the
        // strokes it crossed come back whole there.
        let mut across = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, ink, 1.0);
        across.points = (0..300).map(|i| Point { x: 450.0 + (i as f32 * 0.04).sin() * 60.0, y: 80.0 + i as f32 * 2.5, at: now }).collect();
        app.strokes.push(Drawn::new(6, across));
        app.undo.push(crate::Edit::Added(6));
        let _ = shown(&mut app);
        assert!(app.strokes.iter().all(|drawn| drawn.baked));
        app.undo();
        let after = shown(&mut app);
        let full = repainted(&mut app);
        let differ = after.chunks(4).zip(full.chunks(4)).filter(|(a, b)| a[3].abs_diff(b[3]) > 24).count();
        let still = after.chunks(4).zip(before.chunks(4)).filter(|(a, b)| a[3].abs_diff(b[3]) > 24).count();
        eprintln!("after undoing the crossing stroke: {differ} differ from a full repaint, {still} from before it was drawn");
        assert!(differ == 0 && still == 0);
    }
}

/// Kept strokes baked into the ink layer's tiles look as they do drawn into one bitmap: nothing
/// shifted, doubled or cut off where tiles meet, halos included, before and after undoing one
/// that crosses the rest.
#[test]
#[ignore = "needs a GPU"]
fn baked_tiles_look_like_one_layer() {
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_SIZE_U};
    use windows::Win32::Graphics::Direct2D::{
        D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET, D2D1_MAP_OPTIONS_READ,
    };
    use crate::graphics::bitmap_properties;
    use crate::settings::{STYLE_GLOW, STYLE_NEON};
    use crate::stroke::{self, Pieces, Scratch};
    unsafe {
        // Tiles meet at 512 and 1024 both ways.
        let size = 1100u32;
        let hwnd = hidden_window(0, 0, size as i32, size as i32);
        let gfx = Graphics::new(hwnd, size, size).unwrap();
        let mut app = App::new(hwnd, gfx, (0, 0), (size, size));
        app.live_cursor = || None;
        let waitable = app.waitable().unwrap();
        let shown = |app: &mut App| {
            for _ in 0..3 {
                WaitForSingleObject(waitable, 100);
                app.dirty = true;
                app.render();
            }
            app.gfx.as_ref().unwrap().read_back()
        };
        let now = Instant::now();
        // A thin unsmoothed stroke turning a corner at its 17th sample, where one rectangle of
        // its coverage ends: the curve cuts the corner across 512, 512 into a tile neither
        // straight stretch is over. Baked first, while no other ink has made that tile.
        let mut look = settings::get().look;
        (look.ink, look.style, look.width, look.smoothing) = (true, 0, 2, 0);
        let mut corner = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, look, 1.0);
        corner.points = (0..16)
            .map(|i| (622.0 - 5.0 * i as f32, 507.0))
            .chain([(507.0, 507.0), (507.0, 547.0), (507.0, 587.0)])
            .map(|(x, y)| Point { x, y, at: now })
            .collect();
        let mut strokes = vec![corner];
        for (k, style) in [STYLE_GLOW, STYLE_NEON, 0].into_iter().enumerate() {
            let mut look = settings::get().look;
            (look.ink, look.style, look.width) = (true, style, 8 + 4 * k as u32);
            let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, look, 1.0);
            stroke.points = (0..900)
                .map(|i| {
                    let t = i as f32 * 0.02 + k as f32;
                    let r = 30.0 + t * 24.0;
                    Point { x: 512.0 + r * t.cos() + k as f32 * 150.0, y: 512.0 + r * t.sin() + k as f32 * 120.0, at: now }
                })
                .collect();
            strokes.push(stroke);
        }
        // A snapped circle, drawn whole rather than grown, across the corner where four tiles meet.
        let mut look = settings::get().look;
        (look.ink, look.style) = (true, STYLE_GLOW);
        let outline: Vec<(f32, f32)> = (0..=240).map(|i| { let t = i as f32 / 240.0 * std::f32::consts::TAU; (1024.0 + 60.0 * t.cos(), 1024.0 + 50.0 * t.sin()) }).collect();
        let correction = shapes::correct(&outline, 1.0, 0.35).expect("a clean circle snaps");
        let mut circle = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, look, 1.0);
        crate::set_parts(&mut circle, &shapes::lean(&correction.parts), now);
        circle.whole = true;
        circle.look.smoothing = 0;
        strokes.push(circle);

        let extent = D2D_SIZE_U { width: size, height: size };
        let reference = |app: &App, strokes: &[Stroke]| -> Vec<u8> {
            let gfx = app.gfx.as_ref().unwrap();
            let canvas = gfx.dc.CreateBitmap(extent, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET)).unwrap();
            let readback = gfx.dc.CreateBitmap(extent, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW)).unwrap();
            gfx.dc.SetTarget(&canvas);
            gfx.dc.BeginDraw();
            gfx.dc.Clear(Some(&D2D1_COLOR_F::default()));
            let everywhere = stroke::Bounds { left: 0.0, top: 0.0, right: size as f32, bottom: size as f32 };
            let mut scratch = Scratch::default();
            for stroke in strokes {
                let mut copy = stroke.clone();
                if stroke::grows(stroke) {
                    stroke::draw_kept(&gfx.factory, &gfx.dc1, &gfx.brush, stroke, &mut Pieces::default(), everywhere, false, &mut scratch).unwrap();
                } else {
                    stroke::draw(&gfx.factory, &gfx.dc, &gfx.brush, &mut copy, now, 1.0, &mut scratch).unwrap();
                }
            }
            gfx.dc.EndDraw(None, None).unwrap();
            scratch.finish_frame();
            readback.CopyFromBitmap(None, &canvas, None).unwrap();
            let mapped = readback.Map(D2D1_MAP_OPTIONS_READ).unwrap();
            let mut out = Vec::new();
            for row in 0..size as usize {
                out.extend_from_slice(std::slice::from_raw_parts(mapped.bits.add(row * mapped.pitch as usize), size as usize * 4));
            }
            readback.Unmap().unwrap();
            out
        };
        // Compared as seen, over 3 x 3 pixels, as the kept pieces are.
        let compare = |a: &[u8], b: &[u8]| {
            let n = size as usize;
            let seen = |pixels: &[u8], x: usize, y: usize| (0..9).map(|k| f32::from(pixels[((y + k / 3 - 1) * n + x + k % 3 - 1) * 4 + 3])).sum::<f32>() / 9.0;
            let (mut visible, mut worst) = (0, 0.0f32);
            for y in 1..n - 1 {
                for x in 1..n - 1 {
                    let difference = (seen(a, x, y) - seen(b, x, y)).abs();
                    worst = worst.max(difference);
                    if difference > 24.0 {
                        visible += 1;
                    }
                }
            }
            (visible, worst)
        };
        let mut painted = 0;
        for upto in [1, strokes.len()] {
            for (k, stroke) in strokes[..upto].iter().enumerate().skip(app.strokes.len()) {
                app.strokes.push(Drawn::new(k as u64 + 1, stroke.clone()));
                app.undo.push(crate::Edit::Added(k as u64 + 1));
            }
            let baked = shown(&mut app);
            assert!(app.strokes.iter().all(|drawn| drawn.baked));
            let one = reference(&app, &strokes[..upto]);
            painted = one.chunks(4).filter(|p| p[3] > 0).count();
            let (visible, worst) = compare(&one, &baked);
            eprintln!("{upto} baked in tiles: {visible} of {painted} painted pixels look different, worst by {worst:.0}");
            // The same drawing laid over each tile: only rounding may differ.
            assert!(worst <= 2.0, "{visible} of {painted}, worst {worst}");
        }

        // Undoing the circle repairs the tiles it was in, which the spirals cross too.
        app.undo();
        let repaired = shown(&mut app);
        let one = reference(&app, &strokes[..strokes.len() - 1]);
        let (visible, worst) = compare(&one, &repaired);
        eprintln!("after an undo: {visible} look different, worst by {worst:.0}");
        assert!(worst <= 2.0, "{visible} of {painted}, worst {worst}");
    }
}

#[test]
#[ignore = "prints timings"]
fn perf_report() {
    unsafe {
        let (x, y, width, height) = virtual_screen();
        let hwnd = hidden_window(x, y, width, height);
        eprintln!("virtual screen {width} x {height}");
        let size = (width as u32, height as u32);
        let started = Instant::now();
        let gfx = Graphics::new(hwnd, size.0, size.1).unwrap();
        eprintln!("{:<44} {:>7.3} ms", "Graphics::new", started.elapsed().as_secs_f64() * 1000.0);
        let mut app = App::new(hwnd, gfx, (x, y), size);
        app.live_cursor = || None;

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
        // Every cursor frame the laser's ignite and press will need, made ahead while idle, so
        // none is made mid-animation. Then an ignite at the display's rate finds them all.
        {
            let started = Instant::now();
            let mut made = 0;
            while app.warm_cursor() {
                made += 1;
            }
            eprintln!("{:<44} {made} frames in {:.1} ms", "cursor frames made ahead", started.elapsed().as_secs_f64() * 1000.0);
            let settings = settings::get();
            let motion = crate::cursors::STYLES[settings.cursor].motion;
            let mut position = windows::Win32::Foundation::POINT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut position);
            let dpi = crate::monitor_dpi(position);
            let mut missing = 0;
            for frame in 0..=60 {
                let k = frame as f32 * 0.00408 / motion.ignite_secs();
                for press in [0.0, frame as f32 / 60.0] {
                    let (zoom, turn) = motion.pose(k.min(1.0), if k >= 1.0 { press } else { 0.0 });
                    let key = CursorKey { style: settings.cursor, color: settings.look.color, dpi, step: crate::zoom_step(zoom), turn: crate::turn_step(turn) };
                    if !app.pointer.cursors.iter().any(|(cached, _)| *cached == key) {
                        missing += 1;
                    }
                }
            }
            eprintln!("{:<44} {missing}", "ignite or press frames not made ahead");
            assert_eq!(missing, 0);
        }
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

        // Frames drawn outside the message loop, as on turning on, must take their turn from the
        // swap chain too: the waitable counts, so each that didn't would leave a spare signal and
        // let frames queue behind input.
        {
            std::thread::sleep(Duration::from_millis(50));
            let drain = || (0..8).take_while(|_| WaitForSingleObject(waitable, 0) == windows::Win32::Foundation::WAIT_OBJECT_0).count();
            let before = drain();
            for _ in 0..3 {
                app.presented = false;
                app.render_now();
                std::thread::sleep(Duration::from_millis(30));
            }
            eprintln!("{:<44} {before} before, {} after three frames from show()", "spare waitable signals", drain());
        }

        // A kept stroke still being drawn, long enough to have been held for a while.
        {
            let (now, mut ink) = (Instant::now(), settings::get().look);
            ink.ink = true;
            for n in [500, 2500] {
                app.strokes.clear();
                let mut stroke = Stroke::new(Point { x: 300.0, y: 300.0, at: now }, ink, 1.0);
                stroke.points = (0..n).map(|i| { let t = i as f32 * 0.02; Point { x: 900.0 + (200.0 + t * 8.0) * t.cos(), y: 900.0 + (200.0 + t * 8.0) * t.sin(), at: now } }).collect();
                app.strokes.push(Drawn::new(30, stroke));
                app.drawing = true;
                report(&format!("frame while drawing a kept stroke, {n} points"), 20, || {
                    app.dirty = true;
                    frame(&mut app)
                });
                // Letting go: the next frame bakes it into the ink layer.
                report(&format!("frame baking a finished kept stroke, {n} points"), 10, || {
                    app.drawing = true;
                    app.rebake();
                    app.strokes[0].baked = false;
                    app.dirty = true;
                    frame(&mut app);
                    app.drawing = false;
                    app.dirty = true;
                    frame(&mut app)
                });
                app.drawing = true;
            }
            // The worst case for the GPU: a kept stroke across the whole desktop, each pass a
            // layer that size. More than a refresh of GPU work would show here as two.
            app.strokes.clear();
            let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, ink, 1.0);
            stroke.points = (0..2500)
                .map(|i| {
                    let t = i as f32 / 2500.0;
                    Point { x: 50.0 + t * (width as f32 - 100.0), y: height as f32 / 2.0 + (t * 40.0).sin() * (height as f32 / 2.0 - 60.0), at: now }
                })
                .collect();
            app.strokes.push(Drawn::new(31, stroke));
            report("kept stroke across the desktop, GPU done", 10, || {
                WaitForSingleObject(waitable, 100);
                app.dirty = true;
                app.render();
                let started = Instant::now();
                WaitForSingleObject(waitable, 100);
                let done = started.elapsed();
                app.dirty = true;
                app.render();
                done
            });
            app.drawing = false;
            app.strokes.clear();
        }

        // Drawing: a laser stroke growing a point a frame, as the hand moves.
        let look = settings::get().look;
        let now = Instant::now();
        app.strokes.push(Drawn::new(1, Stroke::new(Point { x: 300.0, y: 300.0, at: now }, look, 1.0)));
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

        // Clearing a screenful of kept strokes: every frame of the short fade that follows.
        {
            let mut ink = settings::get().look;
            ink.ink = true;
            app.strokes.clear();
            for k in 0..30u64 {
                let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, ink, 1.0);
                stroke.points = (0..800)
                    .map(|i| {
                        let t = i as f32 * 0.01;
                        Point { x: 300.0 + (k % 6) as f32 * 900.0 + 300.0 * (t * 1.3).sin(), y: 300.0 + (k / 6) as f32 * 900.0 + 300.0 * (t * 0.9 + k as f32).cos(), at: now }
                    })
                    .collect();
                app.strokes.push(Drawn::new(100 + k, stroke));
            }
            frame(&mut app);
            frame(&mut app);
            let (mut times, mut rebaked) = (Vec::new(), Vec::new());
            for _ in 0..5 {
                app.clear(|_| true);
                let started = Instant::now();
                while started.elapsed().as_secs_f32() < crate::CLEAR_SECS {
                    times.push(frame(&mut app));
                }
                frame(&mut app);
                app.undo();
                rebaked.push(frame(&mut app));
                frame(&mut app);
            }
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            times.sort();
            rebaked.sort();
            eprintln!("{:<44} median {:>7.3} ms   worst {:>7.3} ms", "frame while 30 kept strokes clear", ms(times[times.len() / 2]), ms(times[times.len() - 1]));
            eprintln!("{:<44} median {:>7.3} ms   worst {:>7.3} ms", "undoing it: 30 kept strokes baked again", ms(rebaked[rebaked.len() / 2]), ms(rebaked[rebaked.len() - 1]));
            app.strokes.clear();
        }

        // The longest trails a fast hand leaves: a 1000 Hz mouse moving 4,000 px a second the
        // whole time, for a Laser's one-second fade and a Highlighter's three.
        for (preset, count) in [(0usize, 1000usize), (1, 3000)] {
            let look = settings::presets()[preset].look;
            app.strokes.clear();
            let started = Instant::now();
            let mut stroke = Stroke::new(Point { x: 900.0, y: 600.0, at: started }, look, 1.0);
            stroke.points = (0..count)
                .map(|i| {
                    let t = i as f32 * 0.0034;
                    let at = started + Duration::from_millis(i as u64);
                    Point { x: 1500.0 + 700.0 * (t * 1.7).sin(), y: 1200.0 + 500.0 * (t * 2.3).cos(), at }
                })
                .collect();
            app.strokes.push(Drawn::new(40, stroke));
            app.drawing = true;
            let label = format!("frame with a {count}-point trail ({})", settings::presets()[preset].name);
            report(&label, 20, || {
                app.dirty = true;
                frame(&mut app)
            });
            app.drawing = false;
        }
        app.strokes.clear();

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
                app.strokes.push(Drawn::new(10 + k as u64, stroke));
            }
            let points: usize = app.strokes.iter().map(|d| d.stroke.points.len()).sum();
            let mut trail = Stroke::new(Point { x: 200.0, y: 200.0, at: now }, look, 1.0);
            trail.points = (0..200).map(|i| Point { x: 200.0 + i as f32 * 6.0, y: 300.0 + (i as f32 * 0.1).sin() * 300.0, at: Instant::now() }).collect();
            app.strokes.push(Drawn::new(20, trail));
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

/// Where the mouse is in `on_screen_report`, packed as x and y, for `live_cursor`.
static HAND: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A hand moving about 3,000 px a second round a loose loop, `t` seconds in.
fn hand_at(t: f32) -> (f32, f32) {
    (1900.0 + 900.0 * (t * 2.1).sin(), 1080.0 + 600.0 * (t * 3.3).cos())
}

fn hand_cursor() -> Option<windows::Win32::Foundation::POINT> {
    let bits = HAND.load(std::sync::atomic::Ordering::Relaxed);
    Some(windows::Win32::Foundation::POINT { x: (bits >> 32) as i32, y: bits as u32 as i32 })
}

/// What drawing looks like on screen: frames a second, frames that missed a refresh, and how long
/// from a frame starting, with the newest input, to the refresh that shows it. A 1000 Hz mouse
/// draws for three seconds in each style, paced as the message loop paces frames, over the
/// overlay shown but left transparent, once it has readied what it can while idle.
/// `cargo test --release on_screen_report -- --ignored --nocapture`
#[test]
#[ignore = "shows a transparent overlay"]
fn on_screen_report() {
    use windows::Win32::System::Threading::{CreateWaitableTimerExW, SetWaitableTimer, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, DispatchMessageW, PeekMessageW, ShowWindow, MSG, PM_REMOVE, SW_SHOWNOACTIVATE};
    use crate::settings::{Look, STYLE_SOLID};
    unsafe {
        let (x, y, width, height) = virtual_screen();
        let pacer = CreateWaitableTimerExW(None, windows::core::PCWSTR::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0).unwrap();
        let mut long = settings::presets()[0].look;
        (long.style, long.fade_ms, long.smoothing) = (STYLE_SOLID, 2100, 8);
        let mut pen = settings::presets()[2].look;
        pen.ink = true;
        let scenarios: [(&str, Look); 4] = [
            ("Laser (Glow), the first stroke", settings::presets()[0].look),
            ("Solid, 2.1 s fade, smoothing 8", long),
            ("Highlighter", settings::presets()[1].look),
            ("Pen, kept", pen),
        ];
        let hwnd = hidden_window(x, y, width, height);
        let gfx = Graphics::new(hwnd, width as u32, height as u32).unwrap();
        let mut app = App::new(hwnd, gfx, (x, y), (width as u32, height as u32));
        app.live_cursor = hand_cursor;
        while app.warm() {}
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        crate::boost_compositor(true);
        let waitable = app.waitable().unwrap();
        app.presented = false;
        app.render_now();
        let mut msg = MSG::default();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        for (label, look) in scenarios {
            app.strokes.clear();
            let started = Instant::now();
            let (hx, hy) = hand_at(0.0);
            app.strokes.push(Drawn::new(1, Stroke::new(Point { x: hx - x as f32, y: hy - y as f32, at: started }, look, 1.5)));
            app.drawing = true;
            app.frame_cost = 0.0;
            // Each frame's present number and the clock as it started; each present shown and
            // the clock at the refresh that showed it.
            let (mut gaps, mut costs, mut starts, mut shown) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let (mut fed, mut last, mut second) = (0u64, Instant::now(), 1);
            while started.elapsed().as_secs_f32() < 3.0 {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
                WaitForSingleObject(waitable, 100);
                gaps.push(last.elapsed());
                last = Instant::now();
                if let Some(delay) = app.frame_delay() {
                    let _ = SetWaitableTimer(pacer, &-((delay * 1e7) as i64), 0, None, None, false);
                    WaitForSingleObject(pacer, (delay * 1000.0) as u32 + 5);
                }
                // Every sample since the last frame, as the mouse history brings them.
                let upto = started.elapsed().as_millis() as u64;
                let stroke = &mut app.strokes[0].stroke;
                while fed < upto {
                    fed += 1;
                    let (hx, hy) = hand_at(fed as f32 / 1000.0);
                    let (px, py) = (hx - x as f32, hy - y as f32);
                    if stroke.points.last().is_none_or(|p| (px - p.x).hypot(py - p.y) >= crate::stroke::MIN_STEP * stroke.scale) {
                        stroke.points.push(Point { x: px, y: py, at: started + Duration::from_millis(fed) });
                    }
                }
                let (hx, hy) = hand_at(upto as f32 / 1000.0);
                HAND.store(((hx as i32 as u32 as u64) << 32) | hy as i32 as u32 as u64, std::sync::atomic::Ordering::Relaxed);
                let gfx = app.gfx.as_ref().unwrap();
                let (now, ticks) = gfx.clock();
                second = ticks;
                shown.extend(gfx.presents().1);
                let begun = Instant::now();
                app.dirty = true;
                app.render();
                costs.push(begun.elapsed());
                starts.push((app.gfx.as_ref().unwrap().presents().0, now));
            }
            app.drawing = false;
            let period = app.gfx.as_ref().unwrap().next_frame().map_or(1.0 / 60.0, |(_, period)| period);
            gaps.remove(0);
            let late = gaps.iter().filter(|gap| gap.as_secs_f32() > period * 1.5).count();
            let mut latency: Vec<f64> = shown
                .iter()
                .filter_map(|&(count, at)| starts.iter().find(|start| start.0 == count).map(|start| (at - start.1) as f64 * 1000.0 / second as f64))
                .collect();
            latency.sort_by(f64::total_cmp);
            latency.dedup();
            costs.sort();
            let at = |times: &[Duration], percent: usize| ms(times[(times.len() * percent / 100).min(times.len() - 1)]);
            eprintln!(
                "{label:<32} {:>4.0} fps, {late} late; cpu median {:.3} ms, p99 {:.3}, worst {:.3}; start to shown median {:.2} ms, p90 {:.2}",
                (gaps.len() + 1) as f64 / 3.0,
                at(&costs, 50),
                at(&costs, 99),
                ms(costs[costs.len() - 1]),
                latency.get(latency.len() / 2).copied().unwrap_or(f64::NAN),
                latency.get(latency.len() * 9 / 10).copied().unwrap_or(f64::NAN),
            );
        }
        crate::boost_compositor(false);
        app.strokes.clear();
        app.gfx = None;
        let _ = DestroyWindow(hwnd);
    }
}

/// Process memory and video memory at each stage of the overlay's life, to see what the GPU
/// objects and the ink layer cost. `cargo test --release memory_report -- --ignored --nocapture`
#[test]
#[ignore = "prints memory use"]
fn memory_report() {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let show = |label: &str, app: Option<&App>| {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let _ = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS, size) };
        let mb = |bytes: u64| bytes as f64 / 1048576.0;
        let video = app.and_then(|app| app.gfx.as_ref()).map(Graphics::video_memory);
        let video = video.map_or(String::new(), |(local, shared)| format!("   video {:6.1} MB   shared {:5.1} MB", mb(local), mb(shared)));
        eprintln!("{label:<36} private {:6.1} MB{video}", mb(counters.PrivateUsage as u64));
    };
    show("before any GPU objects", None);
    unsafe {
        let (x, y, width, height) = virtual_screen();
        eprintln!("virtual screen {width} x {height}");
        let hwnd = hidden_window(x, y, width, height);
        let gfx = Graphics::new(hwnd, width as u32, height as u32).unwrap();
        let mut app = App::new(hwnd, gfx, (x, y), (width as u32, height as u32));
        app.live_cursor = || None;
        app.presented = false;
        app.render_now();
        app.render_now();
        show("shown, nothing drawn", Some(&app));
        let mut ink = settings::get().look;
        ink.ink = true;
        let now = Instant::now();
        let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, ink, 1.0);
        stroke.points = (0..800).map(|i| { let t = i as f32 * 0.01; Point { x: 900.0 + 300.0 * t.sin(), y: 900.0 + 300.0 * t.cos(), at: now } }).collect();
        app.strokes.push(Drawn::new(1, stroke));
        app.dirty = true;
        app.render_now();
        app.render_now();
        show("a kept stroke 700 px across, baked", Some(&app));
        app.strokes.clear();
        app.rebake();
        app.render_now();
        app.gfx.as_mut().unwrap().trim().unwrap();
        app.scratch.finish_frame();
        show("hidden and trimmed", Some(&app));
    }
}
