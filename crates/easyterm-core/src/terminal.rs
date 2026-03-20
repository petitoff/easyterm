use crate::ansi::{parse_ansi, AnsiEvent, ClearMode, Color, DecMode, Style};
use crate::grid::{Cell, Cursor, Grid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseReportingMode {
    #[default]
    Off,
    Click,
    Drag,
    Motion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerminalModes {
    pub application_cursor_keys: bool,
    pub mouse_reporting: MouseReportingMode,
    pub sgr_mouse: bool,
    pub alternate_screen: bool,
    pub bracketed_paste: bool,
}

#[derive(Debug, Clone)]
struct ScreenBuffer {
    grid: Grid,
    cursor: Cursor,
    saved_cursor: Cursor,
    active_style: Style,
    pending_wrap: bool,
    scroll_region: (usize, usize),
}

impl ScreenBuffer {
    fn new(width: usize, height: usize) -> Self {
        Self {
            grid: Grid::new(width, height),
            cursor: Cursor { row: 0, col: 0 },
            saved_cursor: Cursor { row: 0, col: 0 },
            active_style: Style::default(),
            pending_wrap: false,
            scroll_region: (0, height.saturating_sub(1)),
        }
    }

    fn resize(&mut self, width: usize, height: usize) {
        self.grid.resize(width, height);
        self.cursor.row = self.cursor.row.min(self.grid.height().saturating_sub(1));
        self.cursor.col = self.cursor.col.min(self.grid.width().saturating_sub(1));
        self.saved_cursor.row = self
            .saved_cursor
            .row
            .min(self.grid.height().saturating_sub(1));
        self.saved_cursor.col = self
            .saved_cursor
            .col
            .min(self.grid.width().saturating_sub(1));
        self.scroll_region.0 = self
            .scroll_region
            .0
            .min(self.grid.height().saturating_sub(1));
        self.scroll_region.1 = self
            .scroll_region
            .1
            .min(self.grid.height().saturating_sub(1));
        if self.scroll_region.0 > self.scroll_region.1 {
            self.scroll_region = (0, self.grid.height().saturating_sub(1));
        }
        if self.grid.width() == 1 && self.cursor.col == 0 {
            self.pending_wrap = false;
        }
    }

    fn reset(&mut self) {
        let width = self.grid.width();
        let height = self.grid.height();
        *self = Self::new(width, height);
    }
}

#[derive(Debug, Clone)]
pub struct Terminal {
    primary: ScreenBuffer,
    alternate: ScreenBuffer,
    scrollback: Vec<String>,
    window_title: String,
    modes: TerminalModes,
}

impl Terminal {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            primary: ScreenBuffer::new(width, height),
            alternate: ScreenBuffer::new(width, height),
            scrollback: Vec::new(),
            window_title: String::new(),
            modes: TerminalModes::default(),
        }
    }

    pub fn grid(&self) -> &Grid {
        &self.active_buffer().grid
    }

    pub fn cursor(&self) -> Cursor {
        self.active_buffer().cursor
    }

    pub fn active_style(&self) -> Style {
        self.active_buffer().active_style
    }

    pub fn scrollback(&self) -> &[String] {
        &self.scrollback
    }

    pub fn view_scrollback(&self) -> &[String] {
        if self.modes.alternate_screen {
            &[]
        } else {
            &self.scrollback
        }
    }

    pub fn window_title(&self) -> &str {
        &self.window_title
    }

    pub fn modes(&self) -> TerminalModes {
        self.modes
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.primary.resize(width, height);
        self.alternate.resize(width, height);
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for event in parse_ansi(bytes) {
            self.apply(event);
        }
    }

    pub fn visible_lines(&self) -> Vec<String> {
        self.grid().snapshot()
    }

    fn active_buffer(&self) -> &ScreenBuffer {
        if self.modes.alternate_screen {
            &self.alternate
        } else {
            &self.primary
        }
    }

    fn active_buffer_mut(&mut self) -> &mut ScreenBuffer {
        if self.modes.alternate_screen {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    fn apply(&mut self, event: AnsiEvent) {
        match event {
            AnsiEvent::Print(ch) => self.put_char(ch),
            AnsiEvent::NewLine => {
                self.active_buffer_mut().pending_wrap = false;
                self.new_line();
            }
            AnsiEvent::CarriageReturn => {
                let buffer = self.active_buffer_mut();
                buffer.pending_wrap = false;
                buffer.cursor.col = 0;
            }
            AnsiEvent::Backspace => {
                self.active_buffer_mut().cursor.col =
                    self.active_buffer().cursor.col.saturating_sub(1)
            }
            AnsiEvent::Index => self.index(),
            AnsiEvent::ReverseIndex => self.reverse_index(),
            AnsiEvent::NextLine => {
                self.index();
                self.active_buffer_mut().cursor.col = 0;
            }
            AnsiEvent::SaveCursor => {
                self.active_buffer_mut().saved_cursor = self.active_buffer().cursor
            }
            AnsiEvent::RestoreCursor => {
                self.active_buffer_mut().cursor = self.active_buffer().saved_cursor
            }
            AnsiEvent::CursorUp(steps) => {
                self.active_buffer_mut().cursor.row = self
                    .active_buffer()
                    .cursor
                    .row
                    .saturating_sub(steps as usize);
            }
            AnsiEvent::CursorDown(steps) => {
                let height = self.grid().height();
                let row = self.active_buffer().cursor.row;
                self.active_buffer_mut().cursor.row =
                    (row + steps as usize).min(height.saturating_sub(1));
            }
            AnsiEvent::CursorForward(steps) => {
                let width = self.grid().width();
                let col = self.active_buffer().cursor.col;
                self.active_buffer_mut().cursor.col =
                    (col + steps as usize).min(width.saturating_sub(1));
            }
            AnsiEvent::CursorBackward(steps) => {
                self.active_buffer_mut().cursor.col = self
                    .active_buffer()
                    .cursor
                    .col
                    .saturating_sub(steps as usize);
            }
            AnsiEvent::CursorPosition { row, col } => {
                let width = self.grid().width();
                let height = self.grid().height();
                let buffer = self.active_buffer_mut();
                buffer.cursor.row = (row.saturating_sub(1) as usize).min(height.saturating_sub(1));
                buffer.cursor.col = (col.saturating_sub(1) as usize).min(width.saturating_sub(1));
            }
            AnsiEvent::SetScrollRegion { top, bottom } => self.set_scroll_region(top, bottom),
            AnsiEvent::InsertLines(count) => self.insert_lines(count as usize),
            AnsiEvent::DeleteLines(count) => self.delete_lines(count as usize),
            AnsiEvent::ClearLine(mode) => self.clear_line(mode),
            AnsiEvent::ClearScreen(mode) => self.clear_screen(mode),
            AnsiEvent::SetStyle(params) => self.apply_sgr(&params),
            AnsiEvent::SetWindowTitle(title) => self.window_title = title,
            AnsiEvent::SetDecMode { mode, enabled } => self.apply_dec_mode(mode, enabled),
        }
    }

    fn put_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }

        if self.active_buffer().pending_wrap {
            self.active_buffer_mut().pending_wrap = false;
            self.new_line();
        }

        let width = char_width(ch);
        if width >= self.grid().width() {
            return;
        }

        if self.active_buffer().cursor.col + width > self.grid().width() {
            self.new_line();
        }

        let style = self.active_buffer().active_style;
        let row = self.active_buffer().cursor.row;
        let col = self.active_buffer().cursor.col;
        let buffer = self.active_buffer_mut();

        if let Some(cell) = buffer.grid.get_mut(row, col) {
            *cell = Cell {
                text: ch.to_string(),
                style,
                wide_continuation: false,
            };
        }

        if width == 2 {
            if let Some(next) = buffer.grid.get_mut(row, col + 1) {
                *next = Cell {
                    text: String::new(),
                    style,
                    wide_continuation: true,
                };
            }
        }

        buffer.cursor.col += width;
        if buffer.cursor.col >= buffer.grid.width() {
            buffer.cursor.col = buffer.grid.width().saturating_sub(1);
            buffer.pending_wrap = true;
        }
    }

    fn new_line(&mut self) {
        self.index();
        self.active_buffer_mut().cursor.col = 0;
    }

    fn index(&mut self) {
        let row = self.active_buffer().cursor.row;
        let (_, bottom) = self.active_buffer().scroll_region;
        if row >= bottom {
            self.scroll_up_region();
        } else {
            self.active_buffer_mut().cursor.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        let row = self.active_buffer().cursor.row;
        let (top, _) = self.active_buffer().scroll_region;
        if row <= top {
            self.scroll_down_region();
        } else {
            self.active_buffer_mut().cursor.row -= 1;
        }
    }

    fn scroll_up_region(&mut self) {
        let (top, bottom) = self.active_buffer().scroll_region;
        if !self.modes.alternate_screen && top == 0 && bottom + 1 == self.primary.grid.height() {
            self.scrollback.push(self.primary.grid.row_text(top));
        }

        let buffer = self.active_buffer_mut();
        for row in top..bottom {
            buffer.grid.copy_row(row + 1, row);
        }
        buffer.grid.clear_row(bottom);
    }

    fn scroll_down_region(&mut self) {
        let (top, bottom) = self.active_buffer().scroll_region;
        let buffer = self.active_buffer_mut();
        for row in (top + 1..=bottom).rev() {
            buffer.grid.copy_row(row - 1, row);
        }
        buffer.grid.clear_row(top);
    }

    fn insert_lines(&mut self, count: usize) {
        let row = self.active_buffer().cursor.row;
        let (top, bottom) = self.active_buffer().scroll_region;
        if row < top || row > bottom {
            return;
        }

        let count = count.min(bottom - row + 1);
        let buffer = self.active_buffer_mut();
        for target_row in (row + count..=bottom).rev() {
            buffer.grid.copy_row(target_row - count, target_row);
        }
        for clear_row in row..row + count {
            buffer.grid.clear_row(clear_row);
        }
    }

    fn delete_lines(&mut self, count: usize) {
        let row = self.active_buffer().cursor.row;
        let (top, bottom) = self.active_buffer().scroll_region;
        if row < top || row > bottom {
            return;
        }

        let count = count.min(bottom - row + 1);
        let buffer = self.active_buffer_mut();
        for target_row in row..=bottom.saturating_sub(count) {
            buffer.grid.copy_row(target_row + count, target_row);
        }
        for clear_row in bottom.saturating_sub(count).saturating_add(1)..=bottom {
            buffer.grid.clear_row(clear_row);
        }
    }

    fn clear_line(&mut self, mode: ClearMode) {
        let row = self.active_buffer().cursor.row;
        let col = self.active_buffer().cursor.col;
        let buffer = self.active_buffer_mut();

        match mode {
            ClearMode::ToEnd => buffer.grid.clear_row_from(row, col),
            ClearMode::ToStart => buffer.grid.clear_row_to(row, col),
            ClearMode::All => buffer.grid.clear_row(row),
        }
    }

    fn clear_screen(&mut self, mode: ClearMode) {
        let row = self.active_buffer().cursor.row;
        match mode {
            ClearMode::All => self.active_buffer_mut().grid.clear(),
            ClearMode::ToEnd => {
                self.clear_line(ClearMode::ToEnd);
                let height = self.grid().height();
                let buffer = self.active_buffer_mut();
                for clear_row in row + 1..height {
                    buffer.grid.clear_row(clear_row);
                }
            }
            ClearMode::ToStart => {
                self.clear_line(ClearMode::ToStart);
                let buffer = self.active_buffer_mut();
                for clear_row in 0..row {
                    buffer.grid.clear_row(clear_row);
                }
            }
        }
    }

    fn apply_sgr(&mut self, params: &[u16]) {
        let mut iter = params.iter().copied().peekable();
        let buffer = self.active_buffer_mut();

        while let Some(param) = iter.next() {
            match param {
                0 => buffer.active_style = Style::default(),
                1 => buffer.active_style.bold = true,
                3 => buffer.active_style.italic = true,
                4 => buffer.active_style.underline = true,
                22 => buffer.active_style.bold = false,
                23 => buffer.active_style.italic = false,
                24 => buffer.active_style.underline = false,
                30..=37 => buffer.active_style.fg = Color::Indexed((param - 30) as u8),
                39 => buffer.active_style.fg = Color::Default,
                40..=47 => buffer.active_style.bg = Color::Indexed((param - 40) as u8),
                49 => buffer.active_style.bg = Color::Default,
                90..=97 => buffer.active_style.fg = Color::Indexed((param - 82) as u8),
                100..=107 => buffer.active_style.bg = Color::Indexed((param - 92) as u8),
                38 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        buffer.active_style.fg = color;
                    }
                }
                48 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        buffer.active_style.bg = color;
                    }
                }
                _ => {}
            }
        }
    }

    fn set_scroll_region(&mut self, top: Option<u16>, bottom: Option<u16>) {
        let height = self.grid().height();
        let top = top.unwrap_or(1).saturating_sub(1) as usize;
        let bottom = bottom.unwrap_or(height as u16).saturating_sub(1) as usize;
        let buffer = self.active_buffer_mut();
        if top < height && bottom < height && top < bottom {
            buffer.scroll_region = (top, bottom);
        } else {
            buffer.scroll_region = (0, height.saturating_sub(1));
        }
        buffer.cursor = Cursor { row: 0, col: 0 };
    }

    fn apply_dec_mode(&mut self, mode: DecMode, enabled: bool) {
        match mode {
            DecMode::ApplicationCursorKeys => self.modes.application_cursor_keys = enabled,
            DecMode::MouseClickReporting => {
                self.modes.mouse_reporting = if enabled {
                    MouseReportingMode::Click
                } else if self.modes.mouse_reporting == MouseReportingMode::Click {
                    MouseReportingMode::Off
                } else {
                    self.modes.mouse_reporting
                };
            }
            DecMode::MouseDragReporting => {
                self.modes.mouse_reporting = if enabled {
                    MouseReportingMode::Drag
                } else if self.modes.mouse_reporting == MouseReportingMode::Drag {
                    MouseReportingMode::Off
                } else {
                    self.modes.mouse_reporting
                };
            }
            DecMode::MouseMotionReporting => {
                self.modes.mouse_reporting = if enabled {
                    MouseReportingMode::Motion
                } else if self.modes.mouse_reporting == MouseReportingMode::Motion {
                    MouseReportingMode::Off
                } else {
                    self.modes.mouse_reporting
                };
            }
            DecMode::SgrMouse => self.modes.sgr_mouse = enabled,
            DecMode::AlternateScreen => {
                self.modes.alternate_screen = enabled;
                if enabled {
                    self.alternate.reset();
                }
            }
            DecMode::BracketedPaste => self.modes.bracketed_paste = enabled,
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
    use super::{MouseReportingMode, Terminal, TerminalModes};
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

    #[test]
    fn alternate_screen_hides_primary_scrollback() {
        let mut terminal = Terminal::new(4, 2);
        terminal.feed(b"one\ntwo\n");
        terminal.feed(b"\x1b[?1049h");
        terminal.feed(b"vim");

        assert!(terminal.modes().alternate_screen);
        assert!(terminal.view_scrollback().is_empty());
        assert_eq!(terminal.visible_lines(), vec!["vim", ""]);
    }

    #[test]
    fn alternate_screen_restores_primary_content() {
        let mut terminal = Terminal::new(5, 2);
        terminal.feed(b"shell");
        terminal.feed(b"\x1b[?1049h");
        terminal.feed(b"vim");
        terminal.feed(b"\x1b[?1049l");

        assert!(!terminal.modes().alternate_screen);
        assert_eq!(terminal.visible_lines(), vec!["shell", ""]);
    }

    #[test]
    fn tracks_terminal_modes() {
        let mut terminal = Terminal::new(8, 2);
        terminal.feed(b"\x1b[?1;1002;1006;2004h");

        assert_eq!(
            terminal.modes(),
            TerminalModes {
                application_cursor_keys: true,
                mouse_reporting: MouseReportingMode::Drag,
                sgr_mouse: true,
                alternate_screen: false,
                bracketed_paste: true,
            }
        );
    }

    #[test]
    fn ignores_charset_designation_sequences() {
        let mut terminal = Terminal::new(10, 2);
        terminal.feed(b"\x1b(Bhello");

        assert_eq!(terminal.visible_lines(), vec!["hello", ""]);
    }

    #[test]
    fn save_restore_cursor_restores_previous_position() {
        let mut terminal = Terminal::new(8, 3);
        terminal.feed(b"top");
        terminal.feed(b"\x1b7\x1b[2;1Hxx\x1b8Z");

        assert_eq!(terminal.visible_lines(), vec!["topZ", "xx", ""]);
    }

    #[test]
    fn reverse_index_scrolls_down_at_top_of_region() {
        let mut terminal = Terminal::new(8, 3);
        terminal.feed(b"top\nmid\nbot");
        terminal.feed(b"\x1b[1;1H\x1bM");

        assert_eq!(terminal.visible_lines(), vec!["", "top", "mid"]);
    }

    #[test]
    fn scroll_region_and_line_ops_shift_lines_inside_region() {
        let mut terminal = Terminal::new(6, 5);
        terminal.feed(b"one\ntwo\nthr\nfur\nfiv");
        terminal.feed(b"\r\x1b[2;4r\x1b[2;1H\x1b[M");

        assert_eq!(
            terminal.visible_lines(),
            vec!["one", "thr", "fur", "", "fiv"]
        );
    }
}
