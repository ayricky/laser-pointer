//! The overlay's Direct2D device, its flip-model swap chain and DirectComposition visual, the
//! layer kept strokes are baked into, and rasterising SVGs into icons and cursors.

use windows::core::{w, Interface, Result, HRESULT};
use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F,
    D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1DeviceContext5, ID2D1Factory1,
    ID2D1SolidColorBrush, D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_MAP_OPTIONS_READ, D2D1_ROUNDED_RECT,
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
    target: Option<ID2D1Bitmap1>,
    /// Kept strokes, drawn once and copied into each frame; created on first use.
    ink: Option<ID2D1Bitmap1>,
    pub brush: ID2D1SolidColorBrush,
    dwrite: IDWriteFactory,
    dxgi: IDXGIDevice,
    size: (u32, u32),
    pub trimmed: bool,
    // Kept alive for the lifetime of the window; dropping them detaches the visual tree.
    _composition: (
        IDCompositionDevice,
        IDCompositionTarget,
        IDCompositionVisual,
    ),
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

fn bitmap_properties(options: D2D1_BITMAP_OPTIONS) -> D2D1_BITMAP_PROPERTIES1 {
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
            target: None,
            ink: None,
            brush,
            dwrite,
            dxgi,
            size: (width, height),
            trimmed: false,
            _composition: (composition, target, visual),
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
        self.ink = None;
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
        self.ink = None;
        self.resize_buffers(TRIMMED, TRIMMED)?;
        self.trimmed = true;
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

    /// Draws into the kept-stroke layer, creating it the first time.
    pub unsafe fn bake(&mut self, draw: impl FnOnce(&Self)) -> Result<()> {
        if self.ink.is_none() {
            let (width, height) = self.size;
            self.ink = Some(self.dc.CreateBitmap(
                D2D_SIZE_U { width, height },
                None,
                0,
                &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET),
            )?);
            self.retarget(|gfx| unsafe { gfx.dc.Clear(Some(&D2D1_COLOR_F::default())) })?;
        }
        self.retarget(draw)
    }

    unsafe fn retarget(&self, draw: impl FnOnce(&Self)) -> Result<()> {
        let Some(ink) = &self.ink else { return Ok(()) };
        self.dc.SetTarget(ink);
        self.dc.BeginDraw();
        draw(self);
        let drawn = self.dc.EndDraw(None, None);
        if let Some(target) = &self.target {
            self.dc.SetTarget(target);
        }
        drawn
    }

    pub fn drop_ink(&mut self) {
        self.ink = None;
    }

    /// Copies the kept strokes under `area` into the frame being drawn.
    pub unsafe fn draw_ink(&self, area: D2D_RECT_F) {
        if let Some(ink) = &self.ink {
            self.dc.DrawBitmap(
                ink,
                Some(&area),
                1.0,
                windows::Win32::Graphics::Direct2D::D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                Some(&area),
                None,
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
            for (from, to) in source.chunks_exact(4).zip(destination.chunks_exact_mut(4)) {
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
