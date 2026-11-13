//! Direct2D drawing into per-pixel-alpha layered windows.
//!
//! Each window owns a `Surface`: a 32-bit DIB that Direct2D draws into and
//! `UpdateLayeredWindow` shows. Everything is drawn in DIPs and scaled by the
//! monitor's DPI.

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::size_of;

use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

use super::theme;

const LOGO_PNG: &[u8] = include_bytes!("../../Assets/ScrollifyLOGO.png");

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn inset(&self, d: f32) -> Self {
        Self::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }

    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextStyle {
    /// Font size in tenths of a DIP, so the style can be a map key.
    pub size_tenths: u32,
    pub weight: i32,
    pub align: Align,
}

impl TextStyle {
    pub const fn new(size: f32, weight: i32, align: Align) -> Self {
        Self {
            size_tenths: (size * 10.0) as u32,
            weight,
            align,
        }
    }
}

/// Factories shared by every window. Lives on the UI thread.
pub struct Graphics {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    logo: Option<IWICBitmapSource>,
    formats: RefCell<HashMap<TextStyle, IDWriteTextFormat>>,
}

impl Graphics {
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            let d2d = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let logo = decode_png(LOGO_PNG).ok();
            Ok(Self {
                d2d,
                dwrite,
                logo,
                formats: RefCell::new(HashMap::new()),
            })
        }
    }

    fn format(&self, style: TextStyle) -> Option<IDWriteTextFormat> {
        if let Some(format) = self.formats.borrow().get(&style) {
            return Some(format.clone());
        }
        let format = unsafe {
            let format = self
                .dwrite
                .CreateTextFormat(
                    &HSTRING::from(theme::FONT),
                    None,
                    DWRITE_FONT_WEIGHT(style.weight),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size_tenths as f32 / 10.0,
                    &HSTRING::from("en-us"),
                )
                .ok()?;
            let align: DWRITE_TEXT_ALIGNMENT = match style.align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            };
            let _ = format.SetTextAlignment(align);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                ..Default::default()
            };
            if let Ok(sign) = self.dwrite.CreateEllipsisTrimmingSign(&format) {
                let _ = format.SetTrimming(&trimming, &sign);
            }
            format
        };
        self.formats.borrow_mut().insert(style, format.clone());
        Some(format)
    }
}

fn decode_png(bytes: &[u8]) -> windows::core::Result<IWICBitmapSource> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = wic.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let decoder =
            wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)?;
        let frame = decoder.GetFrame(0)?;
        let converter = wic.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeMedianCut,
        )?;
        converter.cast()
    }
}

/// Off-screen pixels for one layered window.
pub struct Surface {
    target: Option<Target>,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    px_size: (i32, i32),
}

struct Target {
    rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    logo: Option<ID2D1Bitmap>,
}

impl Surface {
    pub fn new() -> Self {
        Self {
            target: None,
            dc: unsafe { CreateCompatibleDC(HDC::default()) },
            bitmap: HBITMAP::default(),
            old: HGDIOBJ::default(),
            px_size: (0, 0),
        }
    }

    /// Draws a `w`×`h` DIP frame at `scale` and shows it, fully opaque, on
    /// `hwnd` at `pos` (physical pixels).
    pub fn draw(
        &mut self,
        gfx: &Graphics,
        hwnd: HWND,
        pos: POINT,
        (w, h): (f32, f32),
        scale: f32,
        paint: impl FnOnce(&mut Canvas),
    ) {
        let px = ((w * scale).ceil() as i32, (h * scale).ceil() as i32);
        if px != self.px_size && self.resize(px).is_err() {
            return;
        }
        if self.target.is_none() {
            self.target = create_target(gfx).ok();
        }
        let Some(target) = self.target.as_mut() else {
            return;
        };

        let ok = unsafe {
            let rect = RECT {
                left: 0,
                top: 0,
                right: px.0,
                bottom: px.1,
            };
            if target.rt.BindDC(self.dc, &rect).is_err() {
                return;
            }
            target.rt.SetDpi(96.0 * scale, 96.0 * scale);
            target.rt.BeginDraw();
            target.rt.Clear(Some(&theme::rgba(0, 0.0)));
            if target.logo.is_none() {
                if let Some(source) = &gfx.logo {
                    target.logo = target.rt.CreateBitmapFromWicBitmap(source, None).ok();
                }
            }
            let mut canvas = Canvas { gfx, target };
            paint(&mut canvas);
            target.rt.EndDraw(None, None).is_ok()
        };
        if !ok {
            // Device lost or similar: rebuild on the next frame.
            self.target = None;
        }
        self.present(hwnd, pos, 255);
    }

