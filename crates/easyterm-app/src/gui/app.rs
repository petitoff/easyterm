use crate::config::AppConfig;
use crate::gui::canvas::Canvas;
use crate::gui::clipboard::ClipboardState;
use crate::gui::input::{
    control_sequence_for_key, named_key_bytes, normalize_paste, should_forward_text,
};
use crate::gui::render::RendererState;
use crate::gui::tab::{CellPoint, GuiTab};
use crate::pty::PtySize;
use crate::session::LocalSessionSpec;
use softbuffer::{Context, Surface};
use std::error::Error;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState};
use winit::window::{Window, WindowId};

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
    clipboard: ClipboardState,
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
            clipboard: ClipboardState::new(),
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
                Key::Character(value) if value.eq_ignore_ascii_case("c") => {
                    self.copy_selection_to_clipboard();
                    return;
                }
                Key::Character(value) if value.eq_ignore_ascii_case("v") => {
                    self.paste_from_clipboard();
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
                    tab.clear_selection();
                }
                self.reset_cursor_blink();
                return;
            }
        }

        if let Some(bytes) = named_key_bytes(&event.logical_key) {
            if let Some(tab) = self.active_tab_mut() {
                let _ = tab.send_input(bytes);
                tab.clear_selection();
            }
            self.reset_cursor_blink();
            return;
        }

        if let Some(text) = event.text.as_ref() {
            if should_forward_text(text, modifiers) {
                if let Some(tab) = self.active_tab_mut() {
                    let _ = tab.send_input(text.as_bytes());
                    tab.clear_selection();
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
            tab.clear_selection();
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

        if button == MouseButton::Middle && state == ElementState::Pressed {
            self.paste_from_clipboard();
            self.reset_cursor_blink();
            return;
        }

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
            .find(|tab| y < self.renderer.tab_height() && x >= tab.x && x < tab.x + tab.width)
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
            if self.tabs[index].is_exited() {
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

    fn copy_selection_to_clipboard(&mut self) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let Some(selection) = tab.selected_text() else {
            return;
        };
        if selection.is_empty() {
            return;
        }

        if let Err(err) = self.clipboard.set_text(&selection) {
            self.last_error = Some(format!("clipboard copy failed: {err}"));
        }
    }

    fn paste_from_clipboard(&mut self) {
        match self.clipboard.get_text() {
            Ok(Some(text)) if !text.is_empty() => {
                let payload = normalize_paste(&text);
                if let Some(tab) = self.active_tab_mut() {
                    let _ = tab.send_input(&payload);
                    tab.clear_selection();
                }
            }
            Ok(_) => {}
            Err(err) => self.last_error = Some(format!("clipboard paste failed: {err}")),
        }
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
