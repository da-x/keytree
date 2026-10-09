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
/// Largest point size the overlay will use, including when `--font` asks for more.
const MAX_POINT_SIZE: i32 = 36;
/// Smallest point size used when a long list must shrink to fit the screen.
const MIN_POINT_SIZE: i32 = 11;
/// Gutter between the key and description columns, as a fraction of an em.
const GUTTER_EM: f64 = 0.75;

/// Tallest logical window for a screen or workspace `screen_h` pixels high.
///
/// The limit includes the shadow margin around the card.
pub(crate) fn max_window_height(screen_h: i32) -> i32 {
    let screen_h = screen_h.max(0) as i64;
    (screen_h * 80 / 100) as i32
}

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

pub(crate) fn render(
    markup: &str,
    font_spec: &str,
    scale: i32,
    max_logical_height: Option<i32>,
) -> Result<Panel, Error> {
    let scale = scale.max(1);
    let shadow = SHADOW * scale;
    let pad = PAD * scale;
    let radius = RADIUS * scale as f64;
    let blur_radius = (BLUR_LOGICAL * scale as f64).round() as i32;
    let offset_y = (OFFSET_Y_LOGICAL * scale as f64).round() as i32;

    let measure = ImageSurface::create(Format::ARgb32, 1, 1).map_err(draw_err)?;
    let text = fit_text(&measure, markup, font_spec, scale, max_logical_height)?;
    debug_assert!(text.width >= 1 && text.height >= 1);
    debug_assert!(text.gutter >= 0 && text.key_column_width >= 0);
    debug_assert!(text
        .runs
        .iter()
        .all(|run| { run.x >= 0 && run.y >= 0 && run.width >= 0 && run.baseline >= 0 }));
    let text_w = text.width;
    let text_h = text.height;

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

    let font = font_at(font_spec, text.point_size, scale);
    cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    for run in &text.runs {
        if run.markup.is_empty() {
            continue;
        }
        let layout = layout_for(&surface, &run.markup, &font)?;
        cr.move_to((shadow + pad + run.x) as f64, (shadow + pad + run.y) as f64);
        pangocairo::functions::show_layout(&cr, &layout);
    }
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

struct Run {
    markup: String,
    x: i32,
    y: i32,
    width: i32,
    baseline: i32,
}

struct TextLayout {
    width: i32,
    height: i32,
    point_size: i32,
    gutter: i32,
    key_column_width: i32,
    runs: Vec<Run>,
}

struct CellMetrics {
    width: i32,
    height: i32,
    baseline: i32,
}

enum Line {
    Gap,
    Block(String),
    Row { key: String, desc: String },
}

enum Prepared {
    Gap,
    Block(CellMetrics, String),
    Row {
        key: CellMetrics,
        key_markup: String,
        desc: CellMetrics,
        desc_markup: String,
    },
}

/// Lines with a tab are a key/description row. The first tab is the column
/// split. Any other line is one block, centered across the rows.
fn parse_overlay(markup: &str) -> Vec<Line> {
    markup
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                Line::Gap
            } else if let Some((key, desc)) = line.split_once('\t') {
                Line::Row {
                    key: key.to_owned(),
                    desc: desc.to_owned(),
                }
            } else {
                Line::Block(line.to_owned())
            }
        })
        .collect()
}

