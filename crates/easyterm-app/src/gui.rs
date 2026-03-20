use crate::config::AppConfig;
use crate::pty::{spawn_local_runtime, LocalPtyError, PtyRuntime, PtySize};
use crate::session::LocalSessionSpec;
use ab_glyph::{point, Font, FontArc, Glyph, PxScale, ScaleFont};
use easyterm_core::{Cell, Color, Cursor, Terminal};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use softbuffer::{Context, Surface};
use std::cell::RefCell;
use std::cmp::{max, min};
use std::collections::HashMap;
use std::error::Error;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

const TAB_PADDING_X: usize = 12;
const TAB_GAP: usize = 8;
const TAB_MIN_WIDTH: usize = 120;
const CURSOR_BLINK_ON: Duration = Duration::from_millis(550);
const CURSOR_BLINK_OFF: Duration = Duration::from_millis(450);

pub fn run_gui(config: AppConfig) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = GuiApp::new(config);
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct GuiApp {
    config: AppConfig,
    window: Option<Rc<Window>>,
    context: Option<Context<Rc<Window>>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    renderer: RendererState,
    tabs: Vec<GuiTab>,
    active_tab: usize,
    modifiers: ModifiersState,
    cursor_position: Option<PhysicalPosition<f64>>,
    selecting: bool,
    last_error: Option<String>,
    cursor_blink_epoch: Instant,
}

impl GuiApp {
    fn new(config: AppConfig) -> Self {
        let renderer = RendererState::new(config.font.size, &config.font.family);
        Self {
            config,
            window: None,
            context: None,
            surface: None,
            renderer,
            tabs: Vec::new(),
            active_tab: 0,
            modifiers: ModifiersState::default(),
            cursor_position: None,
            selecting: false,
            last_error: None,
            cursor_blink_epoch: Instant::now(),
        }
    }

    fn ensure_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        if self.window.is_some() {
            return Ok(());
        }

        let attrs = Window::default_attributes()
            .with_title("easyterm")
            .with_inner_size(LogicalSize::new(
                self.config.window.width as f64,
                self.config.window.height as f64,
            ))
            .with_min_inner_size(LogicalSize::new(640.0, 420.0));
        let window = Rc::new(event_loop.create_window(attrs)?);
        let context = Context::new(window.clone())?;
        let mut surface = Surface::new(&context, window.clone())?;

