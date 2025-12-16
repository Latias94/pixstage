use crate::ScalingMode;

/// Options shared by all Pixstage variants.
#[derive(Debug, Copy, Clone)]
pub struct PixstageOptions {
    pub backends: wgpu::Backends,
    pub present_mode: wgpu::PresentMode,
    pub scaling_mode: ScalingMode,
    pub clear_color: wgpu::Color,
}

impl Default for PixstageOptions {
    fn default() -> Self {
        Self {
            backends: wgpu::Backends::from_env().unwrap_or_else(wgpu::Backends::all),
            present_mode: wgpu::PresentMode::AutoVsync,
            scaling_mode: ScalingMode::PixelPerfect,
            clear_color: wgpu::Color::BLACK,
        }
    }
}

impl PixstageOptions {
    pub fn with_backends(mut self, backends: wgpu::Backends) -> Self {
        self.backends = backends;
        self
    }

    pub fn with_present_mode(mut self, present_mode: wgpu::PresentMode) -> Self {
        self.present_mode = present_mode;
        self
    }

    pub fn with_scaling_mode(mut self, scaling_mode: ScalingMode) -> Self {
        self.scaling_mode = scaling_mode;
        self
    }

    pub fn with_clear_color(mut self, clear_color: wgpu::Color) -> Self {
        self.clear_color = clear_color;
        self
    }
}
