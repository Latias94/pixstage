use crate::Rect;

/// Controls how the pixel buffer is scaled to the surface.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ScalingMode {
    /// Scale up using an integer factor (best for crisp pixels).
    /// If the surface is smaller than the buffer, the center area is cropped.
    PixelPerfect,
    /// Scale up/down to fit while preserving aspect ratio.
    Fill,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct ScalingState {
    pub(crate) ndc_scale: [f32; 2],
    pub(crate) clip_rect: Rect,
    pub(crate) buffer_to_surface_scale: f32,
}

pub(crate) fn compute_scaling(
    buffer_size: (u32, u32),
    surface_size: (u32, u32),
    mode: ScalingMode,
) -> ScalingState {
    let (buffer_width, buffer_height) = buffer_size;
    let (surface_width, surface_height) = surface_size;

    let buffer_width_f = buffer_width as f32;
    let buffer_height_f = buffer_height as f32;
    let surface_width_f = surface_width as f32;
    let surface_height_f = surface_height as f32;

    let (scaled_width, scaled_height, buffer_to_surface_scale) = match mode {
        ScalingMode::PixelPerfect => {
            let width_ratio = (surface_width_f / buffer_width_f).max(1.0);
            let height_ratio = (surface_height_f / buffer_height_f).max(1.0);
            let scale = width_ratio.min(height_ratio).floor().max(1.0);
            (buffer_width_f * scale, buffer_height_f * scale, scale)
        }
        ScalingMode::Fill => {
            let width_ratio = surface_width_f / buffer_width_f;
            let height_ratio = surface_height_f / buffer_height_f;
            let scale = width_ratio.min(height_ratio);
            (buffer_width_f * scale, buffer_height_f * scale, scale)
        }
    };

    let clip_width = scaled_width.min(surface_width_f).max(1.0);
    let clip_height = scaled_height.min(surface_height_f).max(1.0);

    let clip_x = ((surface_width_f - clip_width) / 2.0).max(0.0) as u32;
    let clip_y = ((surface_height_f - clip_height) / 2.0).max(0.0) as u32;
    let clip_rect = Rect {
        x: clip_x,
        y: clip_y,
        width: clip_width as u32,
        height: clip_height as u32,
    };

    ScalingState {
        ndc_scale: [
            scaled_width / surface_width_f.max(1.0),
            scaled_height / surface_height_f.max(1.0),
        ],
        clip_rect,
        buffer_to_surface_scale,
    }
}