        let size = window.inner_size();
        surface.resize(
            NonZeroU32::new(size.width.max(1)).unwrap(),
            NonZeroU32::new(size.height.max(1)).unwrap(),
        )?;

        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);

        let pty_size = self.current_terminal_size();
        self.open_tab(self.default_shell_spec(), pty_size)?;
        Ok(())
    }

    fn default_shell_spec(&self) -> LocalSessionSpec {
        LocalSessionSpec::new(self.config.shell.program.clone())
            .with_args(self.config.shell.args.clone())
    }

    fn open_tab(&mut self, spec: LocalSessionSpec, size: PtySize) -> Result<(), Box<dyn Error>> {
        let title = std::path::Path::new(&spec.program)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(spec.program.as_str())
            .to_string();
        let tab = GuiTab::new(
            title,
            spec,
            self.config.shell.term.clone(),
            size,
            self.config.scrollback_limit,
        )?;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len().saturating_sub(1);
        Ok(())
    }

    fn current_terminal_size(&self) -> PtySize {
        let Some(window) = self.window.as_ref() else {
            return PtySize::default();
        };
        self.renderer.terminal_size(window.inner_size())
    }

    fn active_tab_mut(&mut self) -> Option<&mut GuiTab> {
        self.tabs.get_mut(self.active_tab)
    }

    fn active_tab(&self) -> Option<&GuiTab> {
        self.tabs.get(self.active_tab)
    }

    fn on_resize(&mut self, size: PhysicalSize<u32>) {
        if let Some(surface) = self.surface.as_mut() {
            let _ = surface.resize(
                NonZeroU32::new(size.width.max(1)).unwrap(),
                NonZeroU32::new(size.height.max(1)).unwrap(),
            );
        }

        let term_size = self.renderer.terminal_size(size);
        for tab in &mut self.tabs {
            let _ = tab.resize(term_size);
        }
    }

    fn handle_keyboard_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: &winit::event::KeyEvent,
    ) {
        if event.state != ElementState::Pressed {
            return;
        }

        if self.modifiers.control_key() && self.modifiers.shift_key() {
            match &event.logical_key {
                Key::Character(value) if value.eq_ignore_ascii_case("t") => {
                    let _ = self.open_tab(self.default_shell_spec(), self.current_terminal_size());
                    self.reset_cursor_blink();
                    return;
                }
                Key::Character(value) if value.eq_ignore_ascii_case("w") => {
                    self.close_active_tab(event_loop);
                    self.reset_cursor_blink();
                    return;
                }
                _ => {}
            }
        }

        let modifiers = self.modifiers;

        if modifiers.control_key() && !modifiers.alt_key() {
            if let Some(bytes) = control_sequence_for_key(&event.logical_key) {
                if let Some(tab) = self.active_tab_mut() {
                    let _ = tab.send_input(&bytes);
                }
                self.reset_cursor_blink();
                return;
            }
        }

        if let Some(bytes) = named_key_bytes(&event.logical_key) {
            if let Some(tab) = self.active_tab_mut() {
                let _ = tab.send_input(bytes);
            }
            self.reset_cursor_blink();
            return;
        }

        if let Some(text) = event.text.as_ref() {
            if should_forward_text(text, modifiers) {
                if let Some(tab) = self.active_tab_mut() {
                    let _ = tab.send_input(text.as_bytes());
                }
                self.reset_cursor_blink();
            }
        }
    }

    fn close_active_tab(&mut self, event_loop: &ActiveEventLoop) {
        if self.tabs.is_empty() {
            event_loop.exit();
            return;
        }

        let mut tab = self.tabs.remove(self.active_tab);
        let _ = tab.shutdown();
        if self.tabs.is_empty() {
            event_loop.exit();
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
    }

    fn handle_ime_commit(&mut self, text: &str) {
        if self.modifiers.control_key() || self.modifiers.alt_key() || self.modifiers.super_key() {
            return;
        }
        if let Some(tab) = self.active_tab_mut() {
            let _ = tab.send_input(text.as_bytes());
        }
        self.reset_cursor_blink();
    }

    fn handle_mouse_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        state: ElementState,
        button: MouseButton,
    ) {
        let Some(position) = self.cursor_position else {
            return;
        };

        if button != MouseButton::Left {
            return;
        }

        if let Some(index) = self.tab_index_at(position.x as usize, position.y as usize) {
            if state == ElementState::Pressed {
                self.active_tab = index;
                self.reset_cursor_blink();
            }
            return;
        }

        if state == ElementState::Pressed {
            self.selecting = true;
            let cell = self.point_to_cell(position);
            if let (Some(tab), Some(cell)) = (self.active_tab_mut(), cell) {
                tab.begin_selection(cell);
            }
        } else {
            self.selecting = false;
            if let Some(tab) = self.active_tab_mut() {
                tab.finish_selection();
            }
        }

        if self.tabs.is_empty() {
            event_loop.exit();
        }
    }

    fn handle_cursor_move(&mut self, position: PhysicalPosition<f64>) {
        self.cursor_position = Some(position);
        if !self.selecting {
            return;
        }
        let cell = self.point_to_cell(position);
        if let (Some(tab), Some(cell)) = (self.active_tab_mut(), cell) {
            tab.update_selection(cell);
        }
    }

    fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => y.round() as i32,
            MouseScrollDelta::PixelDelta(position) => (position.y / 20.0).round() as i32,
        };

        if let Some(tab) = self.active_tab_mut() {
            tab.scroll(lines);
        }
    }

    fn point_to_cell(&self, position: PhysicalPosition<f64>) -> Option<CellPoint> {
        let tab = self.active_tab()?;
        let size = self.window.as_ref()?.inner_size();
        self.renderer.point_to_cell(position, size, tab)
    }

    fn tab_index_at(&self, x: usize, y: usize) -> Option<usize> {
        let window = self.window.as_ref()?;
        self.renderer
            .layout_tabs(window.inner_size(), &self.tabs)
            .into_iter()
            .find(|tab| y < self.renderer.tab_height && x >= tab.x && x < tab.x + tab.width)
            .map(|tab| tab.index)
    }

    fn draw(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        let cursor_visible = self.cursor_visible();
        let Some(surface) = self.surface.as_mut() else {
            return Ok(());
        };
        let size = window.inner_size();

        let mut buffer = surface.buffer_mut()?;
        let mut canvas = Canvas::new(&mut buffer, size.width as usize, size.height as usize);
        self.renderer.render(
            &mut canvas,
            &self.tabs,
            self.active_tab,
            self.last_error.as_deref(),
            cursor_visible,
        );
        buffer.present()?;
        Ok(())
    }

    fn poll_tabs(&mut self) -> bool {
        for tab in &mut self.tabs {
            tab.drain_output();
            let _ = tab.refresh_exit_state();
        }

        let mut removed_before_active = 0usize;
        let mut index = self.tabs.len();
        while index > 0 {
            index -= 1;
            if self.tabs[index].exit_status.is_some() {
                let mut tab = self.tabs.remove(index);
                let _ = tab.shutdown();
                if index < self.active_tab {
                    removed_before_active += 1;
                }
            }
        }

        if self.tabs.is_empty() {
            self.active_tab = 0;
            return true;
        }

        self.active_tab = self
            .active_tab
            .saturating_sub(removed_before_active)
            .min(self.tabs.len() - 1);
        false
    }

    fn cursor_visible(&self) -> bool {
        let cycle = CURSOR_BLINK_ON + CURSOR_BLINK_OFF;
        let elapsed = self.cursor_blink_epoch.elapsed();
        elapsed.as_millis() % cycle.as_millis() < CURSOR_BLINK_ON.as_millis()
    }

    fn reset_cursor_blink(&mut self) {
        self.cursor_blink_epoch = Instant::now();
    }
}

