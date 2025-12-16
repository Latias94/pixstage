use crate::dirty::DirtyTiles;
use crate::scaling::{compute_scaling, ScalingState};
use crate::{Error, PixstageOptions, Rect, Result, ScalingMode, SurfaceTexture};
use wgpu::util::DeviceExt;

fn globals_bytes(ndc_scale: [f32; 2]) -> [f32; 4] {
    [ndc_scale[0], ndc_scale[1], 0.0, 0.0]
}

fn argb1555_to_rgba8(pixel: u16) -> [u8; 4] {
    let a1 = ((pixel >> 15) & 0x1) as u8;
    let r5 = ((pixel >> 10) & 0x1f) as u32;
    let g5 = ((pixel >> 5) & 0x1f) as u32;
    let b5 = (pixel & 0x1f) as u32;

    let r = ((r5 * 255 + 15) / 31) as u8;
    let g = ((g5 * 255 + 15) / 31) as u8;
    let b = ((b5 * 255 + 15) / 31) as u8;
    let a = if a1 == 1 { 255 } else { 0 };

    [r, g, b, a]
}

/// ARGB1555 pixel buffer (CPU) + incremental upload into an internal RGBA8 texture.
#[derive(Debug)]
pub struct PixstageArgb1555<'win> {
    surface: wgpu::Surface<'win>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,

    scaling_mode: ScalingMode,
    scaling_state: ScalingState,
    clear_color: wgpu::Color,

    vertex_buffer: wgpu::Buffer,
    globals_buffer: wgpu::Buffer,

    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    sampler_nearest: wgpu::Sampler,
    sampler_linear: wgpu::Sampler,
    bind_group_nearest: wgpu::BindGroup,
    bind_group_linear: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,

    width: u32,
    height: u32,
    pixels: Vec<u16>,
    dirty: DirtyTiles,
    upload_buffer: Vec<u8>,
}

