//! The overlay's Direct2D device, its flip-model swap chain and DirectComposition visual, the
//! layer kept strokes are baked into, and rasterising SVGs into icons and cursors.

use windows::core::{w, Interface, Result, HRESULT};
use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F,
    D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1DeviceContext1, ID2D1DeviceContext5, ID2D1Factory1,
    ID2D1SolidColorBrush, D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_MAP_OPTIONS_READ, D2D1_ROUNDED_RECT, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_ANTIALIAS_MODE_ALIASED,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextLayout, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_TEXT_METRICS,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIDevice, IDXGIDevice3, IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
    IDXGISwapChain2, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_DEVICE_REMOVED,
    DXGI_ERROR_DEVICE_RESET, DXGI_PRESENT, DXGI_PRESENT_PARAMETERS, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};
use windows_numerics::{Matrix3x2, Vector2};

use crate::stroke::{rgba, Bounds};

/// Direct2D's "the device went away" (D2DERR_RECREATE_TARGET).
const RECREATE_TARGET: HRESULT = HRESULT(0x8899_000C_u32 as i32);

/// Buffers shrink to this while nothing is on screen, to hand the memory back.
const TRIMMED: u32 = 8;

/// Kept strokes are baked into tiles this many pixels square, made only where there is ink: a
/// layer the size of the desktop would hold over a hundred megabytes for a few strokes.
const TILE: u32 = 512;

/// Whether an error means the GPU was reset or removed, so everything must be rebuilt.
pub fn device_lost(error: &windows::core::Error) -> bool {
    [RECREATE_TARGET, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET].contains(&error.code())
}

pub struct Toast {
    pub layout: IDWriteTextLayout,
    pub width: f32,
    pub height: f32,
    pub centre: f32,
    pub bottom: f32,
    pub scale: f32,
}

/// A GDI bitmap deleted however the function using it returns.
struct OwnedBitmap(HBITMAP);

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

/// The swap chain's frame-latency waitable, closed with the swap chain.
struct Waitable(HANDLE);

impl Drop for Waitable {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub struct Graphics {
    swap_chain: IDXGISwapChain1,
    waitable: Waitable,
    pub factory: ID2D1Factory1,
    pub dc: ID2D1DeviceContext,
    /// The same context, for kept strokes' tessellated pieces.
    pub dc1: ID2D1DeviceContext1,
    target: Option<ID2D1Bitmap1>,
    /// Kept strokes, drawn once and copied into each frame, as tiles made where they are needed.
    ink: Tiles,
    /// The ink layer as it was when everything in it was cleared, fading out.
    fading: Tiles,
    pub brush: ID2D1SolidColorBrush,
    dwrite: IDWriteFactory,
    dxgi: IDXGIDevice,
    size: (u32, u32),
    pub trimmed: bool,
    // Kept alive for the lifetime of the window; dropping them detaches the visual tree.
    composition: (
        IDCompositionDevice,
        IDCompositionTarget,
        IDCompositionVisual,
    ),
}

/// A layer the size of the screen, row by row in tiles, each made the first time something is
/// drawn into it.
#[derive(Default)]
struct Tiles(Vec<Option<ID2D1Bitmap1>>);

/// The tiles of a `width` by `height` layer that `area` touches: each one's index and where it
/// lies.
fn tiles_under((width, height): (u32, u32), area: Bounds) -> impl Iterator<Item = (usize, D2D_RECT_F)> {
    let columns = width.div_ceil(TILE);
    let span = |from: f32, to: f32, extent: u32| {
        let first = (from.max(0.0) as u32 / TILE).min(extent.div_ceil(TILE));
        let last = (to.min(extent as f32).ceil().max(0.0) as u32).div_ceil(TILE);
        first..last.max(first)
    };
    let (rows, across) = (span(area.top, area.bottom, height), span(area.left, area.right, width));
    rows.flat_map(move |row| across.clone().map(move |column| (row, column))).map(move |(row, column)| {
        let (left, top) = (column * TILE, row * TILE);
        let rect = D2D_RECT_F {
            left: left as f32,
            top: top as f32,
            right: (left + TILE).min(width) as f32,
            bottom: (top + TILE).min(height) as f32,
        };
        ((row * columns + column) as usize, rect)
    })
}

/// Where `a` and `b` overlap, if they do.
fn overlap(a: D2D_RECT_F, b: D2D_RECT_F) -> Option<D2D_RECT_F> {
    let both = D2D_RECT_F {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (both.left < both.right && both.top < both.bottom).then_some(both)
}

/// `rect` moved so `origin` is at 0, 0.
fn local(rect: D2D_RECT_F, origin: D2D_RECT_F) -> D2D_RECT_F {
    D2D_RECT_F {
        left: rect.left - origin.left,
        top: rect.top - origin.top,
        right: rect.right - origin.left,
        bottom: rect.bottom - origin.top,
    }
}

impl Tiles {
    /// The tiles there are under `area`, each with where it lies and the part of `area` on it.
    fn under(&self, size: (u32, u32), area: D2D_RECT_F) -> impl Iterator<Item = (&ID2D1Bitmap1, D2D_RECT_F, D2D_RECT_F)> {
        let bounds = Bounds { left: area.left, top: area.top, right: area.right, bottom: area.bottom };
        tiles_under(size, bounds).filter_map(move |(index, rect)| {
            let tile = self.0.get(index)?.as_ref()?;
            Some((tile, rect, overlap(rect, area)?))
        })
    }

    fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }
}

unsafe fn create_device(driver: D3D_DRIVER_TYPE) -> Result<IDXGIDevice> {
    let mut d3d: Option<ID3D11Device> = None;
    D3D11CreateDevice(
        None,
        driver,
        Default::default(),
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        None,
        D3D11_SDK_VERSION,
        Some(&mut d3d),
        None,
        None,
    )?;
    d3d.ok_or_else(|| windows::core::Error::from(E_FAIL))?.cast()
}

pub(crate) fn bitmap_properties(options: D2D1_BITMAP_OPTIONS) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: options,
        ..Default::default()
    }
}

