use crate::Error;

/// A logical texture for a window surface.
#[derive(Debug)]
pub struct SurfaceTexture<W: wgpu::WindowHandle> {
    pub(crate) window: W,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl<W: wgpu::WindowHandle> SurfaceTexture<W> {
    /// Create a logical surface texture.
    ///
    /// It is recommended (but not required) that the `width` and `height` are the physical surface
    /// dimensions (e.g. scaled by the HiDPI factor).
    pub fn new(width: u32, height: u32, window: W) -> Result<Self, Error> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidSurfaceSize { width, height });
        }
        Ok(Self {
            window,
            width,
            height,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