impl ApplicationHandler for GuiApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(err) = self.ensure_window(event_loop) {
            self.last_error = Some(err.to_string());
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.on_resize(size),
            WindowEvent::RedrawRequested => {
                if self.poll_tabs() {
                    event_loop.exit();
                    return;
                }
                if let Err(err) = self.draw() {
                    self.last_error = Some(err.to_string());
                    event_loop.exit();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Ime(Ime::Commit(text)) => self.handle_ime_commit(&text),
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_keyboard_input(event_loop, &event)
            }
            WindowEvent::CursorMoved { position, .. } => self.handle_cursor_move(position),
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_input(event_loop, state, button)
            }
            WindowEvent::MouseWheel { delta, .. } => self.handle_mouse_wheel(delta),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

struct GuiTab {
    title: String,
    runtime: PtyRuntime,
    terminal: Terminal,
    scroll_offset: usize,
    selection_anchor: Option<CellPoint>,
    selection_focus: Option<CellPoint>,
    scrollback_limit: usize,
    exit_status: Option<std::process::ExitStatus>,
}

impl GuiTab {
    fn new(
        title: String,
        spec: LocalSessionSpec,
        term: String,
        size: PtySize,
        scrollback_limit: usize,
    ) -> Result<Self, LocalPtyError> {
        let runtime = spawn_local_runtime(&spec, &term, size)?;
        Ok(Self {
            title,
            runtime,
            terminal: Terminal::new(size.cols as usize, size.rows as usize),
            scroll_offset: 0,
            selection_anchor: None,
            selection_focus: None,
            scrollback_limit,
            exit_status: None,
        })
    }

    fn send_input(&mut self, bytes: &[u8]) -> Result<(), LocalPtyError> {
        self.runtime.write_input(bytes)
    }

    fn resize(&mut self, size: PtySize) -> Result<(), LocalPtyError> {
        self.terminal.resize(size.cols as usize, size.rows as usize);
        self.runtime.resize(size)
    }

    fn drain_output(&mut self) {
        for chunk in self.runtime.drain_output() {
            self.terminal.feed(&chunk);
        }

        if self.terminal.scrollback().len() > self.scrollback_limit {
            self.scroll_offset = min(
                self.scroll_offset,
                self.max_scroll_offset(self.terminal.grid().height()),
            );
        }
    }

