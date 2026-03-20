use crate::gui::canvas::Canvas;
use crate::gui::font::{FontRenderer, TextStyle};
use crate::gui::tab::{CellPoint, GuiTab};
use crate::pty::PtySize;
use easyterm_core::{Cell, Color, Cursor};
use std::cmp::max;
use winit::dpi::{PhysicalPosition, PhysicalSize};

const TAB_PADDING_X: usize = 12;
const TAB_GAP: usize = 8;
const TAB_MIN_WIDTH: usize = 120;

pub(crate) struct RendererState {
    font: FontRenderer,
    tab_height: usize,
}

impl RendererState {
    pub(crate) fn new(font_size: u16, family: &str) -> Self {
        let font = FontRenderer::new(font_size, family);
        let tab_height = font.cell_height() + 10;
        Self { font, tab_height }
    }

    pub(crate) fn tab_height(&self) -> usize {
        self.tab_height
    }

    pub(crate) fn terminal_size(&self, size: PhysicalSize<u32>) -> PtySize {
        let cols = max(1, size.width as usize / self.font.cell_width()) as u16;
        let rows = max(
            1,
            size.height.saturating_sub(self.tab_height as u32) as usize / self.font.cell_height(),
        ) as u16;
        PtySize { cols, rows }
    }

    pub(crate) fn layout_tabs(&self, size: PhysicalSize<u32>, tabs: &[GuiTab]) -> Vec<TabLayout> {
        let mut x = 0usize;
        let width_limit = size.width as usize;
        let mut layouts = Vec::new();

        for (index, tab) in tabs.iter().enumerate() {
            let title = tab.title();
            let text_width = self.font.measure_text(title.as_ref());
            let width = max(TAB_MIN_WIDTH, text_width + TAB_PADDING_X * 2);
            if x >= width_limit {
                break;
            }
            layouts.push(TabLayout { index, x, width });
            x += width + TAB_GAP;
        }

        layouts
    }

    pub(crate) fn point_to_cell(
        &self,
        position: PhysicalPosition<f64>,
        size: PhysicalSize<u32>,
        tab: &GuiTab,
    ) -> Option<CellPoint> {
        if position.y < self.tab_height as f64 {
            return None;
        }
        let col = (position.x.max(0.0) as usize) / self.font.cell_width();
        let row = ((position.y as usize).saturating_sub(self.tab_height)) / self.font.cell_height();
        if col >= tab.terminal().grid().width() || row >= self.terminal_size(size).rows as usize {
            return None;
        }

        Some(CellPoint {
            global_row: tab.viewport_start(tab.terminal().grid().height()) + row,
            col,
        })
    }

    pub(crate) fn render(
        &mut self,
        canvas: &mut Canvas<'_>,
        tabs: &[GuiTab],
        active_tab: usize,
        error: Option<&str>,
        cursor_visible: bool,
    ) {
        canvas.clear(rgb(14, 16, 20));
        self.render_tab_bar(canvas, tabs, active_tab);

        if let Some(tab) = tabs.get(active_tab) {
            self.render_terminal(canvas, tab, cursor_visible);
        }

        if let Some(message) = error {
            canvas.fill_rect(
                0,
                canvas.height().saturating_sub(self.font.cell_height() * 2),
                canvas.width(),
                self.font.cell_height() * 2,
                rgb(128, 30, 30),
            );
            self.draw_text(
                canvas,
                10,
                canvas.height().saturating_sub(self.font.cell_height() * 2) + 6,
                message,
                rgb(250, 230, 230),
                None,
            );
        }
    }

    fn render_tab_bar(&mut self, canvas: &mut Canvas<'_>, tabs: &[GuiTab], active_tab: usize) {
        canvas.fill_rect(0, 0, canvas.width(), self.tab_height, rgb(24, 26, 32));

        for layout in self.layout_tabs(
            PhysicalSize::new(canvas.width() as u32, canvas.height() as u32),
            tabs,
        ) {
            let active = layout.index == active_tab;
            let title = tabs[layout.index].title();
            let bg = if active {
                rgb(42, 48, 59)
            } else {
                rgb(28, 31, 38)
            };
            canvas.fill_rect(
                layout.x,
                4,
                layout.width,
                self.tab_height.saturating_sub(8),
                bg,
            );
            self.draw_text(
                canvas,
                layout.x + TAB_PADDING_X,
                7,
                title.as_ref(),
                rgb(224, 229, 236),
                None,
            );
        }
    }

