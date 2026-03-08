// crt.wgsl — CRT post-process shader.
// mode 0 = Sharp (passthrough), 1 = Scanlines, 2 = CRT

struct Uniforms {
    source_size:       vec2<f32>,
    output_size:       vec2<f32>,
    mode:              u32,
    scanline_strength: f32,
    barrel_k:          f32,
    bloom_amount:      f32,
}

@group(0) @binding(0) var src_tex:     texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(1) @binding(0) var<uniform> u:  Uniforms;

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0)       uv:  vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    var out: VertOut;
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    out.uv  = uv;
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    return out;
}

fn barrel(uv: vec2<f32>, k: f32) -> vec2<f32> {
    let c  = uv * 2.0 - 1.0;
    let r2 = dot(c, c);
    return (c * (1.0 + k * r2)) * 0.5 + 0.5;
}

fn scanline(uv_y: f32, src_h: f32, strength: f32) -> f32 {
    // Darken every other output row. The render target is source-resolution,
    // so fract() of the pixel centre is always 0.5 — useless. Instead key
    // off the integer row index; egui upscales the result for display.
    let row = u32(uv_y * src_h);
    return select(1.0, 1.0 - strength, (row & 1u) == 0u);
}

fn aperture(uv_x: f32, src_w: f32) -> vec3<f32> {
    let col = u32(uv_x * src_w) % 3u;
    if col == 0u { return vec3<f32>(1.00, 0.85, 0.85); }
    if col == 1u { return vec3<f32>(0.85, 1.00, 0.85); }
    return          vec3<f32>(0.85, 0.85, 1.00);
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let uv = in.uv;

    if u.mode == 0u {
        return textureSample(src_tex, src_sampler, uv);
    }

    if u.mode == 1u {
        let c  = textureSample(src_tex, src_sampler, uv);
        let sl = scanline(uv.y, u.source_size.y, u.scanline_strength);
        return vec4<f32>(c.rgb * sl, c.a);
    }

    let duv = barrel(uv, u.barrel_k);
    if duv.x < 0.0 || duv.x > 1.0 || duv.y < 0.0 || duv.y > 1.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    let dx = 1.0 / u.source_size.x;
    let dy = 1.0 / u.source_size.y;
    var bloom = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            bloom += textureSample(src_tex, src_sampler,
                         duv + vec2<f32>(f32(x)*dx, f32(y)*dy)).rgb;
        }
    }
    bloom /= 9.0;

    var col = textureSample(src_tex, src_sampler, duv).rgb;
    col = mix(col, bloom, u.bloom_amount);
    col *= scanline(duv.y, u.source_size.y, u.scanline_strength);
    col *= aperture(duv.x, u.source_size.x);

    return vec4<f32>(col, 1.0);
}
