use cairo::{Context, Format, ImageSurface, Operator};
use pango::FontDescription;

use crate::error::Error;

/// Logical pixels reserved around the card for the shadow.
pub(crate) const SHADOW: i32 = 32;
const PAD: i32 = 18;
const RADIUS: f64 = 14.0;
const BLUR_LOGICAL: f64 = 22.0;
const OFFSET_Y_LOGICAL: f64 = 8.0;
const SHADOW_OPACITY: f32 = 0.45;

pub(crate) struct Panel {
    pub logical_width: i32,
    pub logical_height: i32,
    pub card_width: i32,
    pub card_height: i32,
    pub scale: i32,
    /// Tightly packed little-endian premultiplied BGRA.
    pub pixels: Vec<u8>,
    pub buf_width: i32,
    pub buf_height: i32,
}

pub(crate) fn render(markup: &str, font_spec: &str, scale: i32) -> Result<Panel, Error> {
    let scale = scale.max(1);
    let shadow = SHADOW * scale;
    let pad = PAD * scale;
    let radius = RADIUS * scale as f64;
    let blur_radius = (BLUR_LOGICAL * scale as f64).round() as i32;
    let offset_y = (OFFSET_Y_LOGICAL * scale as f64).round() as i32;

    let measure = ImageSurface::create(Format::ARgb32, 1, 1).map_err(draw_err)?;
    let layout = layout_for(&measure, markup, font_spec, scale)?;
    let (text_w, text_h) = layout.pixel_size();
    let text_w = text_w.max(1);
    let text_h = text_h.max(1);

    let card_w = text_w + pad * 2;
    let card_h = text_h + pad * 2;
    let card_logical_w = div_ceil(card_w, scale);
    let card_logical_h = div_ceil(card_h, scale);
    let logical_w = card_logical_w + SHADOW * 2;
    let logical_h = card_logical_h + SHADOW * 2;
    let buf_w = logical_w * scale;
    let buf_h = logical_h * scale;

    let mut surface = ImageSurface::create(Format::ARgb32, buf_w, buf_h).map_err(draw_err)?;
    paint_shadow(
        &mut surface,
        buf_w,
        buf_h,
        shadow,
        offset_y,
        card_w,
        card_h,
        radius,
        blur_radius,
    )?;

    let cr = Context::new(&surface);
    cr.set_operator(Operator::Over);
    cr.set_source_rgba(
        0x16 as f64 / 255.0,
        0x18 as f64 / 255.0,
        0x1d as f64 / 255.0,
        0.94,
    );
    rounded_rect(
        &cr,
        shadow as f64,
        shadow as f64,
        card_w as f64,
        card_h as f64,
        radius,
    );
    cr.fill_preserve();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
    cr.set_line_width(1.0);
    cr.stroke();

    let layout = layout_for(&surface, markup, font_spec, scale)?;
    cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    cr.move_to((shadow + pad) as f64, (shadow + pad) as f64);
    pangocairo::functions::show_layout(&cr, &layout);
    drop(cr);
    surface.flush();

    let pixels = tight_pixels(&mut surface, buf_w, buf_h)?;
    Ok(Panel {
        logical_width: logical_w,
        logical_height: logical_h,
        card_width: card_logical_w,
        card_height: card_logical_h,
        scale,
        pixels,
        buf_width: buf_w,
        buf_height: buf_h,
    })
}

fn layout_for(
    surface: &ImageSurface,
    markup: &str,
    font_spec: &str,
    scale: i32,
) -> Result<pango::Layout, Error> {
    let cr = Context::new(surface);
    let layout = pangocairo::functions::create_layout(&cr)
        .ok_or_else(|| Error::Draw("Pango could not create a layout".to_owned()))?;
    let mut font = FontDescription::from_string(font_spec);
    scale_font(&mut font, scale);
    layout.set_font_description(Some(&font));
    layout.set_markup(markup);
    Ok(layout)
}

fn scale_font(font: &mut FontDescription, scale: i32) {
    let size = font.size();
    if size > 0 {
        if font.size_is_absolute() {
            font.set_absolute_size((size * scale) as f64);
        } else {
            font.set_size(size * scale);
        }
    }
}

fn paint_shadow(
    surface: &mut ImageSurface,
    buf_w: i32,
    buf_h: i32,
    shadow: i32,
    offset_y: i32,
    card_w: i32,
    card_h: i32,
    radius: f64,
    blur_radius: i32,
) -> Result<(), Error> {
    let mut mask = ImageSurface::create(Format::ARgb32, buf_w, buf_h).map_err(draw_err)?;
    {
        let cr = Context::new(&mask);
        cr.set_operator(Operator::Source);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.paint();
        cr.set_operator(Operator::Over);
        cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);
        rounded_rect(
            &cr,
            shadow as f64,
            (shadow + offset_y) as f64,
            card_w as f64,
            card_h as f64,
            radius,
        );
        cr.fill();
    }
    mask.flush();

    let alpha = {
        let stride = mask.stride() as usize;
        let data = mask.data().map_err(|err| Error::Draw(err.to_string()))?;
        let mut alpha = vec![0.0f32; (buf_w * buf_h) as usize];
        for y in 0..buf_h {
            for x in 0..buf_w {
                let index = y as usize * stride + x as usize * 4 + 3;
                alpha[(y * buf_w + x) as usize] = data[index] as f32;
            }
        }
        alpha
    };
    let blurred = blur_alpha(&alpha, buf_w, buf_h, blur_radius);

    let stride = surface.stride() as usize;
    let mut data = surface.data().map_err(|err| Error::Draw(err.to_string()))?;
    for y in 0..buf_h {
        for x in 0..buf_w {
            let covered = blurred[(y * buf_w + x) as usize];
            let alpha = (covered * SHADOW_OPACITY).round().clamp(0.0, 255.0) as u8;
            // Cairo ARGB32 is premultiplied, little-endian B, G, R, A.
            let index = y as usize * stride + x as usize * 4;
            data[index] = 0;
            data[index + 1] = 0;
            data[index + 2] = 0;
            data[index + 3] = alpha;
        }
    }
    Ok(())
}

