use pixstage::State;
use std::sync::Arc;
use winit::{
    event::*,
    event_loop::{EventLoop, EventLoopWindowTarget},
    keyboard::{Key, NamedKey},
    window::{Window, WindowBuilder},
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

// =======drawing=======

fn main() {
    pollster::block_on(run(200, 200, 4));
}

fn my_drawing(state: &mut State) {
    state.clear_canvas();
    let red = [255u8, 0, 0, 255];
    for x in 50..=150 {
        for y in 50..=150 {
            state.draw_pixel(x, y, red);
        }
    }

    draw_line(state, 30, 30, 170, 170, [255, 255, 0, 255]);

    state.submit();
}

fn draw_line(state: &mut State, x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 4]) {
    let mut steep = false;
    let mut x0 = x0 as i32;
    let mut x1 = x1 as i32;
    let mut y0 = y0 as i32;
    let mut y1 = y1 as i32;
    if (x0 - x1).abs() < (y0 - y1).abs() {
        steep = true;
        std::mem::swap(&mut x0, &mut y0);
        std::mem::swap(&mut x1, &mut y1);
    }
    if x0 > x1 {
        std::mem::swap(&mut x0, &mut x1);
        std::mem::swap(&mut y0, &mut y1);
    }
    let dx = x1 - x0;
    let dy = y1 - y0;
    let derror2 = dy.abs() * 2;
    let mut error2 = 0;
    let mut y = y0;
    for x in x0..=x1 {
        if steep {
            state.draw_pixel(y as usize, x as usize, color);
        } else {
            state.draw_pixel(x as usize, y as usize, color);
        }
        error2 += derror2;
        if error2 > dx {
            y += if y1 > y0 { 1 } else { -1 };
            error2 -= dx * 2;
        }
    }
}

// =======window handling=======

fn start_event_loop(state: State, window: Arc<Window>, event_loop: EventLoop<()>) {
    let mut state = state;
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            use winit::platform::web::EventLoopExtWebSys;
            let event_loop_function = EventLoop::spawn;
        } else {
            let event_loop_function = EventLoop::run;
        }
    }
    let _ = (event_loop_function)(
        event_loop,
        move |event: Event<()>, elwt: &EventLoopWindowTarget<()>| {
            if event == Event::NewEvents(StartCause::Init) {
                state.start();
            }

            if let Event::WindowEvent { event, .. } = event {
                match event {
                    WindowEvent::KeyboardInput {
                        event:
                            KeyEvent {
                                logical_key: Key::Named(NamedKey::Escape),
                                ..
                            },
                        ..
                    }
                    | WindowEvent::CloseRequested => elwt.exit(),
                    WindowEvent::Resized(physical_size) => {
                        if physical_size.width == 0 || physical_size.height == 0 {
                            println!("Window minimized!");
                        } else {
                            state.resize([physical_size.width, physical_size.height]);
                            window.request_redraw();
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        my_drawing(&mut state);
                        state.update();
                        match state.render() {
                            Ok(_) => {}
                            Err(wgpu::SurfaceError::Lost) => state.resize(state.size()),
                            Err(e) => eprintln!("{e:?}"),
                        }
                        window.request_redraw();
                    }
                    _ => {}
                }
            }
        },
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub async fn run(width: u32, height: u32, scale: u32) {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(log::Level::Warn).expect("Could't initialize logger");
        } else {
            env_logger::init();
        }
    }

    let event_loop = EventLoop::new().unwrap();
    let builder = WindowBuilder::new();
    let window = Arc::new(
        builder
            .with_inner_size(winit::dpi::PhysicalSize::new(width * scale, height * scale))
            .with_title("Pixstage")
            .with_resizable(false)
            .with_maximized(false)
            .build(&event_loop)
            .unwrap(),
    );

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowExtWebSys;
        web_sys::window()
            .and_then(|win| win.document())
            .map(|doc| {
                let canvas = window.canvas().unwrap();
                let mut web_width = 800.0f32;
                let ratio = 1.0;
                match doc.get_element_by_id("wasm-example") {
                    Some(dst) => {
                        web_width = dst.client_width() as f32;
                        let _ = dst.append_child(&web_sys::Element::from(canvas));
                    }
                    None => {
                        canvas.style().set_css_text(
                            "background-color: black; display: block; margin: 20px auto;",
                        );
                        doc.body()
                            .map(|body| body.append_child(&web_sys::Element::from(canvas)));
                    }
                };
                let canvas = window.canvas().unwrap();
                let web_height = web_width / ratio;
                let scale_factor = window.scale_factor() as f32;
                canvas.set_width((web_width * scale_factor) as u32);
                canvas.set_height((web_height * scale_factor) as u32);
                canvas.style().set_css_text(
                    &(canvas.style().css_text()
                        + &format!("width: {}px; height: {}px", web_width, web_height)),
                );
            })
            .expect("Couldn't append canvas to document body.");

        let state = State::new(window.clone()).await;

        wasm_bindgen_futures::spawn_local(async move {
            let run_closure =
                Closure::once_into_js(move || start_event_loop(state, window.clone(), event_loop));

            if let Err(error) = call_catch(&run_closure) {
                let is_control_flow_exception =
                    error.dyn_ref::<js_sys::Error>().map_or(false, |e| {
                        e.message().includes("Using exceptions for control flow", 0)
                    });

                if !is_control_flow_exception {
                    web_sys::console::error_1(&error);
                }
            }

            #[wasm_bindgen]
            extern "C" {
                #[wasm_bindgen(catch, js_namespace = Function, js_name = "prototype.call.call")]
                fn call_catch(this: &JsValue) -> Result<(), JsValue>;
            }
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let state = State::new(window.clone(), scale).await;
        start_event_loop(state, window.clone(), event_loop);
    }
}
