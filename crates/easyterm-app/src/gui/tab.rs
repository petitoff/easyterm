use crate::pty::{spawn_local_runtime, LocalPtyError, PtyRuntime, PtySize};
use crate::session::LocalSessionSpec;
use easyterm_core::{Grid, MouseReportingMode, Terminal, TerminalModes};
use std::borrow::Cow;
use std::cmp::{max, min};
use std::process::ExitStatus;

pub(crate) struct GuiTab {
    fallback_title: String,
    runtime: PtyRuntime,
    terminal: Terminal,
    scroll_offset: usize,
    selection_anchor: Option<CellPoint>,
    selection_focus: Option<CellPoint>,
    scrollback_limit: usize,
    exit_status: Option<ExitStatus>,
}

impl GuiTab {
    pub(crate) fn new(
        title: String,
        spec: LocalSessionSpec,
        term: String,
        size: PtySize,
        scrollback_limit: usize,
    ) -> Result<Self, LocalPtyError> {
        let runtime = spawn_local_runtime(&spec, &term, size)?;
        Ok(Self {
            fallback_title: title,
            runtime,
            terminal: Terminal::new(size.cols as usize, size.rows as usize),
            scroll_offset: 0,
            selection_anchor: None,
            selection_focus: None,
            scrollback_limit,
            exit_status: None,
        })
    }

    pub(crate) fn title(&self) -> Cow<'_, str> {
        let dynamic = sanitize_title(self.terminal.window_title());
        if !dynamic.is_empty() {
            return dynamic;
        }

        let fallback = sanitize_title(&self.fallback_title);
        if fallback.is_empty() {
            Cow::Borrowed("shell")
        } else {
            fallback
        }
    }

    pub(crate) fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    pub(crate) fn terminal_modes(&self) -> TerminalModes {
        self.terminal.modes()
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub(crate) fn is_exited(&self) -> bool {
        self.exit_status.is_some()
    }

    pub(crate) fn send_input(&mut self, bytes: &[u8]) -> Result<(), LocalPtyError> {
        self.runtime.write_input(bytes)
    }

    pub(crate) fn resize(&mut self, size: PtySize) -> Result<(), LocalPtyError> {
        self.terminal.resize(size.cols as usize, size.rows as usize);
        self.runtime.resize(size)
    }

    pub(crate) fn drain_output(&mut self) -> bool {
        let mut changed = false;
        for chunk in self.runtime.drain_output() {
            self.terminal.feed(&chunk);
            changed = true;
        }

        if !self.allows_local_scrollback() {
            if self.scroll_offset != 0
                || self.selection_anchor.is_some()
                || self.selection_focus.is_some()
            {
                changed = true;
            }
            self.scroll_offset = 0;
            self.clear_selection();
        } else if self.terminal.scrollback().len() > self.scrollback_limit {
            let removed = self.terminal.trim_scrollback(self.scrollback_limit);
            if removed > 0 {
                changed = true;
                self.clear_selection();
            }
            self.scroll_offset = min(
                self.scroll_offset,
                self.max_scroll_offset(self.terminal.grid().height()),
            );
        }

        changed
    }

    pub(crate) fn refresh_exit_state(&mut self) -> Result<(), LocalPtyError> {
        if self.exit_status.is_none() {
            self.exit_status = self.runtime.try_wait()?;
        }
        Ok(())
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), LocalPtyError> {
        self.runtime.terminate()
    }

    pub(crate) fn scroll(&mut self, delta_lines: i32) {
        if !self.allows_local_scrollback() {
            return;
        }
        let max_offset = self.max_scroll_offset(self.terminal.grid().height());
        let updated = if delta_lines > 0 {
            self.scroll_offset.saturating_add(delta_lines as usize)
        } else {
            self.scroll_offset
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.scroll_offset = min(updated, max_offset);
    }

    pub(crate) fn viewport_start(&self, viewport_rows: usize) -> usize {
        self.total_lines()
            .saturating_sub(viewport_rows.saturating_add(self.scroll_offset))
    }

    pub(crate) fn begin_selection(&mut self, point: CellPoint) {
        if !self.allows_local_selection() {
            return;
        }
        self.selection_anchor = Some(point);
        self.selection_focus = Some(point);
    }

    pub(crate) fn update_selection(&mut self, point: CellPoint) {
        if !self.allows_local_selection() {
            return;
        }
        self.selection_focus = Some(point);
    }

    pub(crate) fn finish_selection(&mut self) {}

    pub(crate) fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_focus = None;
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        let (Some(anchor), Some(focus)) = (self.selection_anchor, self.selection_focus) else {
            return None;
        };

        let start = min(anchor, focus);
        let end = max(anchor, focus);
        let mut lines = Vec::new();

        for global_row in start.global_row..=end.global_row {
            let line = self.line_text(global_row)?;
            let line_len = line.chars().count();
            let start_col = if global_row == start.global_row {
                start.col.min(line_len)
            } else {
                0
            };
            let end_col = if global_row == end.global_row {
                end.col.min(line_len.saturating_sub(1))
            } else {
                line_len.saturating_sub(1)
            };

            if line_len == 0 || start_col > end_col {
                lines.push(String::new());
                continue;
            }

            lines.push(
                line.chars()
                    .skip(start_col)
                    .take(end_col - start_col + 1)
                    .collect(),
            );
        }

        Some(lines.join("\n"))
    }

    pub(crate) fn selection_contains(&self, point: CellPoint) -> bool {
        let (Some(anchor), Some(focus)) = (self.selection_anchor, self.selection_focus) else {
            return false;
        };

        let start = min(anchor, focus);
        let end = max(anchor, focus);
        if point.global_row < start.global_row || point.global_row > end.global_row {
            return false;
        }
        if start.global_row == end.global_row {
            return point.col >= start.col && point.col <= end.col;
        }
        if point.global_row == start.global_row {
            return point.col >= start.col;
        }
        if point.global_row == end.global_row {
            return point.col <= end.col;
        }
        true
    }

    fn total_lines(&self) -> usize {
        self.terminal.view_scrollback().len() + self.terminal.grid().height()
    }

    fn max_scroll_offset(&self, viewport_rows: usize) -> usize {
        self.total_lines().saturating_sub(viewport_rows)
    }

    fn line_text(&self, global_row: usize) -> Option<String> {
        let scrollback = self.terminal.view_scrollback();
        if global_row < scrollback.len() {
            return Some(Grid::cells_text(&scrollback[global_row]));
        }

        let grid_row = global_row.checked_sub(scrollback.len())?;
        if grid_row >= self.terminal.grid().height() {
            return None;
        }

        self.terminal.grid().row(grid_row).map(Grid::cells_text)
    }

    pub(crate) fn allows_local_scrollback(&self) -> bool {
        let modes = self.terminal.modes();
        !modes.alternate_screen && modes.mouse_reporting == MouseReportingMode::Off
    }

    pub(crate) fn allows_local_selection(&self) -> bool {
        self.allows_local_scrollback()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CellPoint {
    pub(crate) global_row: usize,
    pub(crate) col: usize,
}

fn sanitize_title(input: &str) -> Cow<'_, str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Cow::Borrowed("");
    }

    if trimmed.chars().all(is_supported_title_char) {
        return Cow::Borrowed(trimmed);
    }

    let mut sanitized = String::with_capacity(trimmed.len());
    let mut last_was_space = false;

    for ch in trimmed.chars() {
        if !is_supported_title_char(ch) {
            continue;
        }

        if ch.is_whitespace() {
            if last_was_space {
                continue;
            }
            sanitized.push(' ');
            last_was_space = true;
        } else {
            sanitized.push(ch);
            last_was_space = false;
        }
    }

    Cow::Owned(sanitized.trim().to_string())
}

