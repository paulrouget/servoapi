#![feature(box_syntax)]

extern crate servo;
extern crate gleam;

use gleam::gl;

use self::servo::Servo;
use self::servo::compositing::windowing::WindowMethods;
use self::servo::euclid::{Point2D, Size2D, TypedPoint2D, TypedRect, ScaleFactor, TypedSize2D};
use self::servo::ipc_channel::ipc::IpcSender;
use self::servo::net_traits::net_error_list::NetError;
use self::servo::servo_config::resource_files::set_resources_path;
use self::servo::servo_geometry::DeviceIndependentPixel;
use self::servo::script_traits::{DevicePixel, LoadData};

use std::cell::{Cell, RefCell};
use std::env;
use std::rc::Rc;


use self::servo::msg::constellation_msg;

pub use self::servo::BrowserId;
pub use self::servo::msg::constellation_msg::{Key, KeyModifiers, KeyState};
pub use self::servo::msg::constellation_msg::{ALT, CONTROL, NONE, SHIFT, SUPER};
pub use self::servo::config::servo_version;
pub use self::servo::compositing::windowing::{MouseWindowEvent, WindowNavigateMsg, WindowEvent};
pub use self::servo::servo_url::ServoUrl;
pub use self::servo::style_traits::cursor::Cursor;
pub use self::servo::script_traits::{MouseButton, TouchEventType};
pub use self::servo::webrender_traits::ScrollLocation;
pub use self::servo::compositing::compositor_thread::EventLoopWaker;

#[derive(Debug)]
pub enum BrowserEvent {
    SetWindowInnerSize(BrowserId, u32, u32),
    SetWindowPosition(BrowserId, i32, i32),
    SetFullScreenState(BrowserId, bool),
    TitleChanged(BrowserId, Option<String>),
    StatusChanged(BrowserId, Option<String>),
    LoadStart(BrowserId),
    LoadEnd(BrowserId),
    LoadError(BrowserId, String),
    HeadParsed(BrowserId),
    HistoryChanged(BrowserId, Vec<LoadData>, usize),
    CursorChanged(Cursor),
    FaviconChanged(BrowserId, ServoUrl),
    Key(Option<BrowserId>, Option<char>, Key, constellation_msg::KeyModifiers),
    AllowNavigation(BrowserId, ServoUrl, IpcSender<bool>),
}

#[derive(Debug, Copy, Clone)]
pub struct DrawableGeometry {
    pub view_size: (u32, u32),
    pub margins: (u32, u32, u32, u32),
    pub position: (i32, i32),
    pub hidpi_factor: f32,
}

pub trait GLMethods {
    fn make_current(&self) -> Result<(),()>;
    fn swap_buffers(&self);
    fn get_gl(&self) -> Rc<gl::Gl>;
}

pub struct Constellation {
}

pub struct Compositor {
    servo: RefCell<Servo<WindowCallback>>,
    callbacks: Rc<WindowCallback>,
}

pub struct View {
}

impl View {
    pub fn show(&self, _: Option<BrowserId>) {
    }
}

struct WindowCallback {
    gl_methods: Rc<GLMethods>,
    waker: Box<EventLoopWaker + 'static + Send>,
    event_queue: RefCell<Vec<BrowserEvent>>,
    pub geometry: Cell<DrawableGeometry>,
}

impl Constellation {
    pub fn new() -> Result<Constellation, &'static str> {
        let path = env::current_dir().unwrap().join("servo_resources/");
        if !path.exists() {
            return Err("Can't find servo_resources/ directory");
        }
        let path = path.to_str().unwrap().to_string();
        set_resources_path(Some(path));
        Ok(Constellation {})
    }

    pub fn new_compositor(&self, gl_methods: Rc<GLMethods>, waker: Box<EventLoopWaker + Send>, geometry: DrawableGeometry) -> Compositor {
        let cb = Rc::new(WindowCallback {
            gl_methods: gl_methods.clone(),
            waker: waker,
            geometry: Cell::new(geometry),
            event_queue: RefCell::new(Vec::new()),
        });
        Compositor {
            servo: RefCell::new(Servo::new(cb.clone())),
            callbacks: cb.clone(),
        }
    }

    pub fn new_browser(&self, url: ServoUrl, compositor: &Compositor /*temporary*/) -> Result<BrowserId,()> {
        compositor.new_browser(url)
    }
}