fn blur_alpha(src: &[f32], width: i32, height: i32, radius: i32) -> Vec<f32> {
    if radius <= 0 || width <= 0 || height <= 0 {
        return src.to_vec();
    }
    let sigma = radius as f64 / 2.0;
    let mut kernel = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut sum = 0.0f64;
    for offset in -radius..=radius {
        let x = offset as f64;
        let weight = (-(x * x) / (2.0 * sigma * sigma)).exp();
        kernel.push(weight);
        sum += weight;
    }
    for weight in &mut kernel {
        *weight /= sum;
    }

    let width = width as usize;
    let height = height as usize;
    let mut horizontal = vec![0.0f32; width * height];
    let mut out = vec![0.0f32; width * height];
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0.0f64;
            for (index, weight) in kernel.iter().enumerate() {
                let sample_x =
                    (x as i32 + index as i32 - radius).clamp(0, width as i32 - 1) as usize;
                acc += src[y * width + sample_x] as f64 * weight;
            }
            horizontal[y * width + x] = acc as f32;
        }
    }
    for y in 0..height {
        for x in 0..width {
            let mut acc = 0.0f64;
            for (index, weight) in kernel.iter().enumerate() {
                let sample_y =
                    (y as i32 + index as i32 - radius).clamp(0, height as i32 - 1) as usize;
                acc += horizontal[sample_y * width + x] as f64 * weight;
            }
            out[y * width + x] = acc as f32;
        }
    }
    out
}

fn rounded_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    let radius = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    const PI: f64 = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - radius, y + radius, radius, -PI / 2.0, 0.0);
    cr.arc(x + w - radius, y + h - radius, radius, 0.0, PI / 2.0);
    cr.arc(x + radius, y + h - radius, radius, PI / 2.0, PI);
    cr.arc(x + radius, y + radius, radius, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

fn tight_pixels(surface: &mut ImageSurface, buf_w: i32, buf_h: i32) -> Result<Vec<u8>, Error> {
    let stride = surface.stride();
    if stride <= 0 {
        return Err(Error::Draw(
            "cairo surface stride is not positive".to_owned(),
        ));
    }
    let row_bytes = (buf_w as usize)
        .checked_mul(4)
        .ok_or_else(|| Error::Draw("frame is too large".to_owned()))?;
    let data = surface.data().map_err(|err| Error::Draw(err.to_string()))?;
    let mut pixels = vec![0u8; row_bytes * buf_h as usize];
    for y in 0..buf_h as usize {
        let src = y * stride as usize;
        let dst = y * row_bytes;
        pixels[dst..dst + row_bytes].copy_from_slice(&data[src..src + row_bytes]);
    }
    Ok(pixels)
}

fn div_ceil(value: i32, divisor: i32) -> i32 {
    (value + divisor - 1) / divisor
}

fn draw_err(err: cairo::Error) -> Error {
    Error::Draw(err.to_string())
}

trait CairoSurfaceExt {
    fn stride(&self) -> i32;
    fn data(&mut self) -> Result<cairo::ImageSurfaceData<'_>, cairo::BorrowError>;
}

impl CairoSurfaceExt for ImageSurface {
    fn stride(&self) -> i32 {
        self.get_stride()
    }

    fn data(&mut self) -> Result<cairo::ImageSurfaceData<'_>, cairo::BorrowError> {
        self.get_data()
    }
}

trait FontExt {
    fn size(&self) -> i32;
    fn size_is_absolute(&self) -> bool;
}

impl FontExt for FontDescription {
    fn size(&self) -> i32 {
        self.get_size()
    }

    fn size_is_absolute(&self) -> bool {
        self.get_size_is_absolute()
    }
}

trait LayoutExt {
    fn pixel_size(&self) -> (i32, i32);
}

impl LayoutExt for pango::Layout {
    fn pixel_size(&self) -> (i32, i32) {
        self.get_pixel_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_is_opaque_and_shadow_reaches_the_margin() {
        let panel = render(
            "<span foreground=\"#f2f4f8\">Next keys</span>",
            "normal 25",
            1,
        )
        .unwrap();
        assert!(panel.buf_width > panel.card_width);
        assert!(panel.logical_width == panel.card_width + SHADOW * 2);

        let center_x = SHADOW + panel.card_width / 2;
        let center_y = SHADOW + panel.card_height / 2;
        assert!(alpha_at(&panel, center_x, center_y) > 200);

        let shadow_y = panel.buf_height - 8;
        assert!(alpha_at(&panel, panel.buf_width / 2, shadow_y) > 0);
    }

    #[test]
    fn higher_scale_renders_more_physical_pixels() {
        let low = render("Hi", "normal 18", 1).unwrap();
        let high = render("Hi", "normal 18", 2).unwrap();
        assert!(high.buf_width > low.buf_width);
        assert_eq!(high.scale, 2);
    }

    fn alpha_at(panel: &Panel, x: i32, y: i32) -> u8 {
        let index = ((y * panel.buf_width + x) * 4 + 3) as usize;
        panel.pixels[index]
    }
}