    fn refresh_exit_state(&mut self) -> Result<(), LocalPtyError> {
        if self.exit_status.is_none() {
            self.exit_status = self.runtime.try_wait()?;
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), LocalPtyError> {
        self.runtime.terminate()
    }

    fn total_lines(&self) -> usize {
        self.terminal.scrollback().len() + self.terminal.grid().height()
    }

    fn max_scroll_offset(&self, viewport_rows: usize) -> usize {
        self.total_lines().saturating_sub(viewport_rows)
    }

    fn scroll(&mut self, delta_lines: i32) {
        let max_offset = self.max_scroll_offset(self.terminal.grid().height());
        let updated = if delta_lines > 0 {
            self.scroll_offset.saturating_add(delta_lines as usize)
        } else {
            self.scroll_offset
                .saturating_sub(delta_lines.unsigned_abs() as usize)
        };
        self.scroll_offset = min(updated, max_offset);
    }

    fn viewport_start(&self, viewport_rows: usize) -> usize {
        self.total_lines()
            .saturating_sub(viewport_rows.saturating_add(self.scroll_offset))
    }

    fn begin_selection(&mut self, point: CellPoint) {
        self.selection_anchor = Some(point);
        self.selection_focus = Some(point);
    }

    fn update_selection(&mut self, point: CellPoint) {
        self.selection_focus = Some(point);
    }

    fn finish_selection(&mut self) {}

    fn selection_contains(&self, point: CellPoint) -> bool {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CellPoint {
    global_row: usize,
    col: usize,
}

struct RendererState {
    font: FontBackend,
    cell_width: usize,
    cell_height: usize,
    tab_height: usize,
}

impl RendererState {
    fn new(font_size: u16, family: &str) -> Self {
        let font = load_outline_font(family, font_size).unwrap_or_else(|| {
            let glyph_scale = max(1, ((font_size as usize) + 7) / 8);
            FontBackend::Bitmap { glyph_scale }
        });

        let (cell_width, cell_height) = match &font {
            FontBackend::Outline(outline) => (outline.cell_width, outline.cell_height),
            FontBackend::Bitmap { glyph_scale } => (8 * glyph_scale, 10 * glyph_scale),
        };
        let tab_height = cell_height + 10;
        Self {
            font,
            cell_width,
            cell_height,
            tab_height,
        }
    }

    fn terminal_size(&self, size: PhysicalSize<u32>) -> PtySize {
        let cols = max(1, size.width as usize / self.cell_width) as u16;
        let rows = max(
            1,
            size.height.saturating_sub(self.tab_height as u32) as usize / self.cell_height,
        ) as u16;
        PtySize { cols, rows }
    }

    fn layout_tabs(&self, size: PhysicalSize<u32>, tabs: &[GuiTab]) -> Vec<TabLayout> {
        let mut x = 0usize;
        let width_limit = size.width as usize;
        let mut layouts = Vec::new();

        for (index, tab) in tabs.iter().enumerate() {
            let text_width = self.measure_text(&tab.title);
            let width = max(TAB_MIN_WIDTH, text_width + TAB_PADDING_X * 2);
            if x >= width_limit {
                break;
            }
            layouts.push(TabLayout { index, x, width });
            x += width + TAB_GAP;
        }

        layouts
    }

    fn point_to_cell(
        &self,
        position: PhysicalPosition<f64>,
        size: PhysicalSize<u32>,
        tab: &GuiTab,
    ) -> Option<CellPoint> {
        if position.y < self.tab_height as f64 {
            return None;
        }
        let col = (position.x.max(0.0) as usize) / self.cell_width;
        let row = ((position.y as usize).saturating_sub(self.tab_height)) / self.cell_height;
        if col >= tab.terminal.grid().width() || row >= self.terminal_size(size).rows as usize {
            return None;
        }

        Some(CellPoint {
            global_row: tab.viewport_start(tab.terminal.grid().height()) + row,
            col,
        })
    }

    fn render(
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
                canvas.height.saturating_sub(self.cell_height * 2),
                canvas.width,
                self.cell_height * 2,
                rgb(128, 30, 30),
            );
            self.draw_text(
                canvas,
                10,
                canvas.height.saturating_sub(self.cell_height * 2) + 6,
                message,
                rgb(250, 230, 230),
                None,
            );
        }
    }

    fn render_tab_bar(&mut self, canvas: &mut Canvas<'_>, tabs: &[GuiTab], active_tab: usize) {
        canvas.fill_rect(0, 0, canvas.width, self.tab_height, rgb(24, 26, 32));

        for layout in self.layout_tabs(
            PhysicalSize::new(canvas.width as u32, canvas.height as u32),
            tabs,
        ) {
            let active = layout.index == active_tab;
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
                &tabs[layout.index].title,
                rgb(224, 229, 236),
                None,
            );
        }
    }

