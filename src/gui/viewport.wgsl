// 3DCadCraft viewport shader.
//
// Colours arrive in linear space; output is gamma-encoded because the target
// is a plain Rgba8Unorm texture that egui samples as gamma-space data.

struct Uniforms {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // Slice Graphics: fragments with dot(clip.xyz, p) > clip.w are cut away;
    // clip.xyz = 0 disables it.
    clip: vec4<f32>,
};

fn sliced(p: vec3<f32>) -> bool {
    return dot(u.clip.xyz, u.clip.xyz) > 0.0 && dot(u.clip.xyz, p) > u.clip.w;
}

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    o.world = v.position;
    o.normal = v.normal;
    o.color = v.color;
    return o;
}

fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    return pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
}

// Shaded front faces: headlight (light at the eye) + a little specular.
@fragment
fn fs_shaded(i: VsOut) -> @location(0) vec4<f32> {
    if (sliced(i.world)) {
        discard;
    }
    let n = normalize(i.normal);
    let v = normalize(u.eye.xyz - i.world);
    let ndv = abs(dot(n, v));
    let diffuse = 0.25 + 0.75 * ndv;
    let spec = 0.15 * pow(ndv, 48.0);
    return vec4<f32>(to_gamma(i.color * diffuse + vec3<f32>(spec)), 1.0);
}

// Back faces. In a valid closed solid they are always hidden behind front
// faces; if one is visible the face is wrongly oriented or the shell is open,
// so paint it red to expose the kernel bug at a glance.
@fragment
fn fs_backface(i: VsOut) -> @location(0) vec4<f32> {
    if (sliced(i.world)) {
        discard;
    }
    let ndv = abs(dot(normalize(i.normal), normalize(u.eye.xyz - i.world)));
    if (dot(u.clip.xyz, u.clip.xyz) > 0.0) {
        // Sliced: through the cut we see the inside of the solid, i.e. its
        // back faces — exactly the section area. Paint it as a flat section
        // fill with 45° hatching in screen space (like Inventor's cut faces).
        let k = (i.clip.x + i.clip.y) / 9.0;
        let line = abs(k - round(k)) * 9.0 < 1.1;
        var c = vec3<f32>(0.62, 0.66, 0.74);
        if (line) {
            c = vec3<f32>(0.22, 0.26, 0.34);
        }
        return vec4<f32>(to_gamma(c), 1.0);
    }
    return vec4<f32>(to_gamma(vec3<f32>(0.75, 0.05, 0.05) * (0.3 + 0.7 * ndv)), 1.0);
}

// Unlit: edges, axes, grid.
@fragment
fn fs_flat(i: VsOut) -> @location(0) vec4<f32> {
    if (sliced(i.world)) {
        discard;
    }
    return vec4<f32>(to_gamma(i.color), 1.0);
}