    /// Shows the last frame again with a new position or opacity.
    pub fn present(&self, hwnd: HWND, pos: POINT, alpha: u8) {
        let size = SIZE {
            cx: self.px_size.0,
            cy: self.px_size.1,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                HDC::default(),
                Some(&pos),
                Some(&size),
                self.dc,
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    fn resize(&mut self, (w, h): (i32, i32)) -> windows::core::Result<()> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w.max(1),
                biHeight: -h.max(1),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(self.dc, &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
        self.release_bitmap();
        self.old = unsafe { SelectObject(self.dc, bitmap) };
        self.bitmap = bitmap;
        self.px_size = (w, h);
        Ok(())
    }

    fn release_bitmap(&mut self) {
        unsafe {
            if !self.bitmap.is_invalid() {
                SelectObject(self.dc, self.old);
                let _ = DeleteObject(self.bitmap);
                self.bitmap = HBITMAP::default();
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.release_bitmap();
        unsafe {
            let _ = DeleteDC(self.dc);
        }
    }
}

fn create_target(gfx: &Graphics) -> windows::core::Result<Target> {
    unsafe {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let rt = gfx.d2d.CreateDCRenderTarget(&props)?;
        // ClearType needs an opaque background; grayscale works with alpha.
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let brush = rt.CreateSolidColorBrush(&theme::TEXT, None)?;
        Ok(Target {
            rt,
            brush,
            logo: None,
        })
    }
}

/// Drawing commands for one frame, in DIPs.
pub struct Canvas<'a> {
    gfx: &'a Graphics,
    target: &'a mut Target,
}

impl Canvas<'_> {
    fn brush(&self, color: D2D1_COLOR_F) -> &ID2D1SolidColorBrush {
        unsafe { self.target.brush.SetColor(&color) };
        &self.target.brush
    }

    pub fn fill_round(&mut self, r: Rect, radius: f32, color: D2D1_COLOR_F) {
        let shape = rounded(r, radius);
        unsafe {
            self.target
                .rt
                .FillRoundedRectangle(&shape, self.brush(color))
        };
    }

    pub fn stroke_round(&mut self, r: Rect, radius: f32, color: D2D1_COLOR_F, width: f32) {
        // Keep the stroke inside the rectangle.
        let shape = rounded(r.inset(width / 2.0), radius);
        unsafe {
            self.target
                .rt
                .DrawRoundedRectangle(&shape, self.brush(color), width, None)
        };
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: D2D1_COLOR_F) {
        let shape = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: cx, y: cy },
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.target.rt.FillEllipse(&shape, self.brush(color)) };
    }

    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), color: D2D1_COLOR_F, width: f32) {
        unsafe {
            self.target.rt.DrawLine(
                D2D_POINT_2F {
                    x: from.0,
                    y: from.1,
                },
                D2D_POINT_2F { x: to.0, y: to.1 },
                self.brush(color),
                width,
                None,
            )
        };
    }

    pub fn text(&mut self, text: &str, r: Rect, style: TextStyle, color: D2D1_COLOR_F) {
        let Some(format) = self.gfx.format(style) else {
            return;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.target.rt.DrawText(
                &wide,
                &format,
                &r.d2d(),
                self.brush(color),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }

    pub fn logo(&mut self, r: Rect) {
        if let Some(logo) = &self.target.logo {
            unsafe {
                self.target.rt.DrawBitmap(
                    logo,
                    Some(&r.d2d()),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                )
            };
        }
    }
}

fn rounded(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: r.d2d(),
        radiusX: radius,
        radiusY: radius,
    }
}