    fn render_terminal(&mut self, canvas: &mut Canvas<'_>, tab: &GuiTab, cursor_visible: bool) {
        let cols = tab.terminal.grid().width();
        let rows = tab.terminal.grid().height();
        let viewport_start = tab.viewport_start(rows);
        let current_start = tab.terminal.scrollback().len();

        for row in 0..rows {
            let global_row = viewport_start + row;
            let y = self.tab_height + row * self.cell_height;
            for col in 0..cols {
                let x = col * self.cell_width;
                let mut bg = rgb(14, 16, 20);
                let mut fg = rgb(224, 229, 236);
                let mut ch = ' ';
                let mut underline = false;

                if global_row < current_start {
                    if let Some(value) = tab.terminal.scrollback()[global_row].chars().nth(col) {
                        ch = value;
                    }
                } else {
                    let grid_row = global_row - current_start;
                    if let Some(cell) = tab.terminal.grid().get(grid_row, col) {
                        let rendered = renderable_cell(cell);
                        ch = rendered.ch;
                        fg = resolve_fg(cell);
                        bg = resolve_bg(cell);
                        underline = cell.style.underline;
                    }
                }

                if tab.selection_contains(CellPoint { global_row, col }) {
                    bg = rgb(52, 78, 116);
                }

                canvas.fill_rect(x, y, self.cell_width, self.cell_height, bg);
                if ch != ' ' {
                    self.draw_char(canvas, x, y, ch, fg, Some(bg));
                }
                if underline {
                    canvas.fill_rect(
                        x,
                        y + self
                            .cell_height
                            .saturating_sub(max(1, self.underline_thickness())),
                        self.cell_width,
                        max(1, self.underline_thickness()),
                        fg,
                    );
                }
            }
        }

        if cursor_visible && tab.scroll_offset == 0 {
            self.render_cursor(
                canvas,
                tab.terminal.cursor(),
                current_start + tab.terminal.cursor().row,
            );
        }
    }

    fn render_cursor(&self, canvas: &mut Canvas<'_>, cursor: Cursor, _global_row: usize) {
        let x = cursor.col * self.cell_width;
        let y = self.tab_height + cursor.row * self.cell_height;
        canvas.stroke_rect(
            x,
            y,
            self.cell_width.saturating_sub(1),
            self.cell_height.saturating_sub(1),
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
            self.draw_char(canvas, pen_x, y, ch, fg, bg);
            pen_x += self.text_step(ch);
        }
    }

    fn draw_char(
        &mut self,
        canvas: &mut Canvas<'_>,
        x: usize,
        y: usize,
        ch: char,
        fg: u32,
        bg: Option<u32>,
    ) {
        match &self.font {
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
                        let target_x = x as isize + glyph.offset_x + px as isize;
                        let target_y = y as isize + glyph.offset_y + py as isize;
                        canvas.blend_pixel(target_x, target_y, fg, alpha);
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
                        let px = x + col as usize * glyph_scale;
                        let py = y + row * glyph_scale;
                        canvas.fill_rect(px, py, *glyph_scale, *glyph_scale, fg);
                    }
                }
            }
        }
    }

    fn text_step(&self, ch: char) -> usize {
        match &self.font {
            FontBackend::Outline(outline) => outline.advance(ch),
            FontBackend::Bitmap { .. } => self.cell_width / 2,
        }
    }

    fn measure_text(&self, text: &str) -> usize {
        text.chars().map(|ch| self.text_step(ch)).sum()
    }

    fn underline_thickness(&self) -> usize {
        match &self.font {
            FontBackend::Outline(outline) => max(1, outline.cell_height / 14),
            FontBackend::Bitmap { glyph_scale } => max(1, *glyph_scale),
        }
    }
}

