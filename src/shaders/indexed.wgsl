struct Globals {
    ndc_scale: vec2<f32>,
    _pad: vec2<f32>,
}

struct VertexOutput {
    @location(0) tex_coord: vec2<f32>,
    @builtin(position) position: vec4<f32>,
}

@group(0) @binding(0) var r_tex_index: texture_2d<f32>;
@group(0) @binding(1) var r_tex_sampler: sampler;
@group(0) @binding(2) var r_tex_palette: texture_2d<f32>;
@group(0) @binding(3) var<uniform> r_globals: Globals;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.tex_coord = fma(position, vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5));
    out.position = vec4<f32>(position * r_globals.ndc_scale, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(@location(0) tex_coord: vec2<f32>) -> @location(0) vec4<f32> {
    let idx_norm = textureSample(r_tex_index, r_tex_sampler, tex_coord).r;
    let idx = floor(idx_norm * 255.0 + 0.5);
    let u = (idx + 0.5) / 256.0;
    return textureSample(r_tex_palette, r_tex_sampler, vec2<f32>(u, 0.5));
}