    fn render_terminal(&mut self, canvas: &mut Canvas<'_>, tab: &GuiTab, cursor_visible: bool) {
        let cols = tab.terminal().grid().width();
        let rows = tab.terminal().grid().height();
        let viewport_start = tab.viewport_start(rows);
        let scrollback_len = tab.terminal().view_scrollback().len();

        for row in 0..rows {
            let global_row = viewport_start + row;
            let y = self.tab_height + row * self.font.cell_height();
            for col in 0..cols {
                let x = col * self.font.cell_width();
                let mut attrs = ResolvedCell::default();

                if let Some(cell) = cell_at(tab, global_row, scrollback_len, col) {
                    attrs = resolve_cell(cell);
                }

                if tab.allows_local_selection()
                    && tab.selection_contains(CellPoint { global_row, col })
                {
                    attrs.bg = rgb(52, 78, 116);
                }

                canvas.fill_rect(
                    x,
                    y,
                    self.font.cell_width(),
                    self.font.cell_height(),
                    attrs.bg,
                );
                if attrs.ch != ' ' && !attrs.hidden {
                    self.font.draw_char(
                        canvas,
                        x,
                        y,
                        attrs.ch,
                        attrs.fg,
                        Some(attrs.bg),
                        attrs.style,
                    );
                }
                if attrs.underline {
                    canvas.fill_rect(
                        x,
                        y + self
                            .font
                            .cell_height()
                            .saturating_sub(self.font.underline_thickness()),
                        self.font.cell_width(),
                        self.font.underline_thickness(),
                        attrs.fg,
                    );
                }
            }
        }

        if cursor_visible && tab.scroll_offset() == 0 {
            self.render_cursor(canvas, tab.terminal().cursor());
        }
    }

    fn render_cursor(&self, canvas: &mut Canvas<'_>, cursor: Cursor) {
        let x = cursor.col * self.font.cell_width();
        let y = self.tab_height + cursor.row * self.font.cell_height();
        canvas.stroke_rect(
            x,
            y,
            self.font.cell_width().saturating_sub(1),
            self.font.cell_height().saturating_sub(1),
            rgb(244, 197, 66),
        );
    }

    fn draw_text(
        &mut self,
        canvas: &mut Canvas<'_>,
        x: usize,
        y: usize,
        text: &str,
        fg: u32,
        bg: Option<u32>,
    ) {
        let mut pen_x = x;
        for ch in text.chars() {
            self.font
                .draw_char(canvas, pen_x, y, ch, fg, bg, TextStyle::default());
            pen_x += self.font.text_step(ch);
        }
    }
}

pub(crate) struct TabLayout {
    pub(crate) index: usize,
    pub(crate) x: usize,
    pub(crate) width: usize,
}

fn renderable_cell(cell: &Cell) -> char {
    if cell.wide_continuation {
        return ' ';
    }
    cell.text.chars().next().unwrap_or(' ')
}

fn cell_at(tab: &GuiTab, global_row: usize, scrollback_len: usize, col: usize) -> Option<&Cell> {
    if global_row < scrollback_len {
        return tab.terminal().scrollback_row(global_row)?.get(col);
    }

    let grid_row = global_row.checked_sub(scrollback_len)?;
    tab.terminal().grid().get(grid_row, col)
}

#[derive(Clone, Copy)]
struct ResolvedCell {
    ch: char,
    fg: u32,
    bg: u32,
    underline: bool,
    hidden: bool,
    style: TextStyle,
}

impl Default for ResolvedCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: rgb(224, 229, 236),
            bg: rgb(14, 16, 20),
            underline: false,
            hidden: false,
            style: TextStyle::default(),
        }
    }
}

fn resolve_cell(cell: &Cell) -> ResolvedCell {
    let mut fg = resolve_color(cell.style.fg, rgb(224, 229, 236));
    let mut bg = resolve_color(cell.style.bg, rgb(14, 16, 20));

    if cell.style.bold {
        fg = brighten_color(fg);
    }
    if cell.style.dim {
        fg = dim_color(fg, bg);
    }
    if cell.style.inverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.style.hidden {
        fg = bg;
    }

    ResolvedCell {
        ch: renderable_cell(cell),
        fg,
        bg,
        underline: cell.style.underline,
        hidden: cell.style.hidden,
        style: TextStyle {
            bold: cell.style.bold,
            italic: cell.style.italic,
        },
    }
}

fn resolve_color(color: Color, default: u32) -> u32 {
    match color {
        Color::Default => default,
        Color::Rgb(r, g, b) => rgb(r, g, b),
        Color::Indexed(value) => indexed_color(value),
    }
}

fn brighten_color(color: u32) -> u32 {
    transform_rgb(color, |component| component.saturating_add(32))
}

fn dim_color(color: u32, bg: u32) -> u32 {
    let (fg_r, fg_g, fg_b) = split_rgb(color);
    let (bg_r, bg_g, bg_b) = split_rgb(bg);
    rgb(
        ((fg_r as u16 * 2 + bg_r as u16) / 3) as u8,
        ((fg_g as u16 * 2 + bg_g as u16) / 3) as u8,
        ((fg_b as u16 * 2 + bg_b as u16) / 3) as u8,
    )
}

fn indexed_color(index: u8) -> u32 {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];

    match index {
        0..=15 => {
            let (r, g, b) = BASE[index as usize];
            rgb(r, g, b)
        }
        16..=231 => {
            let adjusted = index - 16;
            let r = adjusted / 36;
            let g = (adjusted % 36) / 6;
            let b = adjusted % 6;
            rgb(component_6cube(r), component_6cube(g), component_6cube(b))
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            rgb(value, value, value)
        }
    }
}

fn component_6cube(value: u8) -> u8 {
    match value {
        0 => 0,
        _ => 55 + value * 40,
    }
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn split_rgb(color: u32) -> (u8, u8, u8) {
    (
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
    )
}

fn transform_rgb(color: u32, map: impl Fn(u8) -> u8) -> u32 {
    let (r, g, b) = split_rgb(color);
    rgb(map(r), map(g), map(b))
}
