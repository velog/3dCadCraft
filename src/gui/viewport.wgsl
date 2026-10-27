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
    // Section View's second plane (Quarter / Three Quarter); 0 = none.
    clip2: vec4<f32>,
    // x: monochrome faces, y: the two planes cut where either does
    // (Three Quarter) instead of where both do (Quarter).
    style: vec4<f32>,
};

fn cut_by(c: vec4<f32>, p: vec3<f32>) -> bool {
    return dot(c.xyz, c.xyz) > 0.0 && dot(c.xyz, p) > c.w;
}

fn sliced(p: vec3<f32>) -> bool {
    let a = cut_by(u.clip, p);
    if (dot(u.clip2.xyz, u.clip2.xyz) == 0.0) {
        return a;
    }
    let b = cut_by(u.clip2, p);
    if (u.style.y > 0.5) {
        return a || b;
    }
    return a && b;
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
    var col = i.color;
    if (u.style.x > 0.5) {
        // Monochrome: one light grey, the shading stays.
        col = vec3<f32>(0.62);
    }
    return vec4<f32>(to_gamma(col * diffuse + vec3<f32>(spec)), 1.0);
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

// Hidden edges (behind faces): dashed in screen space, faded towards the
// background.
@fragment
fn fs_hidden(i: VsOut) -> @location(0) vec4<f32> {
    let k = (i.clip.x + i.clip.y) / 8.0;
    let dash = abs(k - round(k)) * 8.0 < 2.5;
    if (sliced(i.world) || dash) {
        discard;
    }
    return vec4<f32>(to_gamma(mix(i.color, vec3<f32>(0.62, 0.64, 0.68), 0.6)), 1.0);
}

// Unlit: edges, axes, grid.
@fragment
fn fs_flat(i: VsOut) -> @location(0) vec4<f32> {
    if (sliced(i.world)) {
        discard;
    }
    return vec4<f32>(to_gamma(i.color), 1.0);
}

// See-through faces (appearance opacity < 1): colour with alpha, lit like
// the shaded faces from either side, blended over the opaque scene.
struct AlphaIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct AlphaOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_alpha(v: AlphaIn) -> AlphaOut {
    var o: AlphaOut;
    o.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    o.world = v.position;
    o.normal = v.normal;
    o.color = v.color;
    return o;
}

@fragment
fn fs_alpha(i: AlphaOut) -> @location(0) vec4<f32> {
    if (sliced(i.world)) {
        discard;
    }
    let n = normalize(i.normal);
    let v = normalize(u.eye.xyz - i.world);
    let ndv = abs(dot(n, v));
    let diffuse = 0.25 + 0.75 * ndv;
    let spec = 0.15 * pow(ndv, 48.0);
    return vec4<f32>(to_gamma(i.color.rgb * diffuse + vec3<f32>(spec)), i.color.a);
}

// Cosmetic thread texture: a sleeve just off the threaded face. `phase`
// is h / pitch -/+ angle / 2pi, so whole numbers run along one helix of the
// thread. The output multiplies the shaded face under it by 2 * value
// (blend Dst * Src + Src * Dst): 0.5 leaves it unchanged, so the bands keep
// the face's appearance and lighting.
struct ThreadIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) phase: f32,
};

struct ThreadOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) phase: f32,
};

@vertex
fn vs_thread(v: ThreadIn) -> ThreadOut {
    var o: ThreadOut;
    o.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    o.world = v.position;
    o.normal = v.normal;
    o.phase = v.phase;
    return o;
}

@fragment
fn fs_thread(i: ThreadOut) -> @location(0) vec4<f32> {
    // Derivatives first, in uniform control flow.
    let w = fwidth(i.phase);
    if (sliced(i.world)) {
        discard;
    }
    // A crest (light) and a root (dark) per pitch, with soft flanks.
    let f = fract(i.phase);
    let tri = 1.0 - abs(2.0 * f - 1.0);
    let band = smoothstep(0.2, 0.8, tri);
    // Grazing parts of the sleeve a little darker (the flanks turn away).
    let n = normalize(i.normal);
    let v = normalize(u.eye.xyz - i.world);
    let lit = 0.85 + 0.15 * abs(dot(n, v));
    var s = mix(0.30, 0.62, band) * lit;
    // Bands thinner than a few pixels fade to their mean (no moire).
    s = mix(s, 0.44, clamp((w - 0.2) / 0.3, 0.0, 1.0));
    return vec4<f32>(s, s, s, 1.0);
}