fn is_supported_title_char(ch: char) -> bool {
    if ch.is_control() || ch == '\u{fffd}' {
        return false;
    }

    if matches!(
        ch as u32,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
    ) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::{sanitize_title, CellPoint, GuiTab};
    use crate::pty::PtySize;
    use crate::session::LocalSessionSpec;
    use std::borrow::Cow;

    #[test]
    fn keeps_plain_ascii_titles_untouched() {
        assert_eq!(
            sanitize_title("user@host: ~/repo"),
            Cow::Borrowed("user@host: ~/repo")
        );
    }

    #[test]
    fn strips_private_use_glyphs_from_titles() {
        assert_eq!(
            sanitize_title("user@host: \u{f115} ~/repo").as_ref(),
            "user@host: ~/repo"
        );
    }

    #[test]
    fn collapses_whitespace_after_sanitizing() {
        assert_eq!(
            sanitize_title("  user@host:\t\u{f115}   ~/repo  ").as_ref(),
            "user@host: ~/repo"
        );
    }

    #[test]
    fn extracts_selected_text_across_lines() {
        let spec = LocalSessionSpec::new("/bin/sh");
        let mut tab = GuiTab::new(
            "shell".into(),
            spec,
            "xterm-256color".into(),
            PtySize { cols: 8, rows: 3 },
            1000,
        )
        .unwrap();
        tab.shutdown().unwrap();
        tab.terminal.feed(b"alpha\nbeta\ngamma");

        tab.begin_selection(CellPoint {
            global_row: 0,
            col: 1,
        });
        tab.update_selection(CellPoint {
            global_row: 1,
            col: 2,
        });

        assert_eq!(tab.selected_text().as_deref(), Some("lpha\nbet"));
    }
}