impl<'win> PixstageArgb1555<'win> {
    pub async fn new_async<W: wgpu::WindowHandle + 'win>(
        width: u32,
        height: u32,
        surface_texture: SurfaceTexture<W>,
    ) -> Result<Self> {
        Self::new_async_with_options(width, height, surface_texture, PixstageOptions::default())
            .await
    }

    pub async fn new_async_with_options<W: wgpu::WindowHandle + 'win>(
        width: u32,
        height: u32,
        surface_texture: SurfaceTexture<W>,
        options: PixstageOptions,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidBufferSize { width, height });
        }

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: options.backends,
            ..Default::default()
        });

        let surface = instance.create_surface(surface_texture.window)?;
        let compatible_surface = Some(&surface);
        let adapter =
            wgpu::util::initialize_adapter_from_env_or_default(&instance, compatible_surface)
                .await
                .map_err(|_| Error::AdapterNotFound)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await?;

        let surface_capabilities = surface.get_capabilities(&adapter);
        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(surface_capabilities.formats[0]);

        let present_mode = if surface_capabilities
            .present_modes
            .contains(&options.present_mode)
        {
            options.present_mode
        } else {
            wgpu::PresentMode::AutoVsync
        };
        let alpha_mode = surface_capabilities.alpha_modes[0];

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: surface_texture.width,
            height: surface_texture.height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let scaling_mode = options.scaling_mode;
        let scaling_state = compute_scaling(
            (width, height),
            (surface_config.width, surface_config.height),
            scaling_mode,
        );

        let globals = globals_bytes(scaling_state.ndc_scale);
        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pixstage_argb1555_globals_buffer"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let texture_format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let (texture, texture_view) = create_rgba_texture(&device, width, height, texture_format);

        let sampler_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pixstage_argb1555_sampler_nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let sampler_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pixstage_argb1555_sampler_linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pixstage_argb1555_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<[f32; 4]>() as u64
                        ),
                    },
                    count: None,
                },
            ],
        });

        let bind_group_nearest = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixstage_argb1555_bind_group_nearest"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler_nearest),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: globals_buffer.as_entire_binding(),
                },
            ],
        });
        let bind_group_linear = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixstage_argb1555_bind_group_linear"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler_linear),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: globals_buffer.as_entire_binding(),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pixstage_argb1555_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/rgba.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pixstage_argb1555_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_buffer = create_fullscreen_triangle(&device);
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pixstage_argb1555_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[fullscreen_triangle_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let mut dirty = DirtyTiles::new(width, height, 32);
        dirty.mark_full();

        Ok(Self {
            surface,
            adapter,
            device,
            queue,
            surface_config,
            scaling_mode,
            scaling_state,
            clear_color: options.clear_color,
            vertex_buffer,
            globals_buffer,
            texture,
            texture_view,
            sampler_nearest,
            sampler_linear,
            bind_group_nearest,
            bind_group_linear,
            pipeline,
            width,
            height,
            pixels: vec![0u16; width as usize * height as usize],
            dirty,
            upload_buffer: Vec::new(),
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn new<W: wgpu::WindowHandle + 'win>(
        width: u32,
        height: u32,
        surface_texture: SurfaceTexture<W>,
    ) -> Result<Self> {
        pollster::block_on(Self::new_async(width, height, surface_texture))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_options<W: wgpu::WindowHandle + 'win>(
        width: u32,
        height: u32,
        surface_texture: SurfaceTexture<W>,
        options: PixstageOptions,
    ) -> Result<Self> {
        pollster::block_on(Self::new_async_with_options(
            width,
            height,
            surface_texture,
            options,
        ))
    }

    pub fn buffer_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    pub fn set_scaling_mode(&mut self, scaling_mode: ScalingMode) {
        if self.scaling_mode == scaling_mode {
            return;
        }
        self.scaling_mode = scaling_mode;
        self.recompute_scaling();
    }

    pub fn clear_color(&mut self, clear_color: wgpu::Color) {
        self.clear_color = clear_color;
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.recompute_scaling();
    }

    pub fn resize_buffer(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidBufferSize { width, height });
        }

        self.width = width;
        self.height = height;
        self.pixels.resize(width as usize * height as usize, 0);
        self.dirty.resize(width, height);

        let (texture, texture_view) = create_rgba_texture(
            &self.device,
            width,
            height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        self.texture = texture;
        self.texture_view = texture_view;

        let bind_group_layout = self.pipeline.get_bind_group_layout(0);
        self.bind_group_nearest = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixstage_argb1555_bind_group_nearest"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler_nearest),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.globals_buffer.as_entire_binding(),
                },
            ],
        });
        self.bind_group_linear = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixstage_argb1555_bind_group_linear"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler_linear),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.globals_buffer.as_entire_binding(),
                },
            ],
        });

        self.recompute_scaling();
        self.dirty.mark_full();
        Ok(())
    }

    pub fn frame(&self) -> &[u16] {
        &self.pixels
    }

    pub fn frame_mut(&mut self) -> &mut [u16] {
        self.dirty.mark_full();
        &mut self.pixels
    }

    pub fn clear(&mut self, value: u16) {
        self.pixels.fill(value);
        self.dirty.mark_full();
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, value: u16) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.width + x) as usize;
        self.pixels[offset] = value;
        self.dirty.mark_point(x, y);
    }

    pub fn mark_dirty(&mut self, rect: Rect) {
        self.dirty.mark_rect(rect);
    }

    pub fn window_pos_to_pixel(
        &self,
        physical_position: (f32, f32),
    ) -> std::result::Result<(usize, usize), (isize, isize)> {
        let clip = self.scaling_state.clip_rect;

        let x = physical_position.0.floor() as i32;
        let y = physical_position.1.floor() as i32;
        let clip_x = clip.x as i32;
        let clip_y = clip.y as i32;

        if x < clip_x
            || y < clip_y
            || x >= clip_x + clip.width as i32
            || y >= clip_y + clip.height as i32
        {
            return Err((x as isize, y as isize));
        }

        let local_x = (x - clip_x) as f32;
        let local_y = (y - clip_y) as f32;

        let scale = self
            .scaling_state
            .buffer_to_surface_scale
            .max(f32::MIN_POSITIVE);
        let visible_buffer_width = clip.width as f32 / scale;
        let visible_buffer_height = clip.height as f32 / scale;
        let crop_x = (self.width as f32 - visible_buffer_width) / 2.0;
        let crop_y = (self.height as f32 - visible_buffer_height) / 2.0;

        let pixel_x = (crop_x + local_x / scale).floor() as isize;
        let pixel_y = (crop_y + local_y / scale).floor() as isize;

        if pixel_x < 0
            || pixel_y < 0
            || pixel_x >= self.width as isize
            || pixel_y >= self.height as isize
        {
            Err((pixel_x, pixel_y))
        } else {
            Ok((pixel_x as usize, pixel_y as usize))
        }
    }

    pub fn render(&mut self) -> Result<()> {
        if self.surface_config.width == 0 || self.surface_config.height == 0 {
            return Ok(());
        }

        let frame = self.surface.get_current_texture().or_else(|_| {
            self.surface.configure(&self.device, &self.surface_config);
            self.surface.get_current_texture()
        })?;

        self.upload_dirty_regions();

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pixstage_argb1555_command_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pixstage_argb1555_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            let clip = self.scaling_state.clip_rect;
            pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, self.active_bind_group(), &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn recompute_scaling(&mut self) {
        self.scaling_state = compute_scaling(
            (self.width, self.height),
            (self.surface_config.width, self.surface_config.height),
            self.scaling_mode,
        );
        let globals = globals_bytes(self.scaling_state.ndc_scale);
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
    }

    fn active_bind_group(&self) -> &wgpu::BindGroup {
        match self.scaling_mode {
            ScalingMode::PixelPerfect => &self.bind_group_nearest,
            ScalingMode::Fill => &self.bind_group_linear,
        }
    }

    fn upload_dirty_regions(&mut self) {
        let regions = self.dirty.take_regions(64);
        if regions.is_empty() {
            return;
        }

        for region in regions {
            let is_full = region.x == 0
                && region.y == 0
                && region.width == self.width
                && region.height == self.height;

            let bytes_per_row = region.width as usize * 4;
            let upload_len = bytes_per_row * region.height as usize;
            self.upload_buffer.resize(upload_len, 0);

            if is_full {
                for (i, pixel) in self.pixels.iter().copied().enumerate() {
                    let rgba = argb1555_to_rgba8(pixel);
                    let base = i * 4;
                    self.upload_buffer[base..base + 4].copy_from_slice(&rgba);
                }
            } else {
                for row in 0..region.height as usize {
                    let src_y = region.y as usize + row;
                    let src_x = region.x as usize;
                    let src_start = src_y * self.width as usize + src_x;

                    let dst_row = row * bytes_per_row;
                    for col in 0..region.width as usize {
                        let pixel = self.pixels[src_start + col];
                        let rgba = argb1555_to_rgba8(pixel);
                        let dst = dst_row + col * 4;
                        self.upload_buffer[dst..dst + 4].copy_from_slice(&rgba);
                    }
                }
            }

            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: region.x,
                        y: region.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &self.upload_buffer,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(region.width * 4),
                    rows_per_image: Some(region.height),
                },
                wgpu::Extent3d {
                    width: region.width,
                    height: region.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

fn create_rgba_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pixstage_argb1555_rgba_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_fullscreen_triangle(device: &wgpu::Device) -> wgpu::Buffer {
    let vertex_data: [[f32; 2]; 3] = [[-1.0, -1.0], [3.0, -1.0], [-1.0, 3.0]];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pixstage_fullscreen_triangle_vertex_buffer"),
        contents: bytemuck::cast_slice(&vertex_data),
        usage: wgpu::BufferUsages::VERTEX,
    })
}

fn fullscreen_triangle_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x2,
        }],
    }
}
