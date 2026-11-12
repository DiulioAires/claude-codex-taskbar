//! Direct2D/DirectWrite drawing into a per-pixel-alpha layered window (mockup B).

use super::layout::{ItemMetrics, Layout, Mode};
use super::view::ItemView;
use crate::model::{Kind, bar_color};

mod logo_paths;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_FILL_MODE_ALTERNATE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1PathGeometry, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows::core::{Result, w};

// Sizes in DIPs, from mockup B.
const LABEL_SIZE: f32 = 9.5;
const VALUE_SIZE: f32 = 11.0;
const LABEL_LINE: f32 = 12.0;
const VALUE_LINE: f32 = 15.0;
const BAR_W: f32 = 30.0;
const BAR_H: f32 = 5.0;
const BAR_RADIUS: f32 = 2.5;
const BAR_GAP: f32 = 2.0;
const TICK_W: f32 = 1.0;
const TICK_OVERHANG: f32 = 1.0;
pub const GAP: f32 = 10.0;
pub const SEP_W: f32 = 1.0;
const SEP_H: f32 = 30.0;
const ELLIPSIS: &str = "…";
const LOGO_SIZE: f32 = 20.0;
const LOGO_GAP: f32 = 5.0;
const LOGO_INSET: f32 = 2.0;
const LOGO_WIDTH: f32 = LOGO_SIZE + 2.0 * LOGO_INSET + LOGO_GAP;

struct Palette {
    label: u32,
    value: u32,
    track: u32,
    separator: u32,
}

const DARK: Palette = Palette {
    label: 0xbbbbbb,
    value: 0xffffff,
    track: 0x333333,
    separator: 0x3a3a3a,
};
const LIGHT: Palette = Palette {
    label: 0x5f5f5f,
    value: 0x1a1a1a,
    track: 0xd0d0d0,
    separator: 0xc8c8c8,
};

fn color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: alpha,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// DIPs → physical pixels, rounded up.
pub fn to_px(dip: f32, dpi: u32) -> i32 {
    (dip * dpi as f32 / 96.0).ceil() as i32
}

fn to_dip(px: i32, dpi: u32) -> f32 {
    px as f32 * 96.0 / dpi as f32
}

/// The GDI bitmap and Direct2D target for one strip size, kept between frames (#86).
struct Surface {
    w: i32,
    h: i32,
    dpi: u32,
    mem: HDC,
    dib: HBITMAP,
    /// The bitmap `mem` came with, selected back before `mem` is deleted.
    old: HGDIOBJ,
    rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    #[cfg(test)]
    bits: *const u32,
}

impl Surface {
    fn new(d2d: &ID2D1Factory, w: i32, h: i32, dpi: u32) -> Result<Surface> {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            // Software rendering avoids loading the GPU driver (~60 MB) for a tiny strip.
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi as f32,
            dpiY: dpi as f32,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        // SAFETY: a null DC means "compatible with the screen"; every object created here is
        // released on the error paths below or by `Drop`.
        unsafe {
            let mem = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let dib = match CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(dib) => dib,
                Err(err) => {
                    let _ = DeleteDC(mem);
                    return Err(err);
                }
            };
            let old = SelectObject(mem, HGDIOBJ(dib.0));
            let target = d2d.CreateDCRenderTarget(&props).and_then(|rt| {
                let area = RECT {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                };
                rt.BindDC(mem, &area)?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                let brush = rt.CreateSolidColorBrush(&color(0, 1.0), None)?;
                Ok((rt, brush))
            });
            match target {
                Ok((rt, brush)) => Ok(Surface {
                    w,
                    h,
                    dpi,
                    mem,
                    dib,
                    old,
                    rt,
                    brush,
                    #[cfg(test)]
                    bits: bits as *const u32,
                }),
                Err(err) => {
                    SelectObject(mem, old);
                    let _ = DeleteObject(HGDIOBJ(dib.0));
                    let _ = DeleteDC(mem);
                    Err(err)
                }
            }
        }
    }

    fn fits(&self, w: i32, h: i32, dpi: u32) -> bool {
        (self.w, self.h, self.dpi) == (w, h, dpi)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: the objects were created by `new` and are released exactly once.
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(HGDIOBJ(self.dib.0));
            let _ = DeleteDC(self.mem);
        }
    }
}

