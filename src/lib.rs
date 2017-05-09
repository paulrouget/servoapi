#![feature(box_syntax)]

#[macro_use]
extern crate log;

extern crate servo;
extern crate gleam;

use gleam::gl;

use self::servo::compositing::compositor_thread::{self, CompositorProxy, CompositorReceiver};
use self::servo::compositing::windowing::WindowMethods;
use self::servo::msg::constellation_msg::{self, Key};
use self::servo::euclid::{Point2D, Size2D};
use self::servo::euclid::point::TypedPoint2D;
use self::servo::euclid::rect::TypedRect;
use self::servo::euclid::scale_factor::ScaleFactor;
use self::servo::euclid::size::TypedSize2D;
use self::servo::net_traits::net_error_list::NetError;
use self::servo::servo_config::resource_files::set_resources_path;
use self::servo::servo_config::opts;
use self::servo::servo_config::prefs::{PrefValue, PREFS};
use self::servo::servo_geometry::DeviceIndependentPixel;
use self::servo::script_traits::{DevicePixel, LoadData};

use std::cell::{Cell, RefCell};
use std::env;
use std::sync::mpsc;
use std::rc::Rc;

pub use self::servo::compositing::windowing::WindowEvent;
pub use self::servo::servo_url::ServoUrl;
pub use self::servo::style_traits::cursor::Cursor;
pub use self::servo::script_traits::TouchEventType;
pub use self::servo::webrender_traits::ScrollLocation;

#[derive(Debug)]
pub enum BrowserEvent {
    SetWindowInnerSize(u32, u32),
    SetWindowPosition(i32, i32),
    SetFullScreenState(bool),
    Present,
    TitleChanged(Option<String>),
    UnhandledURL(ServoUrl),
    StatusChanged(Option<String>),
    LoadStart,
    LoadEnd,
    LoadError(String),
    HeadParsed,
    HistoryChanged(Vec<LoadData>, usize),
    CursorChanged(Cursor),
    FaviconChanged(ServoUrl),
    Key(Option<char>, Key, constellation_msg::KeyModifiers),
}

type CompositorChannel = (Box<CompositorProxy + Send>, Box<CompositorReceiver>);

pub struct Constellation;

pub enum BrowserVisibility {
    Visible,
    Hidden,
}

pub trait EventLoopRiser {
    fn clone(&self) -> Box<EventLoopRiser + Send>;
    fn rise(&self);
}

#[derive(Debug, Copy, Clone)]
pub struct DrawableGeometry {
    pub view_size: (u32, u32),
    pub margins: (u32, u32, u32, u32),
    pub position: (i32, i32),
    pub hidpi_factor: f32,
}

pub struct Compositor {
    gl: Rc<gl::Gl>,
}

pub struct View {
    // FIXME: instead, use compositor
    gl: Rc<gl::Gl>,
    geometry: Cell<DrawableGeometry>,
    riser: Box<EventLoopRiser + Send>,
    event_queue: RefCell<Vec<BrowserEvent>>,
}

pub struct Browser {
    servo_browser: RefCell<servo::Browser<View>>,
}

impl Constellation {
    pub fn new() -> Result<Constellation, &'static str> {

        let path = env::current_dir().unwrap().join("servo_resources/");
        if !path.exists() {
            return Err("Can't find servo_resources/ directory");
        }
        let path = path.to_str().unwrap().to_string();
        set_resources_path(Some(path));

        let mut opts = opts::default_opts();
        opts.headless = false;
        opts.url = ServoUrl::parse("https://servo.org").ok();
        opts::set_defaults(opts);
        // FIXME: Pipeline creation fails is layout_threads pref not set
        PREFS.set("layout.threads", PrefValue::Number(1.0));

        Ok(Constellation)
    }
}

impl Compositor {
    pub fn new(_constellation: &Constellation, gl: Rc<gl::Gl>) -> Compositor {
        Compositor { gl: gl.clone() }
    }
}

