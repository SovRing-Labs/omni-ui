// SPDX-License-Identifier: MIT OR Apache-2.0
//! The window: winit (Wayland / X11) + softbuffer (CPU framebuffer, the engine's tiles go straight in) + accesskit_winit
//! (AT-SPI) + the intent socket (access.bus and any other process send Intents as JSON lines). Hosts any `App`.
use crate::a11y;
use crate::app::App;
use crate::font::Fonts;
use crate::frame::{self, Frame};
use crate::widgets::{Kind, Tree};
use accesskit::{Action, ActionData};
use std::io::{BufRead, BufReader, Write};
use std::num::NonZeroU32;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

pub enum UserEvent {
    Ak(accesskit_winit::Event),
    Intent(String, mpsc::Sender<String>),
}
impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self { UserEvent::Ak(e) }
}

pub fn sock_path(app: &str) -> PathBuf {
    std::env::var("OMNI_UI_SOCK").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join(format!("omni-ui-{app}.sock"))
    })
}

struct Shell {
    app: Box<dyn App>,
    fonts: Fonts,
    tree: Tree,
    frame: Option<Frame>,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    adapter: Option<accesskit_winit::Adapter>,
    proxy: EventLoopProxy<UserEvent>,
    cursor: (f64, f64),
    mods: ModifiersState,
    size: (u32, u32),
    /// when the app's clock last ticked (pages with timers keep running after load)
    last_tick: std::time::Instant,
}

impl Shell {
    fn rebuild(&mut self) {
        self.tree = self.app.tree();
        let focus = self.app.focus_widget(&self.tree);
        let f = frame::render_app(self.app.as_mut(), &self.tree, focus, &self.fonts, self.size.0 as usize, self.size.1 as usize);
        self.app.set_extent(f.page_h, self.size.1 as i32);
        if std::env::var("OMNI_UI_TRACE").is_ok() {
            eprintln!("frame {}x{} scroll {} of {}: style {:.1} ms, layout {:.1} ms, raster {:.1} ms, tiles {}/{}", f.w, f.h, f.scroll, f.page_h,
                      f.ms_style, f.ms_layout, f.ms_raster, f.tiles_unique, f.tiles_total);
        }
        if let Some(a) = self.adapter.as_mut() {
            let page = self.app.page_a11y();
            let up = a11y::tree_update(&self.tree, &f, focus, page.as_ref().map(|(r, y, k)| (&r[..], *y, *k)));
            a.update_if_active(|| up);
        }
        if let Some(w) = &self.window { w.set_title(&self.app.title()); w.request_redraw(); }
        self.frame = Some(f);
    }

