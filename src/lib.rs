#![deny(clippy::all)]
#![forbid(unsafe_code)]

pub use raw_window_handle;
pub use wgpu;

mod dirty;
mod options;
mod rect;
mod scaling;
mod surface;

pub mod argb1555;
pub mod indexed;
pub mod rgb565;
pub mod rgba;

pub use argb1555::PixstageArgb1555;
pub use indexed::PixstageIndexed;
pub use options::PixstageOptions;
pub use rect::Rect;
pub use rgb565::PixstageRgb565;
pub use rgba::PixstageRgba;
pub use scaling::ScalingMode;
pub use surface::SurfaceTexture;

/// Pixstage unified error type.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("No suitable `wgpu::Adapter` found.")]
    AdapterNotFound,
    #[error("Invalid buffer size: {width}x{height}.")]
    InvalidBufferSize { width: u32, height: u32 },
    #[error("Invalid surface size: {width}x{height}.")]
    InvalidSurfaceSize { width: u32, height: u32 },
    #[error(transparent)]
    CreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error(transparent)]
    Device(#[from] wgpu::RequestDeviceError),
    #[error(transparent)]
    Surface(#[from] wgpu::SurfaceError),
}

pub type Result<T> = std::result::Result<T, Error>;
