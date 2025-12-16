use crate::dirty::DirtyTiles;
use crate::scaling::{compute_scaling, ScalingState};
use crate::{Error, PixstageOptions, Rect, Result, ScalingMode, SurfaceTexture};
use wgpu::util::DeviceExt;

fn globals_bytes(ndc_scale: [f32; 2]) -> [f32; 4] {
    [ndc_scale[0], ndc_scale[1], 0.0, 0.0]
}

/// Indexed8 + 256-entry palette (GPU lookup) + incremental texture upload.
#[derive(Debug)]
pub struct PixstageIndexed<'win> {
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

    index_texture: wgpu::Texture,
    index_view: wgpu::TextureView,
    palette_texture: wgpu::Texture,
    palette_view: wgpu::TextureView,

    sampler_nearest: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,

    width: u32,
    height: u32,
    indices: Vec<u8>,
    palette: [[u8; 4]; 256],
    dirty: DirtyTiles,
    palette_dirty: bool,
    upload_buffer: Vec<u8>,
}

impl<'win> PixstageIndexed<'win> {
    /// Asynchronously create an Indexed8 stage.
    pub async fn new_async<W: wgpu::WindowHandle + 'win>(
        width: u32,
        height: u32,
        surface_texture: SurfaceTexture<W>,
    ) -> Result<Self> {
        Self::new_async_with_options(width, height, surface_texture, PixstageOptions::default())
            .await
    }

    /// Asynchronously create an Indexed8 stage with options.
    pub async fn new_async_with_options<W: wgpu::WindowHandle + 'win>(
        width: u32,
        height: u32,
        surface_texture: SurfaceTexture<W>,
        options: PixstageOptions,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidBufferSize { width, height });
        }

        let backends = options.backends;
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
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
            label: Some("pixstage_indexed_globals_buffer"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let (index_texture, index_view) = create_index_texture(&device, width, height);
        let (palette_texture, palette_view) = create_palette_texture(&device);

        let sampler_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pixstage_indexed_sampler_nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pixstage_indexed_bind_group_layout"),
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
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixstage_indexed_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&index_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler_nearest),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&palette_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: globals_buffer.as_entire_binding(),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pixstage_indexed_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/indexed.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pixstage_indexed_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_buffer = create_fullscreen_triangle(&device);
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pixstage_indexed_pipeline"),
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

        let mut palette = [[0u8; 4]; 256];
        for entry in palette.iter_mut() {
            entry[3] = 255;
        }

        let mut dirty = DirtyTiles::new(width, height, 32);
        dirty.mark_full();

        let mut stage = Self {
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
            index_texture,
            index_view,
            palette_texture,
            palette_view,
            sampler_nearest,
            bind_group,
            pipeline,
            width,
            height,
            indices: vec![0u8; width as usize * height as usize],
            palette,
            dirty,
            palette_dirty: true,
            upload_buffer: Vec::new(),
        };

        stage.upload_palette();
        Ok(stage)
    }

    /// Synchronously create an Indexed8 stage (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new<W: wgpu::WindowHandle + 'win>(
        width: u32,
        height: u32,
        surface_texture: SurfaceTexture<W>,
    ) -> Result<Self> {
        pollster::block_on(Self::new_async(width, height, surface_texture))
    }

    /// Synchronously create an Indexed8 stage with options (native only).
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

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn buffer_size(&self) -> (u32, u32) {
        (self.width, self.height)
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
        self.indices.resize(width as usize * height as usize, 0);
        self.dirty.resize(width, height);

        let (index_texture, index_view) = create_index_texture(&self.device, width, height);
        self.index_texture = index_texture;
        self.index_view = index_view;

        let bind_group_layout = self.pipeline.get_bind_group_layout(0);
        self.bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixstage_indexed_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.index_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler_nearest),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.palette_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.globals_buffer.as_entire_binding(),
                },
            ],
        });

        self.recompute_scaling();
        self.dirty.mark_full();
        Ok(())
    }

    pub fn frame(&self) -> &[u8] {
        &self.indices
    }

    pub fn frame_mut(&mut self) -> &mut [u8] {
        self.dirty.mark_full();
        &mut self.indices
    }

    pub fn set_index(&mut self, x: u32, y: u32, index: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.width + x) as usize;
        self.indices[offset] = index;
        self.dirty.mark_point(x, y);
    }

    /// Mark a region as dirty (useful if you modify `frame_mut()` partially).
    pub fn mark_dirty(&mut self, rect: Rect) {
        self.dirty.mark_rect(rect);
    }

    pub fn palette(&self) -> &[[u8; 4]; 256] {
        &self.palette
    }

    pub fn palette_mut(&mut self) -> &mut [[u8; 4]; 256] {
        self.palette_dirty = true;
        &mut self.palette
    }

    pub fn set_palette_entry(&mut self, index: u8, color: [u8; 4]) {
        self.palette[index as usize] = color;
        self.palette_dirty = true;
    }

    pub fn render(&mut self) -> Result<()> {
        if self.surface_config.width == 0 || self.surface_config.height == 0 {
            return Ok(());
        }

        let frame = self.surface.get_current_texture().or_else(|_| {
            self.surface.configure(&self.device, &self.surface_config);
            self.surface.get_current_texture()
        })?;

        if self.palette_dirty {
            self.upload_palette();
        }
        self.upload_dirty_regions();

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pixstage_indexed_command_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pixstage_indexed_render_pass"),
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
            pass.set_bind_group(0, &self.bind_group, &[]);
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

    fn upload_palette(&mut self) {
        let palette_bytes = bytemuck::cast_slice(&self.palette);
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.palette_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            palette_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.palette_dirty = false;
    }

    fn upload_dirty_regions(&mut self) {
        let regions = self.dirty.take_regions(64);
        if regions.is_empty() {
            return;
        }

        for region in regions {
            if region.x == 0
                && region.y == 0
                && region.width == self.width
                && region.height == self.height
            {
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.index_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &self.indices,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(self.width),
                        rows_per_image: Some(self.height),
                    },
                    wgpu::Extent3d {
                        width: self.width,
                        height: self.height,
                        depth_or_array_layers: 1,
                    },
                );
                continue;
            }

            let bytes_per_row = region.width as usize;
            let upload_len = bytes_per_row * region.height as usize;
            self.upload_buffer.resize(upload_len, 0);

            for row in 0..region.height as usize {
                let src_y = region.y as usize + row;
                let src_x = region.x as usize;
                let src_start = src_y * self.width as usize + src_x;
                let src_end = src_start + bytes_per_row;
                let dst_start = row * bytes_per_row;
                let dst_end = dst_start + bytes_per_row;
                self.upload_buffer[dst_start..dst_end]
                    .copy_from_slice(&self.indices[src_start..src_end]);
            }

            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.index_texture,
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
                    bytes_per_row: Some(region.width),
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

fn create_index_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pixstage_index_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_palette_texture(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pixstage_palette_texture"),
        size: wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