impl Compositor {
    pub fn new_view(&self, geometry: DrawableGeometry) -> View {
        self.callbacks.geometry.set(geometry);
        View { }
    }
    pub fn new_browser(&self, url: ServoUrl) -> Result<BrowserId,()> {
        self.servo.borrow().create_browser(url)
    }
    pub fn perform_updates(&self) {
        self.servo.borrow_mut().handle_events(vec![]);
    }
    pub fn get_events(&self) -> Vec<BrowserEvent> {
        self.callbacks.get_events()
    }
    pub fn handle_event(&self, event: WindowEvent) {
        self.servo.borrow_mut().handle_events(vec![event]);
    }
}


impl WindowMethods for WindowCallback {
    fn prepare_for_composite(&self, _width: usize, _height: usize) -> bool {
        self.gl_methods.make_current().is_ok()
    }

    fn supports_clipboard(&self) -> bool {
        false
    }

    fn create_event_loop_waker(&self) -> Box<EventLoopWaker> {
        self.waker.clone()
    }

    fn gl(&self) -> Rc<gl::Gl> {
        self.gl_methods.get_gl()
    }

    fn hidpi_factor(&self) -> ScaleFactor<f32, DeviceIndependentPixel, DevicePixel> {
        let scale_factor = self.geometry.get().hidpi_factor;
        ScaleFactor::new(scale_factor)
    }

    fn framebuffer_size(&self) -> TypedSize2D<u32, DevicePixel> {
        let scale_factor = self.geometry.get().hidpi_factor as u32;
        let (width, height) = self.geometry.get().view_size;
        TypedSize2D::new(scale_factor * width, scale_factor * height)
    }

    fn window_rect(&self) -> TypedRect<u32, DevicePixel> {
        let scale_factor = self.geometry.get().hidpi_factor as u32;
        let mut size = self.framebuffer_size();

        let (top, right, bottom, left) = self.geometry.get().margins;
        let top = top * scale_factor;
        let right = right * scale_factor;
        let bottom = bottom * scale_factor;
        let left = left * scale_factor;

        size.height = size.height - top - bottom;
        size.width = size.width - left - right;

        TypedRect::new(TypedPoint2D::new(left, top), size)
    }

    fn size(&self) -> TypedSize2D<f32, DeviceIndependentPixel> {
        let (width, height) = self.geometry.get().view_size;
        TypedSize2D::new(width as f32, height as f32)
    }

    fn client_window(&self, _id: BrowserId) -> (Size2D<u32>, Point2D<i32>) {
        let (width, height) = self.geometry.get().view_size;
        let (x, y) = self.geometry.get().position;
        (Size2D::new(width, height), Point2D::new(x as i32, y as i32))
    }

    // Events

    fn set_inner_size(&self, id: BrowserId, size: Size2D<u32>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::SetWindowInnerSize(id, size.width as u32, size.height as u32));
    }

    fn set_position(&self, id: BrowserId, point: Point2D<i32>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::SetWindowPosition(id, point.x, point.y));
    }

    fn set_fullscreen_state(&self, id: BrowserId, state: bool) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::SetFullScreenState(id, state))
    }

    fn present(&self) {
        self.gl_methods.swap_buffers();
    }

    fn set_page_title(&self, id: BrowserId, title: Option<String>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::TitleChanged(id, title));
    }

    fn status(&self, id: BrowserId, status: Option<String>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::StatusChanged(id, status));
    }

    fn load_start(&self, id: BrowserId) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::LoadStart(id));
    }

    fn load_end(&self, id: BrowserId) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::LoadEnd(id));
    }

    fn load_error(&self, id: BrowserId, _: NetError, url: String) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::LoadError(id, url));
    }

    fn head_parsed(&self, id: BrowserId) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::HeadParsed(id));
    }

    fn history_changed(&self, id: BrowserId, entries: Vec<LoadData>, current: usize) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::HistoryChanged(id, entries, current));
    }

    fn set_cursor(&self, cursor: Cursor) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::CursorChanged(cursor));
    }

    fn set_favicon(&self, id: BrowserId, url: ServoUrl) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::FaviconChanged(id, url));
    }

    fn allow_navigation(&self, id: BrowserId, url: ServoUrl, chan: IpcSender<bool>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::AllowNavigation(id, url, chan));
    }

    fn handle_key(&self, id: Option<BrowserId>, ch: Option<char>, key: Key, mods: constellation_msg::KeyModifiers) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::Key(id, ch, key, mods));
    }
}

impl WindowCallback {
    pub fn get_events(&self) -> Vec<BrowserEvent> {
        let mut events = self.event_queue.borrow_mut();
        let copy = events.drain(..).collect();
        copy
    }
}