    fn present(&mut self) {
        let (Some(s), Some(f)) = (self.surface.as_mut(), self.frame.as_ref()) else { return };
        let (Some(w), Some(h)) = (NonZeroU32::new(f.w as u32), NonZeroU32::new(f.h as u32)) else { return };
        if s.resize(w, h).is_err() { return; }
        if let Ok(mut buf) = s.buffer_mut() {
            let n = buf.len().min(f.pixels.len());
            buf[..n].copy_from_slice(&f.pixels[..n]);
            let _ = buf.present();
        }
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    /// The app's clock: woken at its period (ControlFlow::WaitUntil, set in about_to_wait), never busy-polling.
    fn new_events(&mut self, _el: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            let ms = self.last_tick.elapsed().as_millis() as u64;
            self.last_tick = std::time::Instant::now();
            if self.app.tick(ms) { self.rebuild(); }
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        match self.app.tick_every() {
            Some(p) => { if self.last_tick.elapsed() > p * 4 { self.last_tick = std::time::Instant::now(); } el.set_control_flow(ControlFlow::WaitUntil(self.last_tick + p)); }
            None => { self.last_tick = std::time::Instant::now(); el.set_control_flow(ControlFlow::Wait); }
        }
    }
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() { return; }
        // invisible until the accessibility adapter exists (AccessKit's rule: the first tree must precede the first map)
        // OMNI_UI_TEST: a test window must not take keyboard focus from the person's desktop (their typing landed in it once)
        let test = std::env::var("OMNI_UI_TEST").is_ok();
        let attrs = Window::default_attributes().with_title(self.app.title()).with_inner_size(LogicalSize::new(900.0, 640.0)).with_visible(false).with_active(!test);
        let win = Rc::new(el.create_window(attrs).expect("window"));
        self.adapter = Some(accesskit_winit::Adapter::with_event_loop_proxy(el, &win, self.proxy.clone()));
        win.set_visible(true);
        let ctx = softbuffer::Context::new(win.clone()).expect("softbuffer context");
        self.surface = Some(softbuffer::Surface::new(&ctx, win.clone()).expect("softbuffer surface"));
        let sz = win.inner_size();
        self.size = (sz.width.max(1), sz.height.max(1));
        self.window = Some(win);
        self.rebuild();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(a), Some(w)) = (self.adapter.as_mut(), self.window.as_ref()) { a.process_event(w, &event); }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(sz) => { self.size = (sz.width.max(1), sz.height.max(1)); self.rebuild(); }
            WindowEvent::RedrawRequested => { self.present(); if self.app.live() { self.rebuild(); } }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                if self.app.pointer_move(position.x as i32, position.y as i32) { self.rebuild(); }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta { MouseScrollDelta::LineDelta(_, y) => -(y * 60.0) as i32, MouseScrollDelta::PixelDelta(p) => -p.y as i32 };
                self.app.wheel(dy);
                self.rebuild();
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. }
                if self.frame.as_ref().map(|f| (f.surface_top > 0 && self.cursor.1 as i32 >= f.surface_top)
                    || (f.overlay && f.widget_at(self.cursor.0 as i32, self.cursor.1 as i32).unwrap_or(0) == 0)).unwrap_or(false) => {
                let top = self.frame.as_ref().map(|f| f.surface_top).unwrap_or(0);
                self.app.surface_click(self.cursor.0 as i32, self.cursor.1 as i32 - top);
                self.rebuild();
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Middle, .. }
                if self.frame.as_ref().map(|f| (f.surface_top > 0 && self.cursor.1 as i32 >= f.surface_top)
                    || (f.overlay && f.widget_at(self.cursor.0 as i32, self.cursor.1 as i32).unwrap_or(0) == 0)).unwrap_or(false) => {
                let top = self.frame.as_ref().map(|f| f.surface_top).unwrap_or(0);
                self.app.surface_aux_click(self.cursor.0 as i32, self.cursor.1 as i32 - top);
                self.rebuild();
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Back, .. } => { self.app.act("back", "", "pointer"); self.rebuild(); }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Forward, .. } => { self.app.act("forward", "", "pointer"); self.rebuild(); }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let hit = self.frame.as_ref().and_then(|f| f.widget_at(self.cursor.0 as i32, self.cursor.1 as i32));
                if let Some(w) = hit {
                    let t = self.app.tree();
                    if t.kind[w as usize] == Kind::TextInput { self.app.set_focus(&t, w); } else { self.app.activate(&t, w, "pointer"); }
                    self.rebuild();
                }
            }
            WindowEvent::KeyboardInput { .. } if std::env::var("OMNI_UI_TEST").is_ok() => {} // tests drive keys through the socket only
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let base = match &event.logical_key {
                    Key::Named(NamedKey::Tab) => "Tab".to_string(), Key::Named(NamedKey::Enter) => "Enter".into(),
                    Key::Named(NamedKey::Backspace) => "Backspace".into(), Key::Named(NamedKey::Escape) => "Escape".into(),
                    Key::Named(NamedKey::PageDown) => "PageDown".into(), Key::Named(NamedKey::PageUp) => "PageUp".into(),
                    Key::Named(NamedKey::ArrowDown) => "ArrowDown".into(), Key::Named(NamedKey::ArrowUp) => "ArrowUp".into(),
                    Key::Named(NamedKey::ArrowLeft) => "ArrowLeft".into(), Key::Named(NamedKey::ArrowRight) => "ArrowRight".into(),
                    Key::Named(NamedKey::Home) => "Home".into(), Key::Named(NamedKey::End) => "End".into(),
                    Key::Named(NamedKey::F5) => "F5".into(), Key::Named(NamedKey::F6) => "F6".into(),
                    Key::Character(c) if self.mods.control_key() || self.mods.alt_key() => c.to_uppercase(),
                    _ => String::new(),
                };
                // Ctrl / Alt chords travel as "Ctrl+L", "Alt+ArrowLeft", "Ctrl+Shift+Tab" (and never type a control character)
                let chord = self.mods.control_key() || self.mods.alt_key();
                let mut name = base.clone();
                if chord && !base.is_empty() {
                    let shift = if self.mods.shift_key() && base == "Tab" { "Shift+" } else { "" };
                    name = format!("{}{}{shift}{base}", if self.mods.control_key() { "Ctrl+" } else { "" }, if self.mods.alt_key() { "Alt+" } else { "" });
                }
                let t = self.app.tree();
                self.app.key(&t, &name, if chord { None } else { event.text.as_deref() }, self.mods.shift_key());
                self.rebuild();
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Ak(e) => match e.window_event {
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    if let (Some(a), Some(f)) = (self.adapter.as_mut(), self.frame.as_ref()) {
                        let page = self.app.page_a11y();
                        let up = a11y::tree_update(&self.tree, f, self.app.focus_widget(&self.tree), page.as_ref().map(|(r, y, k)| (&r[..], *y, *k)));
                        a.update_if_active(|| up);
                    }
                }
                accesskit_winit::WindowEvent::ActionRequested(req) if a11y::page_row(req.target_node).is_some() => {
                    let row = a11y::page_row(req.target_node).unwrap_or(0);
                    let value = match &req.data { Some(ActionData::Value(v)) => Some(v.to_string()), _ => None };
                    let act = match req.action { Action::Click => "click", Action::Focus => "focus", Action::SetValue => "set_value", _ => "" };
                    if !act.is_empty() { self.app.page_action(row, act, value.as_deref()); self.rebuild(); }
                }
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    let w = a11y::widget(req.target_node);
                    let t = self.app.tree();
                    if (w as usize) < t.len() {
                        match req.action {
                            Action::Click => { self.app.activate(&t, w, "at-spi"); }
                            Action::Focus => self.app.set_focus(&t, w),
                            Action::SetValue => {
                                if let Some(ActionData::Value(v)) = req.data { self.app.set_focus(&t, w); self.app.act("type", &v, "at-spi"); }
                            }
                            _ => {}
                        }
                        self.rebuild();
                    }
                }
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
            },
            UserEvent::Intent(line, reply) => {
                let t = self.app.tree();
                let out = self.app.handle_line(&t, &line);
                self.rebuild();
                let _ = reply.send(out.to_string());
            }
        }
    }
}

