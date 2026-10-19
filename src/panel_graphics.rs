//! A separate device keeps panel recovery independent of the overlay's ink caches.

use windows::core::{Interface, Result};
use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE, HWND};
use windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_IGNORE;
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DeviceContext, ID2D1Factory1, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, IDXGISwapChain2,
    DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

pub struct Surface {
    pub context: ID2D1DeviceContext,
    pub waitable: HANDLE,
    swap_chain: Option<IDXGISwapChain1>,
    immediate: ID3D11DeviceContext,
}

#[cfg(test)]
pub unsafe fn context() -> Result<ID2D1DeviceContext> {
    let device = crate::graphics_device()?;
    let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
    factory
        .CreateDevice(&device)?
        .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
}

impl Surface {
    pub unsafe fn new(hwnd: HWND, dpi: u32, width: u32, height: u32) -> Result<Self> {
        let device = crate::graphics_device()?;
        let immediate = device.cast::<ID3D11Device>()?.GetImmediateContext()?;
        let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let swap_chain = factory.CreateSwapChainForHwnd(
            &device,
            hwnd,
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
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
                ..Default::default()
            },
            None,
            None,
        )?;
        let latency: IDXGISwapChain2 = swap_chain.cast()?;
        latency.SetMaximumFrameLatency(1)?;
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let context = factory
            .CreateDevice(&device)?
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let surface: IDXGISurface = swap_chain.GetBuffer(0)?;
        let mut properties =
            crate::bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW);
        properties.pixelFormat.alphaMode = D2D1_ALPHA_MODE_IGNORE;
        properties.dpiX = dpi as f32;
        properties.dpiY = dpi as f32;
        let bitmap = context.CreateBitmapFromDxgiSurface(&surface, Some(&properties))?;
        context.SetTarget(&bitmap);
        context.SetDpi(dpi as f32, dpi as f32);
        let waitable = latency.GetFrameLatencyWaitableObject();
        if waitable.is_invalid() {
            return Err(E_FAIL.into());
        }
        Ok(Self {
            context,
            waitable,
            swap_chain: Some(swap_chain),
            immediate,
        })
    }

    pub unsafe fn present(&self) -> Result<()> {
        self.swap_chain
            .as_ref()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?
            .Present(1, DXGI_PRESENT(0))
            .ok()
    }

    pub unsafe fn resize(&self, dpi: u32, width: u32, height: u32) -> Result<()> {
        self.context.SetTarget(None);
        let swap_chain = self
            .swap_chain
            .as_ref()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        swap_chain.ResizeBuffers(
            2,
            width,
            height,
            DXGI_FORMAT_UNKNOWN,
            DXGI_SWAP_CHAIN_FLAG(DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0),
        )?;
        let surface: IDXGISurface = swap_chain.GetBuffer(0)?;
        let mut properties =
            crate::bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW);
        properties.pixelFormat.alphaMode = D2D1_ALPHA_MODE_IGNORE;
        properties.dpiX = dpi as f32;
        properties.dpiY = dpi as f32;
        let bitmap = self
            .context
            .CreateBitmapFromDxgiSurface(&surface, Some(&properties))?;
        self.context.SetTarget(&bitmap);
        self.context.SetDpi(dpi as f32, dpi as f32);
        Ok(())
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            self.context.SetTarget(None);
            self.swap_chain = None;
            self.immediate.ClearState();
            self.immediate.Flush();
            let _ = CloseHandle(self.waitable);
        }
    }
}