fn fit_text(
    surface: &ImageSurface,
    markup: &str,
    font_spec: &str,
    scale: i32,
    max_logical_height: Option<i32>,
) -> Result<TextLayout, Error> {
    let ceiling = point_ceiling(font_spec);
    let at_ceiling = layout_text(surface, markup, font_spec, ceiling, scale)?;
    let Some(limit) = max_logical_height else {
        return Ok(at_ceiling);
    };
    if window_logical_height(at_ceiling.height, scale) <= limit {
        return Ok(at_ceiling);
    }
    let floor = if ceiling < MIN_POINT_SIZE {
        ceiling
    } else {
        MIN_POINT_SIZE
    };
    if floor >= ceiling {
        return Ok(at_ceiling);
    }

    let mut best = layout_text(surface, markup, font_spec, floor, scale)?;
    let mut lo = floor;
    let mut hi = ceiling - 1;
    while lo <= hi {
        let mid = lo + (hi - lo + 1) / 2;
        let candidate = layout_text(surface, markup, font_spec, mid, scale)?;
        if window_logical_height(candidate.height, scale) <= limit {
            best = candidate;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    Ok(best)
}

fn layout_text(
    surface: &ImageSurface,
    markup: &str,
    font_spec: &str,
    points: i32,
    scale: i32,
) -> Result<TextLayout, Error> {
    let font = font_at(font_spec, points, scale);
    let line_box = measure_cell(surface, "X", &font)?.height.max(1);
    let mut prepared = Vec::new();
    let mut key_column_width = 0;
    let mut desc_column_width = 0;
    let mut block_width = 0;

    for line in parse_overlay(markup) {
        match line {
            Line::Gap => prepared.push(Prepared::Gap),
            Line::Block(text) => {
                let cell = measure_cell(surface, &text, &font)?;
                block_width = block_width.max(cell.width);
                prepared.push(Prepared::Block(cell, text));
            }
            Line::Row { key, desc } => {
                let key_cell = measure_cell(surface, &key, &font)?;
                let desc_cell = measure_cell(surface, &desc, &font)?;
                key_column_width = key_column_width.max(key_cell.width);
                desc_column_width = desc_column_width.max(desc_cell.width);
                prepared.push(Prepared::Row {
                    key: key_cell,
                    key_markup: key,
                    desc: desc_cell,
                    desc_markup: desc,
                });
            }
        }
    }

    let em = measure_cell(surface, "M", &font)?.width;
    let gutter = if desc_column_width > 0 {
        ((em as f64) * GUTTER_EM).round().max(1.0) as i32
    } else {
        0
    };
    let columns_width = key_column_width + gutter + desc_column_width;
    let text_w = columns_width.max(block_width).max(1);
    let col_left = (text_w - columns_width) / 2;

    let mut runs = Vec::new();
    let mut y = 0;
    for item in prepared {
        match item {
            Prepared::Gap => y += line_box,
            Prepared::Block(cell, text) => {
                runs.push(Run {
                    x: (text_w - cell.width) / 2,
                    y,
                    width: cell.width,
                    baseline: cell.baseline,
                    markup: text,
                });
                y += cell.height;
            }
            Prepared::Row {
                key,
                key_markup,
                desc,
                desc_markup,
            } => {
                let key_dy = (desc.baseline - key.baseline).max(0);
                let desc_dy = (key.baseline - desc.baseline).max(0);
                let row_h = (key_dy + key.height).max(desc_dy + desc.height).max(1);
                runs.push(Run {
                    x: col_left + key_column_width - key.width,
                    y: y + key_dy,
                    width: key.width,
                    baseline: key.baseline,
                    markup: key_markup,
                });
                runs.push(Run {
                    x: col_left + key_column_width + gutter,
                    y: y + desc_dy,
                    width: desc.width,
                    baseline: desc.baseline,
                    markup: desc_markup,
                });
                y += row_h;
            }
        }
    }

    Ok(TextLayout {
        width: text_w,
        height: y.max(1),
        point_size: points,
        gutter,
        key_column_width,
        runs,
    })
}

fn measure_cell(
    surface: &ImageSurface,
    markup: &str,
    font: &FontDescription,
) -> Result<CellMetrics, Error> {
    if markup.is_empty() {
        return Ok(CellMetrics {
            width: 0,
            height: 0,
            baseline: 0,
        });
    }
    let layout = layout_for(surface, markup, font)?;
    let (width, height) = layout.pixel_size();
    Ok(CellMetrics {
        width: width.max(0),
        height: height.max(0),
        baseline: pango_pixels(layout.baseline()),
    })
}

fn layout_for(
    surface: &ImageSurface,
    markup: &str,
    font: &FontDescription,
) -> Result<pango::Layout, Error> {
    let cr = Context::new(surface);
    let layout = pangocairo::functions::create_layout(&cr)
        .ok_or_else(|| Error::Draw("Pango could not create a layout".to_owned()))?;
    layout.set_font_description(Some(font));
    layout.set_markup(markup);
    Ok(layout)
}

fn font_at(font_spec: &str, points: i32, scale: i32) -> FontDescription {
    let mut font = FontDescription::from_string(font_spec);
    font.set_size(points.saturating_mul(pango::SCALE));
    scale_font(&mut font, scale);
    font
}

fn point_ceiling(font_spec: &str) -> i32 {
    let font = FontDescription::from_string(font_spec);
    let size = font.size();
    if size <= 0 {
        return MAX_POINT_SIZE;
    }
    let points = if font.size_is_absolute() {
        let px = pango_pixels(size);
        ((px as i64) * 72 / 96) as i32
    } else {
        pango_pixels(size)
    };
    points.clamp(1, MAX_POINT_SIZE)
}

fn window_logical_height(text_h: i32, scale: i32) -> i32 {
    let pad = PAD * scale;
    div_ceil(text_h + pad * 2, scale) + SHADOW * 2
}

fn pango_pixels(units: i32) -> i32 {
    units.saturating_add(pango::SCALE / 2) >> 10
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
    fn baseline(&self) -> i32;
}

impl LayoutExt for pango::Layout {
    fn pixel_size(&self) -> (i32, i32) {
        self.get_pixel_size()
    }

    fn baseline(&self) -> i32 {
        self.get_baseline()
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
            None,
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
        let low = render("Hi", "normal 18", 1, None).unwrap();
        let high = render("Hi", "normal 18", 2, None).unwrap();
        assert!(high.buf_width > low.buf_width);
        assert_eq!(high.scale, 2);
    }

    #[test]
    fn font_does_not_grow_past_the_cap() {
        let capped = render("Hi", "normal 100", 1, None).unwrap();
        let at_cap = render("Hi", "normal 36", 1, None).unwrap();
        let smaller = render("Hi", "normal 18", 1, None).unwrap();
        assert_eq!(capped.logical_height, at_cap.logical_height);
        assert_eq!(capped.logical_width, at_cap.logical_width);
        assert!(smaller.logical_height < at_cap.logical_height);
    }

    #[test]
    fn font_shrinks_so_the_window_stays_within_the_limit() {
        let many = (0..24)
            .map(|i| format!("<span>k{i}</span>\t<span>action {i}</span>"))
            .collect::<Vec<_>>()
            .join("\n");
        let loose = render(&many, "normal 36", 1, None).unwrap();
        let limit = (loose.logical_height * 2 / 3).max(1);
        let tight = render(&many, "normal 36", 1, Some(limit)).unwrap();
        let floor = render(&many, "normal 11", 1, None).unwrap();
        assert!(tight.logical_height < loose.logical_height);
        assert!(tight.logical_height <= limit);
        assert!(tight.logical_height >= floor.logical_height);
    }

    #[test]
    fn keys_are_right_aligned_and_descriptions_left_aligned() {
        let surface = ImageSurface::create(Format::ARgb32, 1, 1).unwrap();
        let markup = "\
<span weight=\"semibold\">c</span>\t<span>One</span>\n\
<span weight=\"semibold\">Control</span>\t<span>Two</span>";
        let text = layout_text(&surface, markup, "normal 36", 36, 1).unwrap();
        let narrow = text
            .runs
            .iter()
            .find(|run| run.markup.contains(">c<"))
            .unwrap();
        let wide = text
            .runs
            .iter()
            .find(|run| run.markup.contains("Control"))
            .unwrap();
        assert!(wide.width > narrow.width);
        assert_eq!(narrow.x + narrow.width, wide.x + wide.width);
        assert_eq!(narrow.x + narrow.width, text.key_column_width);
        let descs: Vec<_> = text
            .runs
            .iter()
            .filter(|run| run.markup.contains("One") || run.markup.contains("Two"))
            .collect();
        assert_eq!(descs.len(), 2);
        assert_eq!(descs[0].x, descs[1].x);
        assert_eq!(descs[0].x, text.key_column_width + text.gutter);
        assert!(text.gutter > 0);
        assert_eq!(narrow.y + narrow.baseline, descs[0].y + descs[0].baseline);
        assert_eq!(wide.y + wide.baseline, descs[1].y + descs[1].baseline);
    }

    #[test]
    fn window_limit_is_eighty_percent_of_the_screen() {
        assert_eq!(max_window_height(1000), 800);
        assert_eq!(max_window_height(1080), 864);
        assert_eq!(max_window_height(0), 0);
    }

    fn alpha_at(panel: &Panel, x: i32, y: i32) -> u8 {
        let index = ((y * panel.buf_width + x) * 4 + 3) as usize;
        panel.pixels[index]
    }
}
