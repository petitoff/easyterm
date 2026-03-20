use std::cmp::min;

pub(crate) struct Canvas<'a> {
    buffer: &'a mut [u32],
    width: usize,
    height: usize,
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(buffer: &'a mut [u32], width: usize, height: usize) -> Self {
        Self {
            buffer,
            width,
            height,
        }
    }

    pub(crate) fn width(&self) -> usize {
        self.width
    }

    pub(crate) fn height(&self) -> usize {
        self.height
    }

    pub(crate) fn clear(&mut self, color: u32) {
        self.buffer.fill(color);
    }

    pub(crate) fn fill_rect(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        color: u32,
    ) {
        let x_end = min(self.width, x.saturating_add(width));
        let y_end = min(self.height, y.saturating_add(height));
        for py in y..y_end {
            let row = py * self.width;
            for px in x..x_end {
                self.buffer[row + px] = color;
            }
        }
    }

    pub(crate) fn stroke_rect(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        color: u32,
    ) {
        self.fill_rect(x, y, width, 1, color);
        self.fill_rect(x, y + height, width, 1, color);
        self.fill_rect(x, y, 1, height, color);
        self.fill_rect(x + width, y, 1, height + 1, color);
    }

    pub(crate) fn blend_pixel(&mut self, x: isize, y: isize, fg: u32, alpha: u8) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let idx = y as usize * self.width + x as usize;
        let bg = self.buffer[idx];
        self.buffer[idx] = blend_colors(bg, fg, alpha);
    }
}

fn blend_colors(bg: u32, fg: u32, alpha: u8) -> u32 {
    let alpha = alpha as u32;
    let inv = 255 - alpha;
    let bg_r = (bg >> 16) & 0xff;
    let bg_g = (bg >> 8) & 0xff;
    let bg_b = bg & 0xff;
    let fg_r = (fg >> 16) & 0xff;
    let fg_g = (fg >> 8) & 0xff;
    let fg_b = fg & 0xff;

    let r = (fg_r * alpha + bg_r * inv) / 255;
    let g = (fg_g * alpha + bg_g * inv) / 255;
    let b = (fg_b * alpha + bg_b * inv) / 255;
    (r << 16) | (g << 8) | b
}