fn serve(path: PathBuf, proxy: EventLoopProxy<UserEvent>) {
    let _ = std::fs::remove_file(&path);
    let l = match UnixListener::bind(&path) { Ok(l) => l, Err(e) => { eprintln!("omni-ui: intent socket {}: {e}", path.display()); return; } };
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)); // the owner only
    eprintln!("omni-ui: intents on {}", path.display());
    for conn in l.incoming().flatten() {
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let Ok(rd) = conn.try_clone() else { return };
            let mut w = conn;
            for line in BufReader::new(rd).lines().map_while(Result::ok) {
                let (tx, rx) = mpsc::channel();
                if proxy.send_event(UserEvent::Intent(line, tx)).is_err() { return; }
                let out = rx.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| "{\"ok\":false,\"error\":\"timeout\"}".into());
                if writeln!(w, "{out}").is_err() { return; }
            }
        });
    }
}

pub fn run(app: Box<dyn App>, name: &str) {
    let el = EventLoop::<UserEvent>::with_user_event().build().expect("event loop");
    let proxy = el.create_proxy();
    let p2 = proxy.clone();
    let sp = sock_path(name);
    let sp2 = sp.clone();
    std::thread::spawn(move || serve(sp2, p2));
    let mut shell = Shell { app, fonts: Fonts::new(), tree: Tree::default(), frame: None, window: None, surface: None,
                            adapter: None, proxy, cursor: (0.0, 0.0), mods: ModifiersState::empty(), size: (900, 640), last_tick: std::time::Instant::now() };
    el.run_app(&mut shell).expect("run");
    let _ = std::fs::remove_file(sp);
}
