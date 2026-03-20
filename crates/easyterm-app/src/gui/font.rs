use crate::gui::canvas::Canvas;
use ab_glyph::{point, Font, FontArc, Glyph, PxScale, ScaleFont};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use std::cell::RefCell;
use std::cmp::max;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(crate) struct FontRenderer {
    backend: FontBackend,
    cell_width: usize,
    cell_height: usize,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TextStyle {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
}

impl FontRenderer {
    pub(crate) fn new(font_size: u16, family: &str) -> Self {
        let backend = load_outline_font(family, font_size).unwrap_or_else(|| {
            let glyph_scale = max(1, ((font_size as usize) + 7) / 8);
            FontBackend::Bitmap { glyph_scale }
        });

        let (cell_width, cell_height) = match &backend {
            FontBackend::Outline(outline) => (outline.cell_width, outline.cell_height),
            FontBackend::Bitmap { glyph_scale } => (8 * glyph_scale, 10 * glyph_scale),
        };

        Self {
            backend,
            cell_width,
            cell_height,
        }
    }

    pub(crate) fn cell_width(&self) -> usize {
        self.cell_width
    }

    pub(crate) fn cell_height(&self) -> usize {
        self.cell_height
    }

    pub(crate) fn draw_char(
        &mut self,
        canvas: &mut Canvas<'_>,
        x: usize,
        y: usize,
        ch: char,
        fg: u32,
        bg: Option<u32>,
        style: TextStyle,
    ) {
        match &self.backend {
            FontBackend::Outline(outline) => {
                let Some(glyph) = outline.rasterized(ch) else {
                    return;
                };
                if let Some(bg) = bg {
                    canvas.fill_rect(x, y, self.cell_width, self.cell_height, bg);
                }
                for py in 0..glyph.height {
                    for px in 0..glyph.width {
                        let alpha = glyph.pixels[py * glyph.width + px];
                        if alpha == 0 {
                            continue;
                        }
                        let italic_slant = if style.italic {
                            italic_offset(py, glyph.height)
                        } else {
                            0
                        };
                        let target_x =
                            x as isize + glyph.offset_x + px as isize + italic_slant as isize;
                        let target_y = y as isize + glyph.offset_y + py as isize;
                        canvas.blend_pixel(target_x, target_y, fg, alpha);
                        if style.bold {
                            canvas.blend_pixel(target_x + 1, target_y, fg, alpha);
                        }
                    }
                }
            }
            FontBackend::Bitmap { glyph_scale } => {
                let bitmap = BASIC_FONTS.get(ch).or_else(|| BASIC_FONTS.get('?'));
                if let Some(bg) = bg {
                    canvas.fill_rect(x, y, 8 * glyph_scale, 8 * glyph_scale, bg);
                }
                let Some(bitmap) = bitmap else {
                    return;
                };

                for (row, bits) in bitmap.iter().copied().enumerate() {
                    for col in 0..8 {
                        if bits & (1 << col) == 0 {
                            continue;
                        }
                        let px = x
                            + col as usize * glyph_scale
                            + italic_offset(row, bitmap.len()) * glyph_scale;
                        let py = y + row * glyph_scale;
                        canvas.fill_rect(px, py, *glyph_scale, *glyph_scale, fg);
                        if style.bold {
                            canvas.fill_rect(px + 1, py, *glyph_scale, *glyph_scale, fg);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn text_step(&self, ch: char) -> usize {
        match &self.backend {
            FontBackend::Outline(outline) => outline.advance(ch),
            FontBackend::Bitmap { .. } => self.cell_width / 2,
        }
    }

    pub(crate) fn measure_text(&self, text: &str) -> usize {
        text.chars().map(|ch| self.text_step(ch)).sum()
    }

    pub(crate) fn underline_thickness(&self) -> usize {
        match &self.backend {
            FontBackend::Outline(outline) => max(1, outline.cell_height / 14),
            FontBackend::Bitmap { glyph_scale } => max(1, *glyph_scale),
        }
    }
}

enum FontBackend {
    Outline(OutlineFont),
    Bitmap { glyph_scale: usize },
}

struct OutlineFont {
    font: FontArc,
    scale: PxScale,
    baseline: f32,
    cell_width: usize,
    cell_height: usize,
    cache: RefCell<HashMap<char, RasterizedGlyph>>,
}

impl OutlineFont {
    fn new(font: FontArc, size: u16) -> Self {
        let scale = PxScale::from(size as f32 * 1.2);
        let scaled = font.as_scaled(scale);
        let baseline = scaled.ascent().ceil();
        let descent = scaled.descent().abs().ceil();
        let line_gap = scaled.line_gap().ceil();
        let cell_width = scaled
            .h_advance(font.glyph_id('M'))
            .max(scaled.h_advance(font.glyph_id('W')))
            .ceil() as usize
            + 2;
        let cell_height = (baseline + descent + line_gap).ceil() as usize + 2;

        Self {
            font,
            scale,
            baseline,
            cell_width,
            cell_height,
            cache: RefCell::new(HashMap::new()),
        }
    }

    fn advance(&self, ch: char) -> usize {
        let scaled = self.font.as_scaled(self.scale);
        let id = self.font.glyph_id(ch);
        max(1, scaled.h_advance(id).ceil() as usize)
    }

    fn rasterized(&self, ch: char) -> Option<RasterizedGlyph> {
        if let Some(cached) = self.cache.borrow().get(&ch).cloned() {
            return Some(cached);
        }

        let glyph = self.rasterize_char(ch)?;
        self.cache.borrow_mut().insert(ch, glyph.clone());
        Some(glyph)
    }

    fn rasterize_char(&self, ch: char) -> Option<RasterizedGlyph> {
        let glyph = Glyph {
            id: self.font.glyph_id(ch),
            scale: self.scale,
            position: point(0.0, self.baseline),
        };
        let outlined = self.font.outline_glyph(glyph).or_else(|| {
            self.font.outline_glyph(Glyph {
                id: self.font.glyph_id('?'),
                scale: self.scale,
                position: point(0.0, self.baseline),
            })
        })?;

        let bounds = outlined.px_bounds();
        let width = (bounds.max.x - bounds.min.x).max(0.0).ceil() as usize;
        let height = (bounds.max.y - bounds.min.y).max(0.0).ceil() as usize;
        let mut pixels = vec![0_u8; width.saturating_mul(height)];
        if width > 0 && height > 0 {
            outlined.draw(|x, y, coverage| {
                let idx = y as usize * width + x as usize;
                pixels[idx] = (coverage * 255.0) as u8;
            });
        }

        Some(RasterizedGlyph {
            width,
            height,
            offset_x: bounds.min.x.floor() as isize + 1,
            offset_y: bounds.min.y.floor() as isize + 1,
            pixels,
        })
    }
}

#[derive(Clone)]
struct RasterizedGlyph {
    width: usize,
    height: usize,
    offset_x: isize,
    offset_y: isize,
    pixels: Vec<u8>,
}

fn load_outline_font(family: &str, size: u16) -> Option<FontBackend> {
    for path in font_candidates(family) {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(font) = FontArc::try_from_vec(bytes) {
                return Some(FontBackend::Outline(OutlineFont::new(font, size)));
            }
        }
    }

    None
}

fn italic_offset(row: usize, total_rows: usize) -> usize {
    total_rows.saturating_sub(row.saturating_add(1)).min(3) / 2
}

fn font_candidates(family: &str) -> Vec<PathBuf> {
    let family_lower = family.to_ascii_lowercase();
    let mut paths = Vec::new();

    if family_lower.contains("iosevka") {
        paths.extend([
            "/usr/share/fonts/truetype/iosevka/IosevkaTerm-Regular.ttf",
            "/usr/share/fonts/TTF/IosevkaTerm-Regular.ttf",
            "/usr/local/share/fonts/IosevkaTerm-Regular.ttf",
        ]);
    }
    if family_lower.contains("noto") {
        paths.push("/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf");
    }
    if family_lower.contains("dejavu") {
        paths.push("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf");
    }
    if family_lower.contains("liberation") {
        paths.push("/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf");
    }

    paths.extend([
        "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/freefont/FreeMono.ttf",
    ]);

    paths
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| Path::new(path).exists())
        .collect()
}