struct TabLayout {
    index: usize,
    x: usize,
    width: usize,
}

struct Canvas<'a> {
    buffer: &'a mut [u32],
    width: usize,
    height: usize,
}

impl<'a> Canvas<'a> {
    fn new(buffer: &'a mut [u32], width: usize, height: usize) -> Self {
        Self {
            buffer,
            width,
            height,
        }
    }

    fn clear(&mut self, color: u32) {
        self.buffer.fill(color);
    }

    fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        let x_end = min(self.width, x.saturating_add(width));
        let y_end = min(self.height, y.saturating_add(height));
        for py in y..y_end {
            let row = py * self.width;
            for px in x..x_end {
                self.buffer[row + px] = color;
            }
        }
    }

    fn stroke_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        self.fill_rect(x, y, width, 1, color);
        self.fill_rect(x, y + height, width, 1, color);
        self.fill_rect(x, y, 1, height, color);
        self.fill_rect(x + width, y, 1, height + 1, color);
    }

    fn blend_pixel(&mut self, x: isize, y: isize, fg: u32, alpha: u8) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let idx = y as usize * self.width + x as usize;
        let bg = self.buffer[idx];
        self.buffer[idx] = blend_colors(bg, fg, alpha);
    }
}

struct RenderedCell {
    ch: char,
}

fn renderable_cell(cell: &Cell) -> RenderedCell {
    if cell.wide_continuation {
        return RenderedCell { ch: ' ' };
    }
    let ch = cell.text.chars().next().unwrap_or(' ');
    RenderedCell { ch }
}

fn resolve_fg(cell: &Cell) -> u32 {
    resolve_color(cell.style.fg, rgb(224, 229, 236))
}

fn resolve_bg(cell: &Cell) -> u32 {
    resolve_color(cell.style.bg, rgb(14, 16, 20))
}

fn resolve_color(color: Color, default: u32) -> u32 {
    match color {
        Color::Default => default,
        Color::Rgb(r, g, b) => rgb(r, g, b),
        Color::Indexed(value) => indexed_color(value),
    }
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

fn named_key_bytes(key: &Key) -> Option<&'static [u8]> {
    match key {
        Key::Named(NamedKey::Enter) => Some(b"\r"),
        Key::Named(NamedKey::Tab) => Some(b"\t"),
        Key::Named(NamedKey::Backspace) => Some(b"\x7f"),
        Key::Named(NamedKey::Escape) => Some(b"\x1b"),
        Key::Named(NamedKey::ArrowUp) => Some(b"\x1b[A"),
        Key::Named(NamedKey::ArrowDown) => Some(b"\x1b[B"),
        Key::Named(NamedKey::ArrowRight) => Some(b"\x1b[C"),
        Key::Named(NamedKey::ArrowLeft) => Some(b"\x1b[D"),
        Key::Named(NamedKey::Home) => Some(b"\x1b[H"),
        Key::Named(NamedKey::End) => Some(b"\x1b[F"),
        Key::Named(NamedKey::Delete) => Some(b"\x1b[3~"),
        Key::Named(NamedKey::Insert) => Some(b"\x1b[2~"),
        Key::Named(NamedKey::PageUp) => Some(b"\x1b[5~"),
        Key::Named(NamedKey::PageDown) => Some(b"\x1b[6~"),
        _ => None,
    }
}

fn control_sequence_for_key(key: &Key) -> Option<Vec<u8>> {
    match key {
        Key::Character(value) if value.chars().count() == 1 => {
            let ch = value.chars().next()?.to_ascii_lowercase();
            if ch.is_ascii_lowercase() {
                Some(vec![(ch as u8) - b'a' + 1])
            } else {
                None
            }
        }
        _ => None,
    }
}

fn should_forward_text(text: &str, modifiers: ModifiersState) -> bool {
    if text.is_empty() {
        return false;
    }
    if modifiers.control_key() || modifiers.super_key() {
        return false;
    }
    !text
        .chars()
        .any(|ch| ch.is_control() && ch != '\r' && ch != '\n' && ch != '\t')
}