pub fn rect_f(bounds: Bounds) -> D2D_RECT_F {
    D2D_RECT_F {
        left: bounds.left.floor(),
        top: bounds.top.floor(),
        right: bounds.right.ceil(),
        bottom: bounds.bottom.ceil(),
    }
}

impl Graphics {
    /// On a machine without a usable GPU (some VMs and remote sessions), falls back to WARP.
    pub unsafe fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        let dxgi = create_device(D3D_DRIVER_TYPE_HARDWARE)
            .or_else(|_| create_device(D3D_DRIVER_TYPE_WARP))?;
        let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let swap_chain = factory.CreateSwapChainForComposition(
            &dxgi,
            &DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
                ..Default::default()
            },
            None,
        )?;
        let latency: IDXGISwapChain2 = swap_chain.cast()?;
        latency.SetMaximumFrameLatency(1)?;
        let waitable = Waitable(latency.GetFrameLatencyWaitableObject());

        let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dc = d2d
            .CreateDevice(&dxgi)?
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let dc1: ID2D1DeviceContext1 = dc.cast()?;
        let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let composition: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
        let target = composition.CreateTargetForHwnd(hwnd, true)?;
        let visual = composition.CreateVisual()?;
        visual.SetContent(&swap_chain)?;
        target.SetRoot(&visual)?;
        composition.Commit()?;

        let mut gfx = Self {
            swap_chain,
            waitable,
            factory: d2d,
            dc,
            dc1,
            target: None,
            ink: Tiles::default(),
            fading: Tiles::default(),
            brush,
            dwrite,
            dxgi,
            size: (width, height),
            trimmed: false,
            composition: (composition, target, visual),
        };
        gfx.bind_target()?;
        Ok(gfx)
    }

    pub fn waitable(&self) -> HANDLE {
        self.waitable.0
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// How long until Windows next composes the screen, and how long apart its frames are, in
    /// seconds.
    pub fn next_frame(&self) -> Option<(f32, f32)> {
        let stats = unsafe { self.composition.0.GetFrameStatistics() }.ok()?;
        let rate = stats.currentCompositionRate;
        if rate.Numerator == 0 || rate.Denominator == 0 || stats.timeFrequency <= 0 {
            return None;
        }
        let period = rate.Denominator as f32 / rate.Numerator as f32;
        let until = (stats.nextEstimatedFrameTime - stats.currentTime) as f32 / stats.timeFrequency as f32;
        Some((until, period))
    }

    unsafe fn bind_target(&mut self) -> Result<()> {
        let surface: IDXGISurface = self.swap_chain.GetBuffer(0)?;
        let target = self.dc.CreateBitmapFromDxgiSurface(
            &surface,
            Some(&bitmap_properties(
                D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            )),
        )?;
        self.dc.SetTarget(&target);
        self.target = Some(target);
        Ok(())
    }

    unsafe fn resize_buffers(&mut self, width: u32, height: u32) -> Result<()> {
        self.dc.SetTarget(None);
        self.target = None;
        self.swap_chain.ResizeBuffers(
            2,
            width,
            height,
            DXGI_FORMAT_UNKNOWN,
            DXGI_SWAP_CHAIN_FLAG(DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0),
        )?;
        self.bind_target()
    }

    /// The kept-stroke layer is dropped too; the caller re-bakes it.
    pub unsafe fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.ink = Tiles::default();
        self.fading = Tiles::default();
        self.size = (width, height);
        if self.trimmed {
            return Ok(());
        }
        self.resize_buffers(width, height)
    }

    /// Hands back the screen-sized buffers while nothing is shown. Only valid with no kept
    /// strokes, since their layer goes too.
    pub unsafe fn trim(&mut self) -> Result<()> {
        if self.trimmed {
            return Ok(());
        }
        self.ink = Tiles::default();
        self.resize_buffers(TRIMMED, TRIMMED)?;
        self.trimmed = true;
        // Direct2D keeps textures from large layers and fills cached; let them go too.
        if let Ok(device) = self.dc.GetDevice() {
            device.ClearResources(0);
        }
        if let Ok(device) = self.dxgi.cast::<IDXGIDevice3>() {
            device.Trim();
        }
        Ok(())
    }

    pub unsafe fn untrim(&mut self) -> Result<()> {
        if !self.trimmed {
            return Ok(());
        }
        self.trimmed = false;
        let (width, height) = self.size;
        self.resize_buffers(width, height)
    }

    /// Draws into the tiles of the kept-stroke layer under `areas`, making those there aren't yet
    /// if `make`, and otherwise leaving out the ones with nothing in them; `erase` is cleared in
    /// each first. `draw` runs once, in screen coordinates, into a bitmap just big enough, which
    /// is then laid over each tile: drawing into every tile would tessellate and blur a stroke
    /// once for each.
    pub unsafe fn bake(&mut self, areas: impl Iterator<Item = Bounds>, make: bool, erase: Option<Bounds>, draw: impl FnOnce(&Self)) -> Result<()> {
        let count = (self.size.0.div_ceil(TILE) * self.size.1.div_ceil(TILE)) as usize;
        self.ink.0.resize(count, None);
        let mut chosen: Vec<Option<(D2D_RECT_F, bool)>> = vec![None; count];
        let mut extent: Option<Bounds> = None;
        for area in areas {
            for (index, rect) in tiles_under(self.size, area) {
                chosen[index] = Some((rect, false));
            }
            extent = crate::stroke::union(extent, Some(area));
        }
        for (tile, choice) in self.ink.0.iter_mut().zip(&mut chosen) {
            let Some((rect, fresh)) = choice else { continue };
            if tile.is_none() && make {
                let size = D2D_SIZE_U { width: (rect.right - rect.left) as u32, height: (rect.bottom - rect.top) as u32 };
                *tile = Some(self.dc.CreateBitmap(size, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET))?);
                *fresh = true;
            }
        }
        let screen = D2D_RECT_F { left: 0.0, top: 0.0, right: self.size.0 as f32, bottom: self.size.1 as f32 };
        let Some(extent) = extent.and_then(|extent| overlap(rect_f(extent), screen)) else { return Ok(()) };
        let size = D2D_SIZE_U { width: (extent.right - extent.left) as u32, height: (extent.bottom - extent.top) as u32 };
        let drawing = self.dc.CreateBitmap(size, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET))?;
        self.dc.SetTarget(&drawing);
        self.dc.BeginDraw();
        self.dc.Clear(Some(&D2D1_COLOR_F::default()));
        self.dc.SetTransform(&Matrix3x2::translation(-extent.left, -extent.top));
        draw(self);
        self.dc.SetTransform(&Matrix3x2::identity());
        let mut drawn = self.dc.EndDraw(None, None);
        for (tile, choice) in self.ink.0.iter().zip(&chosen) {
            let (Some(tile), Some((rect, fresh)), Ok(())) = (tile, choice, &drawn) else { continue };
            self.dc.SetTarget(tile);
            self.dc.BeginDraw();
            if *fresh {
                self.dc.Clear(Some(&D2D1_COLOR_F::default()));
            } else if let Some(erase) = erase.and_then(|erase| overlap(rect_f(erase), *rect)) {
                self.dc.PushAxisAlignedClip(&local(erase, *rect), D2D1_ANTIALIAS_MODE_ALIASED);
                self.dc.Clear(Some(&D2D1_COLOR_F::default()));
                self.dc.PopAxisAlignedClip();
            }
            if let Some(part) = overlap(*rect, extent) {
                self.dc.DrawBitmap(&drawing, Some(&local(part, *rect)), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, Some(&local(part, extent)), None);
            }
            drawn = self.dc.EndDraw(None, None);
        }
        if let Some(target) = &self.target {
            self.dc.SetTarget(target);
        }
        drawn
    }

    /// Empties the kept-stroke layer, handing its tiles back.
    pub fn clear_ink(&mut self) {
        self.ink = Tiles::default();
    }

    /// Sets the whole ink layer aside to fade out, leaving none: the next kept stroke starts a
    /// new one.
    pub fn fade_ink(&mut self) {
        self.fading = std::mem::take(&mut self.ink);
    }

    pub fn end_fade(&mut self) {
        self.fading = Tiles::default();
    }

    /// Draws what is fading out under `area`, `opacity` times as opaque as it was.
    pub unsafe fn draw_fading(&self, area: D2D_RECT_F, opacity: f32) {
        for (tile, rect, part) in self.fading.under(self.size, area) {
            self.dc.DrawBitmap(tile, Some(&part), opacity, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, Some(&local(part, rect)), None);
        }
    }

    /// Starts `area` of the frame being drawn with the kept strokes under it, or with nothing.
    /// Each tile is drawn over the cleared area, which leaves exactly its pixels: copying would
    /// clear the whole clip again, and with it the tiles drawn before.
    pub unsafe fn start_with_ink(&self, area: D2D_RECT_F) {
        self.dc.Clear(Some(&D2D1_COLOR_F::default()));
        if self.ink.is_empty() {
            return;
        }
        for (tile, rect, part) in self.ink.under(self.size, area) {
            self.dc.DrawImage(
                tile,
                Some(&Vector2 { X: part.left, Y: part.top }),
                Some(&local(part, rect)),
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
        }
    }

    /// Presents with the changed rectangles, or the whole frame when `rects` is empty.
    pub unsafe fn present(&self, rects: &mut [RECT]) -> Result<()> {
        let parameters = DXGI_PRESENT_PARAMETERS {
            DirtyRectsCount: rects.len() as u32,
            pDirtyRects: if rects.is_empty() {
                std::ptr::null_mut()
            } else {
                rects.as_mut_ptr()
            },
            ..Default::default()
        };
        self.swap_chain
            .Present1(1, DXGI_PRESENT(0), &parameters)
            .ok()
    }

    /// For tests: the compositor's clock now, and its ticks a second.
    #[cfg(test)]
    pub fn clock(&self) -> (i64, i64) {
        let stats = unsafe { self.composition.0.GetFrameStatistics() }.unwrap_or_default();
        (stats.currentTime, stats.timeFrequency)
    }

    /// For tests: the number of the last present, and the newest one shown with the clock at
    /// the refresh that showed it.
    #[cfg(test)]
    pub fn presents(&self) -> (u32, Option<(u32, i64)>) {
        unsafe {
            let mut shown = windows::Win32::Graphics::Dxgi::DXGI_FRAME_STATISTICS::default();
            let shown = self.swap_chain.GetFrameStatistics(&mut shown).ok().map(|()| (shown.PresentCount, shown.SyncQPCTime));
            (self.swap_chain.GetLastPresentCount().unwrap_or(0), shown)
        }
    }

    /// For tests: the video memory this process has in use on the overlay's GPU, its own and
    /// shared with the system, in bytes.
    #[cfg(test)]
    pub fn video_memory(&self) -> (u64, u64) {
        use windows::Win32::Graphics::Dxgi::{
            IDXGIAdapter3, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
        };
        unsafe {
            let Ok(adapter) = self.dxgi.GetAdapter().and_then(|adapter| adapter.cast::<IDXGIAdapter3>()) else { return (0, 0) };
            let used = |group| {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                adapter.QueryVideoMemoryInfo(0, group, &mut info).map_or(0, |()| info.CurrentUsage)
            };
            (used(DXGI_MEMORY_SEGMENT_GROUP_LOCAL), used(DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL))
        }
    }

    /// For tests: the swap chain's back buffer, which holds the frame presented two before, as
    /// BGRA rows.
    #[cfg(test)]
    pub unsafe fn read_back(&self) -> Vec<u8> {
        use windows::Win32::Graphics::Direct3D11::{
            ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_STAGING,
        };
        let buffer: ID3D11Texture2D = self.swap_chain.GetBuffer(0).unwrap();
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        buffer.GetDesc(&mut desc);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let device = buffer.GetDevice().unwrap();
        let mut staging = None;
        device.CreateTexture2D(&desc, None, Some(&mut staging)).unwrap();
        let staging = staging.unwrap();
        let context = device.GetImmediateContext().unwrap();
        context.CopyResource(&staging, &buffer);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
        let mut out = Vec::with_capacity((desc.Width * desc.Height * 4) as usize);
        for row in 0..desc.Height as usize {
            out.extend_from_slice(std::slice::from_raw_parts(
                (mapped.pData as *const u8).add(row * mapped.RowPitch as usize),
                desc.Width as usize * 4,
            ));
        }
        context.Unmap(&staging, 0);
        out
    }

    /// A pill near the bottom of the screen, laid out at the DPI of the monitor it shows on.
    pub unsafe fn make_toast(&self, text: &str, centre: f32, bottom: f32, scale: f32) -> Option<Toast> {
        let format = self
            .dwrite
            .CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                15.0 * scale,
                w!("en-us"),
            )
            .ok()?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = self.dwrite.CreateTextLayout(&wide, &format, 4000.0, 400.0).ok()?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics).ok()?;
        Some(Toast {
            layout,
            width: metrics.width,
            height: metrics.height,
            centre,
            bottom,
            scale,
        })
    }

    pub fn toast_bounds(toast: &Toast) -> Bounds {
        let scale = toast.scale;
        let width = toast.width + 46.0 * scale;
        let height = toast.height + 18.0 * scale;
        Bounds {
            left: toast.centre - width / 2.0,
            top: toast.bottom - height,
            right: toast.centre + width / 2.0,
            bottom: toast.bottom,
        }
    }

    pub unsafe fn draw_toast(&self, toast: &Toast, color: u32, alpha: f32) {
        let scale = toast.scale;
        let (pad, dot, gap) = (18.0 * scale, 5.0 * scale, 10.0 * scale);
        let bounds = Self::toast_bounds(toast);
        let height = bounds.bottom - bounds.top;
        self.brush.SetColor(&rgba(0x1c1c22, 0.9 * alpha));
        self.dc.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: rect_f(bounds),
                radiusX: height / 2.0,
                radiusY: height / 2.0,
            },
            &self.brush,
        );
        self.brush.SetColor(&rgba(color, alpha));
        self.dc.FillEllipse(
            &crate::ellipse(bounds.left + pad + dot, bounds.top + height / 2.0, dot),
            &self.brush,
        );
        self.brush.SetColor(&rgba(0xffffff, alpha));
        self.dc.DrawTextLayout(
            Vector2 {
                X: bounds.left + pad + dot * 2.0 + gap,
                Y: bounds.top + (height - toast.height) / 2.0,
            },
            &toast.layout,
            &self.brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }

    /// Rasterises an SVG, scaled by `zoom` and turned by `degrees` about `hotspot` (in output
    /// pixels), so the cursor and tray icon stay sharp at any display scale and the cursor can
    /// animate.
    pub unsafe fn svg_icon(
        &self,
        svg: &str,
        size: u32,
        hotspot: Option<(f32, f32)>,
        zoom: f32,
        degrees: f32,
    ) -> Result<HICON> {
        let stream = SHCreateMemStream(Some(svg.as_bytes())).ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let dc5: ID2D1DeviceContext5 = self.dc.cast()?;
        let document = dc5.CreateSvgDocument(
            &stream,
            D2D_SIZE_F {
                width: size as f32,
                height: size as f32,
            },
        )?;
        let (x, y) = hotspot.unwrap_or((size as f32 / 2.0, size as f32 / 2.0));
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (m11, m12, m21, m22) = (zoom * cos, zoom * sin, -zoom * sin, zoom * cos);
        self.render_icon(size, hotspot, |dc| unsafe {
            dc.SetTransform(&Matrix3x2 {
                M11: m11,
                M12: m12,
                M21: m21,
                M22: m22,
                M31: x - (m11 * x + m21 * y),
                M32: y - (m12 * x + m22 * y),
            });
            dc5.DrawSvgDocument(&document);
            dc.SetTransform(&Matrix3x2::identity());
        })
    }

    /// Draws into a small bitmap that is then thrown away, to ready what the drawing needs.
    pub unsafe fn draw_aside(&self, draw: impl FnOnce(&Self)) -> Result<()> {
        let canvas = self.dc.CreateBitmap(D2D_SIZE_U { width: 64, height: 64 }, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET))?;
        self.dc.SetTarget(&canvas);
        self.dc.BeginDraw();
        draw(self);
        let drawn = self.dc.EndDraw(None, None);
        if let Some(target) = &self.target {
            self.dc.SetTarget(target);
        }
        drawn
    }

    /// Draws into an offscreen bitmap and turns it into an icon, or a cursor if given a hotspot.
    unsafe fn render_icon(
        &self,
        size: u32,
        hotspot: Option<(f32, f32)>,
        draw: impl FnOnce(&ID2D1DeviceContext),
    ) -> Result<HICON> {
        let extent = D2D_SIZE_U {
            width: size,
            height: size,
        };
        let canvas = self.dc.CreateBitmap(
            extent,
            None,
            0,
            &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET),
        )?;
        let readback = self.dc.CreateBitmap(
            extent,
            None,
            0,
            &bitmap_properties(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW),
        )?;
        self.dc.SetTarget(&canvas);
        self.dc.BeginDraw();
        self.dc.Clear(Some(&D2D1_COLOR_F::default()));
        draw(&self.dc);
        let drawn = self.dc.EndDraw(None, None);
        if let Some(target) = &self.target {
            self.dc.SetTarget(target);
        }
        drawn?;
        readback.CopyFromBitmap(None, &canvas, None)?;

        let header = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = OwnedBitmap(CreateDIBSection(
            None,
            &BITMAPINFO {
                bmiHeader: header,
                ..Default::default()
            },
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )?);
        let mapped = readback.Map(D2D1_MAP_OPTIONS_READ)?;
        let row_bytes = size as usize * 4;
        for row in 0..size as usize {
            let source =
                std::slice::from_raw_parts(mapped.bits.add(row * mapped.pitch as usize), row_bytes);
            let destination =
                std::slice::from_raw_parts_mut((bits as *mut u8).add(row * row_bytes), row_bytes);
            // Direct2D hands back premultiplied pixels; icon bitmaps want straight alpha, as in
            // Microsoft's alpha-blended cursor example, which sets alpha without scaling RGB.
            for (from, to) in source.as_chunks::<4>().0.iter().zip(destination.as_chunks_mut::<4>().0) {
                let alpha = from[3];
                for channel in 0..3 {
                    to[channel] = if alpha == 0 {
                        0
                    } else {
                        (u32::from(from[channel]) * 255 / u32::from(alpha)).min(255) as u8
                    };
                }
                to[3] = alpha;
            }
        }
        readback.Unmap()?;

        let mask = OwnedBitmap(CreateBitmap(size as i32, size as i32, 1, 1, None));
        let (x, y) = hotspot.unwrap_or_default();
        // The icon keeps its own copies of both bitmaps.
        CreateIconIndirect(&ICONINFO {
            fIcon: hotspot.is_none().into(),
            xHotspot: x.round() as u32,
            yHotspot: y.round() as u32,
            hbmMask: mask.0,
            hbmColor: color.0,
        })
    }
}