pub struct Renderer {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    label_fmt: IDWriteTextFormat,
    value_fmt: IDWriteTextFormat,
    surface: Option<Surface>,
    claude_logo: ID2D1PathGeometry,
    codex_logo: ID2D1PathGeometry,
}

fn text_format(
    dw: &IDWriteFactory,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
) -> Result<IDWriteTextFormat> {
    // SAFETY: plain factory call; DirectWrite falls back if the family is missing.
    let f = unsafe {
        dw.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!(""),
        )?
    };
    // SAFETY: format is valid.
    unsafe {
        f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
    }
    Ok(f)
}

/// Keep the original label for tooltips and clipboard; shorten only the painted row.
fn paint_label(item: &ItemView) -> &str {
    if item.kind == Kind::Hardware {
        &item.label
    } else {
        item.label
            .strip_prefix("Claude ")
            .or_else(|| item.label.strip_prefix("Codex "))
            .unwrap_or(&item.label)
    }
}

fn first_provider(items: &[ItemView], index: usize) -> bool {
    items[index].kind != Kind::Hardware
        && !items[..index]
            .iter()
            .any(|item| item.kind == items[index].kind)
}

fn logo_geometry(factory: &ID2D1Factory, contours: &[&[[f32; 2]]]) -> Result<ID2D1PathGeometry> {
    // SAFETY: owned COM geometry and sink; the point conversion preserves the documented
    // two-f32 Vector2 ABI used by Direct2D. Contours are embedded, closed and nonempty.
    unsafe {
        let geometry = factory.CreatePathGeometry()?;
        let sink = geometry.Open()?;
        sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
        for contour in contours {
            let points: Vec<_> = contour
                .iter()
                .map(|&point| std::mem::transmute::<[f32; 2], _>(point))
                .collect();
            sink.BeginFigure(points[0], D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLines(&points[1..]);
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        }
        sink.Close()?;
        Ok(geometry)
    }
}

impl Renderer {
    pub fn new() -> Result<Renderer> {
        // SAFETY: factory creation has no preconditions.
        let d2d: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let label_fmt = text_format(&dwrite, LABEL_SIZE, DWRITE_FONT_WEIGHT_NORMAL)?;
        let value_fmt = text_format(&dwrite, VALUE_SIZE, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
        let claude_logo = logo_geometry(&d2d, logo_paths::CLAUDE)?;
        let codex_logo = logo_geometry(&d2d, logo_paths::CODEX)?;
        Ok(Renderer {
            d2d,
            dwrite,
            label_fmt,
            value_fmt,
            surface: None,
            claude_logo,
            codex_logo,
        })
    }

    fn text_width(&self, text: &str, fmt: &IDWriteTextFormat) -> f32 {
        let mut m = DWRITE_TEXT_METRICS::default();
        // SAFETY: valid string and format; metrics is an out value.
        let ok = unsafe {
            self.dwrite
                .CreateTextLayout(&wide(text), fmt, 1000.0, 100.0)
                .and_then(|layout| layout.GetMetrics(&mut m))
        };
        if ok.is_ok() {
            m.widthIncludingTrailingWhitespace
        } else {
            text.len() as f32 * 7.0
        }
    }

    /// Item widths in physical pixels at `dpi`.
    pub fn measure(&self, items: &[ItemView], dpi: u32) -> Vec<ItemMetrics> {
        items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let value = self.text_width(&item.text(), &self.value_fmt).max(BAR_W);
                let label = self.text_width(paint_label(item), &self.label_fmt);
                let badge = if first_provider(items, i) {
                    LOGO_WIDTH
                } else {
                    0.0
                };
                ItemMetrics {
                    full_w: to_px(badge + value.max(label), dpi),
                    compact_w: to_px(badge + value, dpi),
                }
            })
            .collect()
    }

    pub fn ellipsis_px(&self, dpi: u32) -> i32 {
        to_px(self.text_width(ELLIPSIS, &self.value_fmt), dpi)
    }

    /// Draws the strip and pushes it to `hwnd` at screen position `pos` with size `size` (px).
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        hwnd: HWND,
        pos: (i32, i32),
        size: (i32, i32),
        dpi: u32,
        items: &[ItemView],
        metrics: &[ItemMetrics],
        layout: &Layout,
        light: bool,
    ) -> Result<()> {
        let (w, h) = (size.0.max(1), size.1.max(1));
        if !self.surface.as_ref().is_some_and(|s| s.fits(w, h, dpi)) {
            self.surface = None;
            self.surface = Some(Surface::new(&self.d2d, w, h, dpi)?);
        }
        let result = match &self.surface {
            Some(surface) => {
                // SAFETY: the surface's DC holds a 32-bpp DIB of (w, h); hwnd is layered.
                unsafe { self.draw(surface, items, metrics, layout, light) }.and_then(|_| {
                    let blend = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8,
                    };
                    // SAFETY: as above; a null destination DC means the screen's default palette.
                    unsafe {
                        UpdateLayeredWindow(
                            hwnd,
                            None,
                            Some(&POINT { x: pos.0, y: pos.1 }),
                            Some(&SIZE { cx: w, cy: h }),
                            Some(surface.mem),
                            Some(&POINT::default()),
                            COLORREF(0),
                            Some(&blend),
                            ULW_ALPHA,
                        )
                    }
                })
            }
            None => Ok(()),
        };
        if result.is_err() {
            // Covers D2DERR_RECREATE_TARGET: the next frame starts from new objects.
            self.surface = None;
        }
        result
    }

    unsafe fn draw(
        &self,
        surface: &Surface,
        items: &[ItemView],
        metrics: &[ItemMetrics],
        layout: &Layout,
        light: bool,
    ) -> Result<()> {
        let Surface {
            w,
            h,
            dpi,
            rt,
            brush,
            ..
        } = surface;
        let (w, h, dpi) = (*w, *h, *dpi);
        let palette = if light { &LIGHT } else { &DARK };
        // SAFETY: the render target is bound to the surface's DC for its whole life.
        unsafe {
            rt.BeginDraw();
            // Nearly transparent fill keeps the whole strip hit-testable for hover and clicks.
            rt.Clear(Some(&color(0, 1.0 / 255.0)));
            let set = |rgb: u32, a: f32| brush.SetColor(&color(rgb, a));
            let h_dip = to_dip(h, dpi);
            let block_h = match layout.mode {
                Mode::Full => LABEL_LINE + VALUE_LINE + BAR_GAP + BAR_H,
                Mode::Compact => VALUE_LINE + BAR_GAP + BAR_H,
            };
            let top = ((h_dip - block_h) / 2.0).max(0.0);
            let mut x = 0.0f32;
            for (n, &i) in layout.visible.iter().enumerate() {
                if n > 0 {
                    x += GAP;
                }
                let item = &items[i];
                let mut col_w = to_dip(
                    match layout.mode {
                        Mode::Full => metrics[i].full_w,
                        Mode::Compact => metrics[i].compact_w,
                    },
                    dpi,
                );
                if !layout.visible[..n]
                    .iter()
                    .any(|&previous| items[previous].kind == item.kind)
                    && item.kind != Kind::Hardware
                {
                    let logo_y = top + (block_h - LOGO_SIZE) / 2.0;
                    if item.completion_flash {
                        set(palette.value, 1.0);
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: x,
                                    top: logo_y - LOGO_INSET,
                                    right: x + LOGO_SIZE + 2.0 * LOGO_INSET,
                                    bottom: logo_y + LOGO_SIZE + LOGO_INSET,
                                },
                                radiusX: 4.0,
                                radiusY: 4.0,
                            },
                            brush,
                        );
                    }
                    set(
                        if item.completion_flash {
                            if light { 0xffffff } else { 0x202020 }
                        } else {
                            palette.value
                        },
                        1.0,
                    );
                    let mut previous = Default::default();
                    rt.GetTransform(&mut previous);
                    let mut transform = previous;
                    transform.M11 = LOGO_SIZE / 24.0;
                    transform.M22 = LOGO_SIZE / 24.0;
                    transform.M31 = x + LOGO_INSET;
                    transform.M32 = logo_y;
                    rt.SetTransform(&transform);
                    rt.FillGeometry(
                        if item.kind == Kind::Claude {
                            &self.claude_logo
                        } else {
                            &self.codex_logo
                        },
                        brush,
                        None,
                    );
                    rt.SetTransform(&previous);
                    x += LOGO_WIDTH;
                    col_w -= LOGO_WIDTH;
                }
                let mut y = top;
                if layout.mode == Mode::Full {
                    set(palette.label, 1.0);
                    let r = D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + col_w,
                        bottom: y + LABEL_LINE,
                    };
                    rt.DrawText(
                        &wide(paint_label(item)),
                        &self.label_fmt,
                        &r,
                        brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    y += LABEL_LINE;
                }
                set(palette.value, 1.0);
                let r = D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + col_w,
                    bottom: y + VALUE_LINE,
                };
                rt.DrawText(
                    &wide(&item.text()),
                    &self.value_fmt,
                    &r,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                y += VALUE_LINE + BAR_GAP;
                let bx = x + (col_w - BAR_W) / 2.0;
                let bar = |right: f32| D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: bx,
                        top: y,
                        right,
                        bottom: y + BAR_H,
                    },
                    radiusX: BAR_RADIUS,
                    radiusY: BAR_RADIUS,
                };
                set(palette.track, 1.0);
                rt.FillRoundedRectangle(&bar(bx + BAR_W), brush);
                if let Some(v) = item.value.filter(|v| *v > 0.0) {
                    set(
                        bar_color(item.kind, Some(v)),
                        if item.dim { 0.5 } else { 1.0 },
                    );
                    rt.FillRoundedRectangle(&bar(bx + BAR_W * v.clamp(0.0, 100.0) / 100.0), brush);
                }
                if let Some(t) = item.tick {
                    let tw = to_dip(to_px(TICK_W, dpi), dpi);
                    let raw_x = bx + (BAR_W - tw) * t.clamp(0.0, 1.0);
                    let tx = to_dip((raw_x * dpi as f32 / 96.0).round() as i32, dpi);
                    set(palette.value, 1.0);
                    rt.FillRectangle(
                        &D2D_RECT_F {
                            left: tx,
                            top: y - TICK_OVERHANG,
                            right: tx + tw,
                            bottom: y + BAR_H + TICK_OVERHANG,
                        },
                        brush,
                    );
                }
                x += col_w;
                if layout.separator_after == Some(i) {
                    x += GAP;
                    set(palette.separator, 1.0);
                    let sy = (h_dip - SEP_H) / 2.0;
                    rt.FillRectangle(
                        &D2D_RECT_F {
                            left: x,
                            top: sy,
                            right: x + SEP_W,
                            bottom: sy + SEP_H,
                        },
                        brush,
                    );
                    x += SEP_W;
                }
            }
            if layout.overflow && !layout.visible.is_empty() {
                x += GAP;
                set(palette.value, 1.0);
                let r = D2D_RECT_F {
                    left: x,
                    top: 0.0,
                    right: to_dip(w, dpi),
                    bottom: h_dip,
                };
                rt.DrawText(
                    &wide(ELLIPSIS),
                    &self.value_fmt,
                    &r,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
            rt.EndDraw(None, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ItemId, Kind};
    use crate::ui::layout::layout;

    fn render_pixels(
        renderer: &Renderer,
        items: &[ItemView],
        dpi: u32,
        light: bool,
    ) -> (i32, i32, Vec<u32>) {
        render_mode_pixels(renderer, items, dpi, light, Mode::Full)
    }

    fn render_mode_pixels(
        renderer: &Renderer,
        items: &[ItemView],
        dpi: u32,
        light: bool,
        mode: Mode,
    ) -> (i32, i32, Vec<u32>) {
        let metrics = renderer.measure(items, dpi);
        let kinds: Vec<_> = items.iter().map(|i| i.kind).zip(metrics.clone()).collect();
        let mut l = layout(
            &kinds,
            4000,
            to_px(GAP, dpi),
            to_px(SEP_W, dpi),
            renderer.ellipsis_px(dpi),
        );
        l.mode = mode;
        if mode == Mode::Compact {
            l.width = metrics.iter().map(|m| m.compact_w).sum::<i32>()
                + to_px(GAP, dpi) * (items.len() as i32 - 1).max(0)
                + if l.separator_after.is_some() {
                    to_px(SEP_W + GAP, dpi)
                } else {
                    0
                };
        }
        let (w, h) = (l.width.max(1), to_px(48.0, dpi));
        let surface = Surface::new(&renderer.d2d, w, h, dpi).expect("surface");
        unsafe {
            renderer
                .draw(&surface, items, &metrics, &l, light)
                .expect("draw");
            let px = std::slice::from_raw_parts(surface.bits, (w * h) as usize).to_vec();
            (w, h, px)
        }
    }

    fn provider_item(kind: Kind, weekly: bool) -> ItemView {
        ItemView {
            id: match (kind, weekly) {
                (Kind::Claude, false) => ItemId::ClaudeFive,
                (Kind::Claude, true) => ItemId::ClaudeWeek,
                (Kind::Codex, false) => ItemId::CodexFive,
                _ => ItemId::CodexWeek,
            },
            kind,
            label: if weekly {
                "wk".into()
            } else {
                format!("{kind:?} 5h")
            },
            value: Some(36.0),
            dim: false,
            completion_flash: false,
            show_remaining: false,
            badge: None,
            tick: None,
        }
    }

    #[test]
    fn provider_names_are_replaced_by_one_logo_before_the_first_visible_period() {
        let renderer = Renderer::new().expect("renderer");
        for kind in [Kind::Claude, Kind::Codex] {
            let five = provider_item(kind, false);
            let week = provider_item(kind, true);
            for dpi in [96, 144] {
                let together = renderer.measure(&[five.clone(), week.clone()], dpi);
                let weekly_only = renderer.measure(std::slice::from_ref(&week), dpi);
                for compact in [false, true] {
                    let width = |m: ItemMetrics| if compact { m.compact_w } else { m.full_w };
                    assert_eq!(width(together[0]), width(weekly_only[0]));
                    assert!(width(together[0]) > width(together[1]));
                }
            }
            assert!(five.label.contains(match kind {
                Kind::Claude => "Claude",
                _ => "Codex",
            }));
        }
    }

    #[test]
    fn account_letters_do_not_change_strip_pixels_or_width() {
        let renderer = Renderer::new().unwrap();
        for kind in [Kind::Claude, Kind::Codex] {
            let plain = provider_item(kind, false);
            let mut labeled = plain.clone();
            labeled.badge = Some("Work".into());
            for mode in [Mode::Full, Mode::Compact] {
                assert_eq!(
                    render_mode_pixels(&renderer, std::slice::from_ref(&plain), 96, false, mode),
                    render_mode_pixels(&renderer, std::slice::from_ref(&labeled), 96, false, mode)
                );
            }
        }
    }

    #[test]
    fn completion_flash_changes_only_logo_pixels_in_both_modes() {
        let renderer = Renderer::new().expect("renderer");
        for kind in [Kind::Claude, Kind::Codex] {
            let mut base = provider_item(kind, false);
            base.badge = Some("P".into());
            let mut flashing = base.clone();
            flashing.completion_flash = true;
            for dpi in [96, 144] {
                for light in [false, true] {
                    for mode in [Mode::Full, Mode::Compact] {
                        let (w, _, before) = render_mode_pixels(
                            &renderer,
                            std::slice::from_ref(&base),
                            dpi,
                            light,
                            mode,
                        );
                        let (_, _, after) =
                            render_mode_pixels(&renderer, &[flashing.clone()], dpi, light, mode);
                        let changed: Vec<_> = before
                            .iter()
                            .zip(&after)
                            .enumerate()
                            .filter(|(_, (a, b))| a != b)
                            .map(|(i, _)| i)
                            .collect();
                        assert!(!changed.is_empty(), "{kind:?} {mode:?} {dpi} light={light}");
                        assert!(
                            changed
                                .iter()
                                .all(|i| (i % w as usize) < to_px(24.0, dpi) as usize)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn each_logo_has_distinct_pixels_and_a_provider_has_no_second_logo() {
        let renderer = Renderer::new().expect("renderer");
        for dpi in [96, 144] {
            for light in [false, true] {
                for mode in [Mode::Full, Mode::Compact] {
                    let claude = provider_item(Kind::Claude, false);
                    let codex = provider_item(Kind::Codex, false);
                    let (_, _, a) = render_mode_pixels(
                        &renderer,
                        std::slice::from_ref(&claude),
                        dpi,
                        light,
                        mode,
                    );
                    let (_, _, b) = render_mode_pixels(&renderer, &[codex], dpi, light, mode);
                    assert!(a.iter().zip(b).filter(|(a, b)| **a != *b).count() > 30);
                    let mut week = provider_item(Kind::Claude, true);
                    let (_, _, normal) = render_mode_pixels(
                        &renderer,
                        &[claude.clone(), week.clone()],
                        dpi,
                        light,
                        mode,
                    );
                    week.completion_flash = true;
                    let (_, _, no_second_logo) =
                        render_mode_pixels(&renderer, &[claude, week], dpi, light, mode);
                    assert_eq!(normal, no_second_logo);
                }
            }
        }
    }

    #[test]
    #[ignore = "exports only offscreen synthetic provider previews to CCT_PREVIEW_DIR"]
    fn export_provider_previews() {
        let dir =
            std::path::PathBuf::from(std::env::var_os("CCT_PREVIEW_DIR").expect("CCT_PREVIEW_DIR"));
        std::fs::create_dir_all(&dir).expect("preview directory");
        let renderer = Renderer::new().expect("renderer");
        let mut items = vec![
            provider_item(Kind::Claude, false),
            provider_item(Kind::Claude, true),
            provider_item(Kind::Codex, false),
            provider_item(Kind::Codex, true),
        ];
        items[0].badge = Some("P".into());
        items[2].badge = Some("W".into());
        for dpi in [96, 144] {
            for light in [false, true] {
                for mode in [Mode::Full, Mode::Compact] {
                    for flash in [false, true] {
                        items[0].completion_flash = flash;
                        items[2].completion_flash = flash;
                        let (w, h, px) = render_mode_pixels(&renderer, &items, dpi, light, mode);
                        let size = 54 + w * h * 4;
                        let mut bmp = Vec::with_capacity(size as usize);
                        bmp.extend_from_slice(b"BM");
                        bmp.extend_from_slice(&(size as u32).to_le_bytes());
                        bmp.extend_from_slice(&[0; 4]);
                        bmp.extend_from_slice(&54u32.to_le_bytes());
                        bmp.extend_from_slice(&40u32.to_le_bytes());
                        bmp.extend_from_slice(&w.to_le_bytes());
                        bmp.extend_from_slice(&(-h).to_le_bytes());
                        bmp.extend_from_slice(&1u16.to_le_bytes());
                        bmp.extend_from_slice(&32u16.to_le_bytes());
                        bmp.extend_from_slice(&[0; 24]);
                        for pixel in px {
                            // Composite premultiplied D2D pixels onto a taskbar-like background.
                            let bg = if light { 0xf3u32 } else { 0x20u32 };
                            let alpha = pixel >> 24;
                            let channel = |shift: u32| -> u32 {
                                ((pixel >> shift) & 255) + bg * (255 - alpha) / 255
                            };
                            let rgb =
                                0xff00_0000u32 | channel(16) << 16 | channel(8) << 8 | channel(0);
                            bmp.extend_from_slice(&rgb.to_le_bytes());
                        }
                        let filename = format!(
                            "provider-{}-{:?}-{dpi}-{}.bmp",
                            if light { "light" } else { "dark" },
                            mode,
                            if flash { "flash" } else { "normal" }
                        );
                        std::fs::write(dir.join(filename), bmp).expect("preview bitmap");
                    }
                }
            }
        }
    }

    #[test]
    fn draw_renders_tick_marker_at_100_and_150_dpi() {
        let renderer = Renderer::new().expect("renderer");
        let base = ItemView {
            id: ItemId::ClaudeFive,
            kind: Kind::Claude,
            label: "Claude 5h".into(),
            badge: None,
            value: Some(36.0),
            tick: None,
            dim: false,
            completion_flash: false,
            show_remaining: false,
        };
        let mut with_tick = base.clone();
        with_tick.tick = Some(0.5);

        for dpi in [96u32, 144u32] {
            for (light, expected_rgb) in [(false, DARK.value), (true, LIGHT.value)] {
                let (w, h, px_no_tick) =
                    render_pixels(&renderer, std::slice::from_ref(&base), dpi, light);
                let (_, _, px_with_tick) =
                    render_pixels(&renderer, std::slice::from_ref(&with_tick), dpi, light);
                let expected_argb = 0xff00_0000 | expected_rgb;
                let diff_expected: usize = px_no_tick
                    .iter()
                    .zip(&px_with_tick)
                    .filter(|(a, b)| a != b && **b == expected_argb)
                    .count();
                let min_px = ((BAR_H + 2.0 * TICK_OVERHANG) * dpi as f32 / 96.0).floor() as usize
                    * to_px(TICK_W, dpi) as usize;
                assert!(
                    diff_expected >= min_px,
                    "dpi={dpi} light={light}: expected at least {min_px} new {expected_argb:#x} tick pixels, got {diff_expected} (size {w}x{h})"
                );
            }
        }
    }
}
