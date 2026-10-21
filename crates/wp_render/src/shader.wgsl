// wp_render: one shader for everything. Flat normals come from screen-space derivatives of the
// world position, so deforming bodies need no normal recomputation on the CPU; smooth-shaded
// geometry (city terrain) passes its own normal (zero means flat).

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    // xyz: direction toward the sun (unit).
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    sky_color: vec4<f32>,
    ground_color: vec4<f32>,
    // rgb: fog colour, a: density per metre.
    fog: vec4<f32>,
    // x: night factor 0..1, y: spot lights used.
    night: vec4<f32>,
    lights: array<Spot, 32>,
};

// A headlight: position + range (m), direction + cos(outer cone), colour + cos(inner cone).
struct Spot {
    pos_range: vec4<f32>,
    dir_outer: vec4<f32>,
    color_inner: vec4<f32>,
};

struct Object {
    model: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> o: Object;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) shade: f32,
    @location(3) normal: vec3<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) shade: f32,
    @location(3) normal: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world = o.model * vec4<f32>(v.pos, 1.0);
    out.clip = g.view_proj * world;
    out.world = world.xyz;
    out.color = v.color;
    out.shade = v.shade;
    out.normal = (o.model * vec4<f32>(v.normal, 0.0)).xyz;
    return out;
}

// Anti-aliased grid line strength for spacing `step` (m).
fn grid(p: vec2<f32>, fw: vec2<f32>, step: f32, width: f32) -> f32 {
    let q = abs(fract(p / step - 0.5) - 0.5) * step;
    let l = 1.0 - smoothstep(vec2<f32>(0.0), fw * width, q);
    return max(l.x, l.y);
}

// Filmic curve (Narkowicz's ACES fit) at a fixed exposure, so pale greybox surfaces (sidewalks,
// concrete) in full sun roll off instead of clipping to white. Lit colour only: fog and the
// clear colour stay as given so the horizon matches.
fn tonemap(x: vec3<f32>) -> vec3<f32> {
    let e = x * 0.8;
    return clamp((e * (2.51 * e + 0.03)) / (e * (2.43 * e + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Light reaching `p` (normal `n`) from the spot lights.
fn spot_light(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let count = u32(g.night.y);
    for (var k = 0u; k < count; k++) {
        let s = g.lights[k];
        let to_light = s.pos_range.xyz - p;
        let d = length(to_light);
        let l = to_light / max(d, 1e-3);
        let cone = smoothstep(s.dir_outer.w, s.color_inner.w, dot(-l, s.dir_outer.xyz));
        let fade = clamp(1.0 - pow(d / s.pos_range.w, 4.0), 0.0, 1.0);
        let falloff = fade * fade / (1.0 + d * d * 0.04);
        sum += s.color_inner.rgb * (cone * falloff * max(dot(n, l), 0.0));
    }
    return sum;
}

// Integer hash of a cell (PCG-style), 0..1. Exact on every GPU, unlike sin-based hashes.
fn hash3(c: vec3<i32>) -> f32 {
    var h = bitcast<u32>(c.x) * 747796405u + bitcast<u32>(c.y) * 2891336453u
        + bitcast<u32>(c.z) * 277803737u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h = (h >> 22u) ^ h;
    return f32(h & 0xffffu) / 65535.0;
}

// Window grid on a building wall: x = 1 inside a window pane, y = 1 if that window is lit at
// night (about 40 % are). Panes are 3.2 m apart along the wall and 3.5 m apart in height; the
// wall's own offset seeds the pattern so neighbouring buildings differ.
fn window(p: vec3<f32>, n: vec3<f32>) -> vec2<f32> {
    if abs(n.z) > 0.3 {
        return vec2<f32>(0.0);
    }
    let t = normalize(vec2<f32>(-n.y, n.x));
    let uv = vec2<f32>(dot(p.xy, t) / 3.2, p.z / 3.5);
    let f = fract(uv);
    let pane = step(0.25, f.x) * step(f.x, 0.75) * step(0.3, f.y) * step(f.y, 0.75);
    let wall = i32(floor(dot(p.xy, normalize(n.xy)) * 0.5));
    let lit = step(hash3(vec3<i32>(vec2<i32>(floor(uv)), wall)), 0.4);
    return vec2<f32>(pane, pane * lit);
}

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    // Derivatives first, in uniform control flow.
    let dx = dpdx(i.world);
    let dy = dpdy(i.world);
    let fw = fwidth(i.world.xy);

    let to_cam = normalize(g.camera_pos.xyz - i.world);
    var n = normalize(cross(dx, dy));
    if dot(i.normal, i.normal) > 0.25 {
        n = normalize(i.normal);
    }
    // Face the camera (single quads are seen from both sides).
    if dot(n, to_cam) < 0.0 {
        n = -n;
    }
    var base = i.color.rgb;
    var glow = vec3<f32>(0.0);
    if i.shade > 2.5 {
        // Building: darker glass panes, a warm glow from the lit ones at night.
        let w = window(i.world, n);
        base = mix(base, base * 0.45, w.x);
        glow = vec3<f32>(1.0, 0.72, 0.42) * (w.y * g.night.x * 0.9);
    } else if i.shade > 1.5 {
        // Ground: asphalt with a 1 m grid and a stronger 10 m grid.
        n = vec3<f32>(0.0, 0.0, 1.0);
        let fine = grid(i.world.xy, fw, 1.0, 1.0);
        let coarse = grid(i.world.xy, fw, 10.0, 1.5);
        base = mix(base, base * 1.6, fine * 0.35);
        base = mix(base, vec3<f32>(0.32, 0.33, 0.30), coarse * 0.6);
    }

    var c: vec3<f32>;
    if i.shade > 0.5 && i.shade < 1.5 {
        c = base;
    } else {
        let sun = max(dot(n, g.sun_dir.xyz), 0.0);
        let hemi = mix(g.ground_color.rgb, g.sky_color.rgb, n.z * 0.5 + 0.5);
        let h = normalize(g.sun_dir.xyz + to_cam);
        let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.25 * sun;
        // Daylight drowns headlights: full strength only at night.
        let lamps = spot_light(i.world, n) * max(g.night.x, 0.1);
        c = tonemap(base * (hemi + g.sun_color.rgb * sun + lamps) + g.sun_color.rgb * spec);
        c = min(c + glow, vec3<f32>(1.0));
    }

    let d = distance(g.camera_pos.xyz, i.world);
    let f = 1.0 - exp(-d * g.fog.a);
    c = mix(c, g.fog.rgb, f);
    return vec4<f32>(c, i.color.a);
}