impl View {
    pub fn new<R: EventLoopRiser + 'static + Send>(compositor: &Compositor,
                                                   geometry: DrawableGeometry,
                                                   riser: Box<R>)
                                                   -> Rc<View> {
        Rc::new(View {
                    gl: compositor.gl.clone(),
                    geometry: Cell::new(geometry),
                    riser: riser,
                    event_queue: RefCell::new(Vec::new()),
                })
    }

    // FIXME
    // pub fn new_headless(geometry: DrawableGeometry) {
    //     View {
    //         compositor: None,
    //         geometry: Cell::new(geometry),
    //     }
    // }

    pub fn get_events(&self) -> Vec<BrowserEvent> {
        // FIXME: ports/glutin/window.rs uses mem::replace. Should we too?
        // See: https://doc.rust-lang.org/core/mem/fn.replace.html
        let mut events = self.event_queue.borrow_mut();
        let copy = events.drain(..).collect();
        copy
    }
}

impl Browser {
    pub fn new(_constellation: &Constellation, _url: ServoUrl, view: Rc<View>) -> Browser {

        let mut servo = servo::Browser::new(view.clone());
        servo.handle_events(vec![WindowEvent::InitializeCompositing]);

        Browser { servo_browser: RefCell::new(servo) }
    }

    pub fn set_visibility(&self, _: BrowserVisibility) {
        // FIXME
    }

    pub fn handle_event(&self, event: WindowEvent) {
        self.servo_browser
            .borrow_mut()
            .handle_events(vec![event]);
    }

    pub fn perform_updates(&self) {
        self.servo_browser.borrow_mut().handle_events(vec![]);
    }
}


impl WindowMethods for View {
    fn prepare_for_composite(&self, _width: usize, _height: usize) -> bool {
        true
    }

    fn supports_clipboard(&self) -> bool {
        false
    }

    fn allow_navigation(&self, _url: ServoUrl) -> bool {
        true
    }

    fn create_compositor_channel(&self) -> CompositorChannel {
        let (sender, receiver) = mpsc::channel();
        (box SynchroCompositorProxy {
                 sender: sender,
                 riser: self.riser.clone(),
             } as Box<CompositorProxy + Send>,
         box receiver as Box<CompositorReceiver>)
    }

    fn gl(&self) -> Rc<gl::Gl> {
        self.gl.clone()
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

    fn client_window(&self) -> (Size2D<u32>, Point2D<i32>) {
        let (width, height) = self.geometry.get().view_size;
        let (x, y) = self.geometry.get().position;
        (Size2D::new(width, height), Point2D::new(x as i32, y as i32))
    }

    // Events

    fn set_inner_size(&self, size: Size2D<u32>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::SetWindowInnerSize(size.width as u32, size.height as u32));
    }

    fn set_position(&self, point: Point2D<i32>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::SetWindowPosition(point.x, point.y));
    }

    fn set_fullscreen_state(&self, state: bool) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::SetFullScreenState(state))
    }

    fn present(&self) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::Present);
    }

    fn set_page_title(&self, title: Option<String>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::TitleChanged(title));
    }

    fn status(&self, status: Option<String>) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::StatusChanged(status));
    }

    fn load_start(&self) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::LoadStart);
    }

    fn load_end(&self) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::LoadEnd);
    }

    fn load_error(&self, _: NetError, url: String) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::LoadError(url));
    }

    fn head_parsed(&self) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::HeadParsed);
    }

    fn history_changed(&self, entries: Vec<LoadData>, current: usize) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::HistoryChanged(entries, current));
    }

    fn set_cursor(&self, cursor: Cursor) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::CursorChanged(cursor));
    }

    fn set_favicon(&self, url: ServoUrl) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::FaviconChanged(url));
    }

    fn handle_key(&self, ch: Option<char>, key: Key, mods: constellation_msg::KeyModifiers) {
        self.event_queue
            .borrow_mut()
            .push(BrowserEvent::Key(ch, key, mods));
    }
}

struct SynchroCompositorProxy {
    sender: mpsc::Sender<compositor_thread::Msg>,
    riser: Box<EventLoopRiser + Send>,
}

impl CompositorProxy for SynchroCompositorProxy {
    fn send(&self, msg: compositor_thread::Msg) {
        if let Err(err) = self.sender.send(msg) {
            warn!("Failed to send response ({}).", err);
        }
        self.riser.rise();
    }

    fn clone_compositor_proxy(&self) -> Box<CompositorProxy + Send> {
        box SynchroCompositorProxy {
                sender: self.sender.clone(),
                riser: self.riser.clone(),
            } as Box<CompositorProxy + Send>
    }
}
