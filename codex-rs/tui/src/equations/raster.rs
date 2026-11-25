//! Typeset locally with embedded math fonts. Layout stores only dimensions and source.
//! Only visible equations acquire a raster, on the background worker.

use image::DynamicImage;
use image::ImageFormat;
use image::RgbaImage;
use ratex_layout::LayoutOptions;
use ratex_render::RenderOptions;
use ratex_types::color::Color;
use ratex_types::display_item::DisplayList;
use ratex_types::math_style::MathStyle;
use std::io::Cursor;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CellSize {
    pub width: u16,
    pub height: u16,
}

impl CellSize {
    pub fn detect() -> Option<Self> {
        let s = crossterm::terminal::window_size().ok()?;
        let width = s.width.checked_div(s.columns)?;
        let height = s.height.checked_div(s.rows)?;
        (width > 0 && height > 0 && width <= 128 && height <= 256).then_some(Self { width, height })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct MathColors {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Formula {
    pub source: Arc<str>,
    pub display: bool,
    pub cell: CellSize,
    pub(super) colors: MathColors,
    pub columns: u16,
    pub above: u16,
    pub below: u16,
    em: u16,
    top: u16,
}

fn typeset(source: &str, display: bool, rgb: [u8; 3]) -> Option<DisplayList> {
    let delimiter = if source.starts_with("$$") || source.starts_with('\\') {
        2
    } else {
        1
    };
    let body = source
        .get(delimiter..source.len().checked_sub(delimiter)?)?
        .trim();
    let ast = ratex_parser::parser::parse(body).ok()?;
    let [r, g, b] = rgb.map(|v| f32::from(v) / 255.0);
    let options = LayoutOptions {
        style: if display {
            MathStyle::Display
        } else {
            MathStyle::Text
        },
        color: Color::rgb(r, g, b),
        ..Default::default()
    };
    let layout = ratex_layout::layout(&ast, &options);
    ratex_layout::validate_layout(&layout).ok()?;
    let list = ratex_layout::to_display_list(&layout);
    (!list.items.is_empty()).then_some(list)
}

#[derive(Clone, Copy)]
pub(super) struct Metrics {
    width: f64,
    height: f64,
    depth: f64,
}

pub(super) fn metrics(source: &str, display: bool) -> Option<Metrics> {
    let list = typeset(source, display, [255; 3])?;
    Some(Metrics {
        width: list.width,
        height: list.height,
        depth: list.depth,
    })
}

impl Formula {
    pub(super) fn measure(
        list: Metrics,
        source: Arc<str>,
        display: bool,
        cell: CellSize,
        max_columns: u16,
        colors: MathColors,
    ) -> Option<Self> {
        let h = f64::from(cell.height);
        let w = f64::from(cell.width);
        let natural = (h * if display { 0.98 } else { 0.83 }).round().max(1.0);
        let fit = ((w * f64::from(max_columns) - 2.0) / list.width.max(0.01)).floor();
        let em = natural.min(fit);
        // A source fallback is more legible than a formula shrunk to a few pixels.
        if em < natural * 0.55
            || !list.width.is_finite()
            || !list.height.is_finite()
            || !list.depth.is_finite()
        {
            return None;
        }
        let ascent = (list.height * em).ceil() + 1.0;
        let descent = (list.depth * em).ceil() + 1.0;
        let baseline = (h * 0.77).round();
        let above = ((ascent - baseline) / h).ceil().max(0.0) as u16;
        let below = ((descent - (h - baseline)) / h).ceil().max(0.0) as u16;
        let columns = ((list.width * em + 2.0) / w).ceil().max(1.0) as u16;
        if above > 48
            || below > 48
            || columns > max_columns
            || u64::from(columns)
                * u64::from(cell.width)
                * u64::from(above + below + 1)
                * u64::from(cell.height)
                > 512 * 1024
        {
            return None;
        }
        Some(Self {
            source,
            display,
            cell,
            colors,
            columns,
            above,
            below,
            em: em as u16,
            top: (f64::from(above) * h + baseline - ascent).max(0.0) as u16,
        })
    }

    pub fn rows(&self) -> u16 {
        self.above + self.below + 1
    }

    pub fn rasterize(&self) -> Option<RgbaImage> {
        let list = typeset(&self.source, self.display, self.colors.foreground)?;
        let options = RenderOptions {
            font_size: f32::from(self.em),
            padding: 1.0,
            background_color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
            ..Default::default()
        };
        let png = ratex_render::render_to_png(&list, &options).ok()?;
        let ink = image::load_from_memory_with_format(&png, ImageFormat::Png)
            .ok()?
            .into_rgba8();
        let mut canvas = RgbaImage::new(
            u32::from(self.columns) * u32::from(self.cell.width),
            u32::from(self.rows()) * u32::from(self.cell.height),
        );
        image::imageops::overlay(&mut canvas, &ink, 0, i64::from(self.top));
        Some(canvas)
    }
}

pub(super) fn png(pixels: RgbaImage) -> Option<Vec<u8>> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels)
        .write_to(&mut output, ImageFormat::Png)
        .ok()?;
    Some(output.into_inner())
}
