//! For tests: draw with Direct2D into an offscreen bitmap and save it as a PNG, so the panel and
//! strokes can be looked at without opening a window.

use std::path::PathBuf;

use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_RENDER_TARGET_PROPERTIES,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppPBGRA, IWICBitmap,
    IWICBitmapFrameEncode, IWICImagingFactory, WICBitmapCacheOnLoad, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

pub fn init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub struct Canvas {
    pub factory: ID2D1Factory,
    pub target: ID2D1RenderTarget,
    bitmap: IWICBitmap,
    wic: IWICImagingFactory,
}

impl Canvas {
    pub fn new(width: u32, height: u32, dpi: u32) -> Self {
        unsafe {
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).unwrap();
            let bitmap = wic
                .CreateBitmap(width, height, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)
                .unwrap();
            let factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).unwrap();
            let target = factory
                .CreateWicBitmapRenderTarget(
                    &bitmap,
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: dpi as f32,
                        dpiY: dpi as f32,
                        ..Default::default()
                    },
                )
                .unwrap();
            Self {
                factory,
                target,
                bitmap,
                wic,
            }
        }
    }

    /// Saves to target-v2/snapshots/<name>.
    pub fn save(&self, name: &str) {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-v2").join("snapshots");
        std::fs::create_dir_all(&directory).unwrap();
        let path = HSTRING::from(directory.join(name).as_os_str());
        unsafe {
            let stream = self.wic.CreateStream().unwrap();
            stream.InitializeFromFilename(&path, GENERIC_WRITE.0).unwrap();
            let encoder = self.wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).unwrap();
            encoder.Initialize(&stream, WICBitmapEncoderNoCache).unwrap();
            let mut frame: Option<IWICBitmapFrameEncode> = None;
            encoder.CreateNewFrame(&mut frame, std::ptr::null_mut()).unwrap();
            let frame = frame.unwrap();
            frame.Initialize(None).unwrap();
            let (mut width, mut height) = (0, 0);
            self.bitmap.GetSize(&mut width, &mut height).unwrap();
            frame.SetSize(width, height).unwrap();
            let mut format = GUID_WICPixelFormat32bppPBGRA;
            frame.SetPixelFormat(&mut format).unwrap();
            frame.WriteSource(&self.bitmap.cast::<windows::Win32::Graphics::Imaging::IWICBitmapSource>().unwrap(), std::ptr::null()).unwrap();
            frame.Commit().unwrap();
            encoder.Commit().unwrap();
        }
    }
}
