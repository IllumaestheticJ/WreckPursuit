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

@fragment
fn fs_main(i: VertexOut) -> @location(0) vec4<f32> {
    // Derivatives first, in uniform control flow.
    let dx = dpdx(i.world);
    let dy = dpdy(i.world);
    let fw = fwidth(i.world.xy);

    let to_cam = normalize(g.camera_pos.xyz - i.world);
    var n = normalize(cross(dx, dy));
    if dot(n, to_cam) < 0.0 {
        n = -n;
    }
    if dot(i.normal, i.normal) > 0.25 {
        n = normalize(i.normal);
    }
    var base = i.color.rgb;
    if i.shade > 1.5 {
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
        c = tonemap(base * (hemi + g.sun_color.rgb * sun) + g.sun_color.rgb * spec);
    }

    let d = distance(g.camera_pos.xyz, i.world);
    let f = 1.0 - exp(-d * g.fog.a);
    c = mix(c, g.fog.rgb, f);
    return vec4<f32>(c, i.color.a);
}
