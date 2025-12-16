use pixstage::{PixstageRgb565, SurfaceTexture};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    let r5 = (r as u16 >> 3) & 0x1f;
    let g6 = (g as u16 >> 2) & 0x3f;
    let b5 = (b as u16 >> 3) & 0x1f;
    (r5 << 11) | (g6 << 5) | b5
}

fn draw_scene(stage: &mut PixstageRgb565) {
    let (w, h) = stage.buffer_size();
    let cell = 16;

    for y in 0..h {
        for x in 0..w {
            let cx = (x / cell) & 1;
            let cy = (y / cell) & 1;
            let value = if (cx ^ cy) == 0 {
                rgb565(255, 255, 255)
            } else {
                rgb565(40, 40, 40)
            };
            stage.set_pixel(x, y, value);
        }
    }

    for x in 0..w {
        let r = (x * 255 / (w.max(1) - 1).max(1)) as u8;
        stage.set_pixel(x, 0, rgb565(r, 0, 0));
        stage.set_pixel(x, 1, rgb565(0, r, 0));
        stage.set_pixel(x, 2, rgb565(0, 0, r));
    }
}

#[cfg(target_arch = "wasm32")]
fn setup_canvas(window: &Window, buffer_width: u32, buffer_height: u32) {
    use winit::platform::web::WindowExtWebSys;

    let _ = web_sys::window()
        .and_then(|win| win.document())
        .map(|doc| {
            let canvas = window.canvas().unwrap();
            let mut web_width = 800.0f32;
            let ratio = buffer_width as f32 / buffer_height as f32;
            match doc.get_element_by_id("wasm-example") {
                Some(dst) => {
                    web_width = dst.client_width() as f32;
                    let _ = dst.append_child(&web_sys::Element::from(canvas));
                }
                None => {
                    canvas.style().set_css_text(
                        "background-color: black; display: block; margin: 20px auto;",
                    );
                    let _ = doc
                        .body()
                        .map(|body| body.append_child(&web_sys::Element::from(canvas)));
                }
            };
            let canvas = window.canvas().unwrap();
            let web_height = web_width / ratio;
            let scale_factor = window.scale_factor() as f32;
            canvas.set_width((web_width * scale_factor) as u32);
            canvas.set_height((web_height * scale_factor) as u32);
            let _ = canvas.style().set_css_text(
                &(canvas.style().css_text()
                    + &format!("width: {}px; height: {}px", web_width, web_height)),
            );
        })
        .expect("Couldn't append canvas to document body.");
}

#[derive(Debug)]
struct AppState {
    buffer_width: u32,
    buffer_height: u32,
    scale: u32,
    window: Option<Arc<Window>>,
    stage: Option<PixstageRgb565<'static>>,
}

#[derive(Clone, Debug)]
struct App {
    state: Rc<RefCell<AppState>>,
}

impl App {
    fn new(buffer_width: u32, buffer_height: u32, scale: u32) -> Self {
        Self {
            state: Rc::new(RefCell::new(AppState {
                buffer_width,
                buffer_height,
                scale,
                window: None,
                stage: None,
            })),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.borrow().window.is_some() {
            return;
        }

        let (buffer_width, buffer_height, scale) = {
            let state = self.state.borrow();
            (state.buffer_width, state.buffer_height, state.scale)
        };

        let window_attributes = Window::default_attributes()
            .with_inner_size(winit::dpi::PhysicalSize::new(
                buffer_width * scale,
                buffer_height * scale,
            ))
            .with_title("Pixstage - RGB565")
            .with_resizable(true);

        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

        #[cfg(target_arch = "wasm32")]
        setup_canvas(&window, buffer_width, buffer_height);

        self.state.borrow_mut().window = Some(window.clone());

        #[cfg(not(target_arch = "wasm32"))]
        {
            let size = window.inner_size();
            let surface_texture =
                SurfaceTexture::new(size.width.max(1), size.height.max(1), window.clone()).unwrap();
            let stage = pollster::block_on(PixstageRgb565::new_async(
                buffer_width,
                buffer_height,
                surface_texture,
            ))
            .unwrap();
            self.state.borrow_mut().stage = Some(stage);
            window.request_redraw();
        }

        #[cfg(target_arch = "wasm32")]
        {
            let state = self.state.clone();
            let window = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let size = window.inner_size();
                let surface_texture =
                    SurfaceTexture::new(size.width.max(1), size.height.max(1), window.clone())
                        .unwrap();
                let stage = PixstageRgb565::new_async(buffer_width, buffer_height, surface_texture)
                    .await
                    .unwrap();
                state.borrow_mut().stage = Some(stage);
                window.request_redraw();
            });
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let mut state = self.state.borrow_mut();
        let Some(window) = state.window.clone() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(physical_size) => {
                if let Some(stage) = state.stage.as_mut() {
                    if physical_size.width > 0 && physical_size.height > 0 {
                        stage.resize_surface(physical_size.width, physical_size.height);
                    }
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(stage) = state.stage.as_mut() {
                    draw_scene(stage);
                    window.pre_present_notify();
                    if let Err(error) = stage.render() {
                        eprintln!("{error:?}");
                    }
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new(256, 240, 3);
    event_loop.run_app(&mut app).unwrap();
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn main_web() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_log::init_with_level(log::Level::Warn).expect("Couldn't initialize logger");

    use winit::platform::web::EventLoopExtWebSys;
    let event_loop = EventLoop::new().unwrap();
    let app = App::new(256, 240, 3);
    event_loop.spawn_app(app);
}
