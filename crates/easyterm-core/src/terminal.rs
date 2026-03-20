use crate::ansi::{parse_ansi, AnsiEvent, ClearMode, Color, Style};
use crate::grid::{Cell, Cursor, Grid};

#[derive(Debug, Clone)]
pub struct Terminal {
    grid: Grid,
    cursor: Cursor,
    active_style: Style,
    scrollback: Vec<String>,
    window_title: String,
    pending_wrap: bool,
}

impl Terminal {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            grid: Grid::new(width, height),
            cursor: Cursor { row: 0, col: 0 },
            active_style: Style::default(),
            scrollback: Vec::new(),
            window_title: String::new(),
            pending_wrap: false,
        }
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn active_style(&self) -> Style {
        self.active_style
    }

    pub fn scrollback(&self) -> &[String] {
        &self.scrollback
    }

    pub fn window_title(&self) -> &str {
        &self.window_title
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.grid.resize(width, height);
        self.cursor.row = self.cursor.row.min(self.grid.height().saturating_sub(1));
        self.cursor.col = self.cursor.col.min(self.grid.width().saturating_sub(1));
        if self.grid.width() == 1 && self.cursor.col == 0 {
            self.pending_wrap = false;
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for event in parse_ansi(bytes) {
            self.apply(event);
        }
    }

    pub fn visible_lines(&self) -> Vec<String> {
        self.grid.snapshot()
    }

    fn apply(&mut self, event: AnsiEvent) {
        match event {
            AnsiEvent::Print(ch) => self.put_char(ch),
            AnsiEvent::NewLine => {
                self.pending_wrap = false;
                self.new_line();
            }
            AnsiEvent::CarriageReturn => {
                self.pending_wrap = false;
                self.cursor.col = 0;
            }
            AnsiEvent::Backspace => self.cursor.col = self.cursor.col.saturating_sub(1),
            AnsiEvent::CursorUp(steps) => {
                self.cursor.row = self.cursor.row.saturating_sub(steps as usize);
            }
            AnsiEvent::CursorDown(steps) => {
                self.cursor.row =
                    (self.cursor.row + steps as usize).min(self.grid.height().saturating_sub(1));
            }
            AnsiEvent::CursorForward(steps) => {
                self.cursor.col =
                    (self.cursor.col + steps as usize).min(self.grid.width().saturating_sub(1));
            }
            AnsiEvent::CursorBackward(steps) => {
                self.cursor.col = self.cursor.col.saturating_sub(steps as usize);
            }
            AnsiEvent::CursorPosition { row, col } => {
                self.cursor.row = row.saturating_sub(1) as usize;
                self.cursor.col = col.saturating_sub(1) as usize;
                self.cursor.row = self.cursor.row.min(self.grid.height().saturating_sub(1));
                self.cursor.col = self.cursor.col.min(self.grid.width().saturating_sub(1));
            }
            AnsiEvent::ClearLine(mode) => self.clear_line(mode),
            AnsiEvent::ClearScreen(mode) => self.clear_screen(mode),
            AnsiEvent::SetStyle(params) => self.apply_sgr(&params),
            AnsiEvent::SetWindowTitle(title) => self.window_title = title,
        }
    }

    fn put_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }

        if self.pending_wrap {
            self.pending_wrap = false;
            self.new_line();
        }

        let width = char_width(ch);
        if width >= self.grid.width() {
            return;
        }

        if self.cursor.col + width > self.grid.width() {
            self.new_line();
        }

        if let Some(cell) = self.grid.get_mut(self.cursor.row, self.cursor.col) {
            *cell = Cell {
                text: ch.to_string(),
                style: self.active_style,
                wide_continuation: false,
            };
        }

        if width == 2 {
            if let Some(next) = self.grid.get_mut(self.cursor.row, self.cursor.col + 1) {
                *next = Cell {
                    text: String::new(),
                    style: self.active_style,
                    wide_continuation: true,
                };
            }
        }

        self.cursor.col += width;
        if self.cursor.col >= self.grid.width() {
            self.cursor.col = self.grid.width().saturating_sub(1);
            self.pending_wrap = true;
        }
    }

    fn new_line(&mut self) {
        self.cursor.col = 0;
        if self.cursor.row + 1 >= self.grid.height() {
            self.scroll_up();
        } else {
            self.cursor.row += 1;
        }
    }

    fn scroll_up(&mut self) {
        self.scrollback.push(self.grid.row_text(0));
        let height = self.grid.height();
        let width = self.grid.width();

        for row in 0..height.saturating_sub(1) {
            for col in 0..width {
                let next = self.grid.get(row + 1, col).cloned().unwrap_or_default();
                if let Some(cell) = self.grid.get_mut(row, col) {
                    *cell = next;
                }
            }
        }

        self.grid.clear_row(height.saturating_sub(1));
    }

    fn clear_line(&mut self, mode: ClearMode) {
        match mode {
            ClearMode::ToEnd => self.grid.clear_row_from(self.cursor.row, self.cursor.col),
            ClearMode::ToStart => self.grid.clear_row_to(self.cursor.row, self.cursor.col),
            ClearMode::All => self.grid.clear_row(self.cursor.row),
        }
    }

    fn clear_screen(&mut self, mode: ClearMode) {
        match mode {
            ClearMode::All => self.grid.clear(),
            ClearMode::ToEnd => {
                self.clear_line(ClearMode::ToEnd);
                for row in self.cursor.row + 1..self.grid.height() {
                    self.grid.clear_row(row);
                }
            }
            ClearMode::ToStart => {
                self.clear_line(ClearMode::ToStart);
                for row in 0..self.cursor.row {
                    self.grid.clear_row(row);
                }
            }
        }
    }

    fn apply_sgr(&mut self, params: &[u16]) {
        let mut iter = params.iter().copied().peekable();

        while let Some(param) = iter.next() {
            match param {
                0 => self.active_style = Style::default(),
                1 => self.active_style.bold = true,
                3 => self.active_style.italic = true,
                4 => self.active_style.underline = true,
                22 => self.active_style.bold = false,
                23 => self.active_style.italic = false,
                24 => self.active_style.underline = false,
                30..=37 => self.active_style.fg = Color::Indexed((param - 30) as u8),
                39 => self.active_style.fg = Color::Default,
                40..=47 => self.active_style.bg = Color::Indexed((param - 40) as u8),
                49 => self.active_style.bg = Color::Default,
                90..=97 => self.active_style.fg = Color::Indexed((param - 82) as u8),
                100..=107 => self.active_style.bg = Color::Indexed((param - 92) as u8),
                38 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        self.active_style.fg = color;
                    }
                }
                48 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        self.active_style.bg = color;
                    }
                }
                _ => {}
            }
        }
    }
}

