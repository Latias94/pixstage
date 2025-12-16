# Pixstage

Pixstage is a tiny pixel buffer library built on top of `wgpu`.

It targets retro game emulators and software renderers, with two core goals:

- **Retro-friendly**: first-class `Indexed8 + Palette` rendering (palette lookup happens on the GPU).
- **Fast uploads**: dirty-rect tracking to avoid uploading the whole frame when only a small area changes.

Pixstage is windowing-framework-agnostic: any framework that supports `raw-window-handle` can be used.

## Features

- `PixstageRgba`: `RGBA8` pixel buffer with incremental texture updates
- `PixstageIndexed`: `Indexed8 + Palette` with GPU palette lookup (great for palette cycling)
- `PixstageRgb565`: `RGB565` input with incremental upload (converted to RGBA8 only for dirty regions)
- `PixstageArgb1555`: `ARGB1555` input with incremental upload (1-bit alpha)
- `ScalingMode::PixelPerfect` and `ScalingMode::Fill`
- `PixstageOptions`: shared configuration (backends/present_mode/scaling/clear_color)

## Examples

- `rectangle_and_line`: simple drawing using `PixstageRgba`
- `palette_cycle`: palette animation using `PixstageIndexed`
- `rgb565_checker`: RGB565 input example using `PixstageRgb565`
- `argb1555_alpha`: ARGB1555 (1-bit alpha) example using `PixstageArgb1555`

Run:

```sh
cargo run --example rectangle_and_line
cargo run --example palette_cycle
cargo run --example rgb565_checker
cargo run --example argb1555_alpha
```

## Minimal usage (winit)

```rust
use pixstage::{PixstageRgba, SurfaceTexture};
use winit::event_loop::EventLoop;
use winit::window::Window;

# let event_loop = EventLoop::new().unwrap();
# let window = Window::new(&event_loop).unwrap();
# let size = window.inner_size();
let surface = SurfaceTexture::new(size.width, size.height, &window)?;
let mut stage = PixstageRgba::new_async(320, 240, surface).await?;

// Write pixels
stage.set_pixel(10, 10, [255, 0, 0, 255]);
stage.render()?;
# Ok::<(), pixstage::Error>(())
```

## Notes

- The examples use `winit`, but the library itself does not depend on it.