fn parse_extended_color<I>(iter: &mut std::iter::Peekable<I>) -> Option<Color>
where
    I: Iterator<Item = u16>,
{
    match iter.next()? {
        5 => iter.next().map(|value| Color::Indexed(value as u8)),
        2 => {
            let r = iter.next()? as u8;
            let g = iter.next()? as u8;
            let b = iter.next()? as u8;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

fn char_width(ch: char) -> usize {
    match ch as u32 {
        0x1100..=0x115F
        | 0x2329..=0x232A
        | 0x2E80..=0x303E
        | 0x3040..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::Terminal;
    use crate::ansi::Color;

    #[test]
    fn writes_and_wraps_text() {
        let mut terminal = Terminal::new(4, 2);
        terminal.feed(b"abcdZ");

        assert_eq!(terminal.visible_lines(), vec!["abcd", "Z"]);
        assert_eq!(terminal.cursor().row, 1);
    }

    #[test]
    fn applies_ansi_styles() {
        let mut terminal = Terminal::new(8, 2);
        terminal.feed(b"\x1b[38;2;1;2;3mA");

        let cell = terminal.grid().get(0, 0).unwrap();
        assert_eq!(cell.style.fg, Color::Rgb(1, 2, 3));
    }

    #[test]
    fn handles_cursor_movement_and_overwrite() {
        let mut terminal = Terminal::new(5, 2);
        terminal.feed(b"abc\x1b[2DXY");

        assert_eq!(terminal.visible_lines(), vec!["aXY", ""]);
    }

    #[test]
    fn clears_current_line() {
        let mut terminal = Terminal::new(5, 2);
        terminal.feed(b"abc\r\x1b[2K");

        assert_eq!(terminal.visible_lines(), vec!["", ""]);
    }

    #[test]
    fn preserves_wide_glyphs_in_snapshot() {
        let mut terminal = Terminal::new(4, 2);
        terminal.feed("界x".as_bytes());

        assert_eq!(terminal.visible_lines(), vec!["界x", ""]);
    }

    #[test]
    fn resize_clamps_cursor() {
        let mut terminal = Terminal::new(4, 4);
        terminal.feed(b"ab");
        terminal.resize(1, 1);

        assert_eq!(terminal.cursor().row, 0);
        assert_eq!(terminal.cursor().col, 0);
    }

    #[test]
    fn scrollback_collects_scrolled_lines() {
        let mut terminal = Terminal::new(3, 2);
        terminal.feed(b"one\ntwo\nthree");

        assert_eq!(terminal.scrollback(), &["one", "two"]);
        assert_eq!(terminal.visible_lines(), vec!["thr", "ee"]);
    }

    #[test]
    fn stores_window_title_from_osc() {
        let mut terminal = Terminal::new(8, 2);
        terminal.feed(b"\x1b]0;user@host: ~/repo\x07");

        assert_eq!(terminal.window_title(), "user@host: ~/repo");
    }
}
