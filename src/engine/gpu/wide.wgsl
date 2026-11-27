// R40b: the float compositor's passes (`engine/gpu/wide.rs`; a module of their own). Sources are uploaded as
// Rgba32Float, everything composed is Rgba16Float; each pass mirrors `engine/wide/compose.rs` in f32.

struct P {
    // The pixels the pass may change: x0, y0, x1, y1.
    rect: vec4<i32>,
    // draw: 16.16 source step per pixel (du, dv) and the source size in 16.16 (umax, vmax).
    uv: vec4<i32>,
    // draw: whole-pixel offset (ox, oy), whole-pixel (1) or stepped (0), bilinear (1) or nearest (0).
    off: vec4<i32>,
    // blend: mode (`Composite as u32`), group (1); view: encoding, view transform.
    k: vec4<u32>,
    // draw: alpha; blend: opacity, lowest, highest; fade: colour r, g, b and how far in `g` below.
    f: vec4<f32>,
    // fade: how far (x); view: the peak (x).
    g: vec4<f32>,
    // view: working values (decoded) -> linear Rec.709, by rows.
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
}

@group(0) @binding(0) var dst: texture_2d<f32>;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var<uniform> p: P;
@group(0) @binding(3) var<storage, read> rows: array<vec2<i32>>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole target.
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

const SOURCE_ALPHA: u32 = 0u;
const ADD: u32 = 1u;
const SUBTRACT: u32 = 2u;
const MULTIPLY: u32 = 3u;
const SCREEN: u32 = 4u;
const OVERLAY: u32 = 5u;
const DARKEN: u32 = 6u;
const LIGHTEN: u32 = 7u;
const DIFFERENCE: u32 = 8u;
const THREE_D: u32 = 9u;
const CUT: u32 = 10u;
const HARD_LIGHT: u32 = 11u;
const DODGE: u32 = 12u;
const BURN: u32 = 13u;
const DIFFERENCE_SQUARED: u32 = 14u;
const SOFT_LIGHT: u32 = 16u;
const EXCLUSION: u32 = 17u;
const HARD_MIX: u32 = 18u;
const HUE: u32 = 19u;
const SATURATION: u32 = 20u;
const COLOR: u32 = 21u;
const LUMINOSITY: u32 = 22u;

fn over(d: vec4<f32>, s: vec4<f32>, a: f32) -> vec4<f32> {
    // `compose::over`: alpha as da + sa (1 - da), so an opaque backdrop stays exactly opaque.
    let o = s * a + d * (1.0 - s.a * a);
    return vec4<f32>(o.rgb, d.a + s.a * a * (1.0 - d.a));
}

fn texel(x: i32, y: i32) -> vec4<f32> {
    return textureLoad(src, vec2<i32>(x, y), 0);
}

// `compose::bilerp`: 16.16 position, pixel centres at integers, edges clamped, 16-bit weights.
fn bilerp(u: i32, v: i32) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(src));
    let iu = u >> 16u;
    let iv = v >> 16u;
    let fu = f32(u & 0xffff) / 65536.0;
    let fv = f32(v & 0xffff) / 65536.0;
    let x0 = clamp(iu, 0, size.x - 1);
    let x1 = clamp(iu + 1, 0, size.x - 1);
    let y0 = clamp(iv, 0, size.y - 1);
    let y1 = clamp(iv + 1, 0, size.y - 1);
    let p00 = texel(x0, y0);
    let top = p00 + (texel(x1, y0) - p00) * fu;
    let p01 = texel(x0, y1);
    let bot = p01 + (texel(x1, y1) - p01) * fu;
    return top + (bot - top) * fv;
}

@fragment
fn fs_draw(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec2<i32>(pos.xy);
    let d = textureLoad(dst, q, 0);
    if q.x < p.rect.x || q.y < p.rect.y || q.x >= p.rect.z || q.y >= p.rect.w {
        return d;
    }
    if p.off.z != 0 {
        return over(d, textureLoad(src, q - p.off.xy, 0), p.f.x);
    }
    let r = rows[q.y - p.rect.y];
    let n = q.x - p.rect.x;
    if abs(f32(r.x) + f32(p.uv.x) * f32(n)) > 2.0e9 || abs(f32(r.y) + f32(p.uv.y) * f32(n)) > 2.0e9 {
        return d;
    }
    let u = r.x + p.uv.x * n;
    let v = r.y + p.uv.y * n;
    if u < 0 || v < 0 || u >= p.uv.z || v >= p.uv.w {
        return d;
    }
    var s: vec4<f32>;
    if p.off.w != 0 {
        s = bilerp(u - 32768, v - 32768);
    } else {
        s = texel(u >> 16u, v >> 16u);
    }
    return over(d, s, p.f.x);
}

fn separable(mode: u32, b: f32, s: f32, hi: f32) -> f32 {
    switch mode {
        case ADD: { return min(b + s, hi); }
        case SUBTRACT: { return max(b - s, 0.0); }
        case MULTIPLY: { return b * s; }
        case SCREEN: { return b + s - b * s; }
        case DARKEN: { return min(b, s); }
        case LIGHTEN: { return max(b, s); }
        case DIFFERENCE: { return abs(b - s); }
        case OVERLAY: {
            if b <= 0.5 { return 2.0 * s * b; }
            return 1.0 - 2.0 * (1.0 - s) * (1.0 - b);
        }
        case HARD_LIGHT: {
            if s <= 0.5 { return 2.0 * b * s; }
            return 1.0 - 2.0 * (1.0 - b) * (1.0 - s);
        }
        case DODGE: {
            if b <= 0.0 { return 0.0; }
            if s >= 1.0 { return min(hi, 1.0e6); }
            return min(b / (1.0 - s), hi);
        }
        case BURN: {
            if b >= 1.0 { return 1.0; }
            if s <= 0.0 { return 0.0; }
            return 1.0 - min((1.0 - b) / s, 1.0);
        }
        case DIFFERENCE_SQUARED: { return (b - s) * (b - s); }
        case SOFT_LIGHT: {
            if s <= 0.5 { return b - (1.0 - 2.0 * s) * b * (1.0 - b); }
            var d: f32;
            if b <= 0.25 { d = ((16.0 * b - 12.0) * b + 4.0) * b; } else { d = sqrt(max(b, 0.0)); }
            return b + (2.0 * s - 1.0) * (d - b);
        }
        case EXCLUSION: { return b + s - 2.0 * b * s; }
        case HARD_MIX: { return select(0.0, 1.0, b + s >= 1.0); }
        default: { return s; }
    }
}

fn lum(c: vec3<f32>) -> f32 {
    return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
}

fn set_lum(c0: vec3<f32>, l0: f32) -> vec3<f32> {
    let c = c0 + vec3<f32>(l0 - lum(c0));
    let l = lum(c);
    let n = min(min(c.r, c.g), c.b);
    let x = max(max(c.r, c.g), c.b);
    var o = c;
    if n < 0.0 && l > n {
        o = vec3<f32>(l) + (o - vec3<f32>(l)) * l / (l - n);
    }
    if x > 1.0 && x > l {
        o = vec3<f32>(l) + (o - vec3<f32>(l)) * (1.0 - l) / (x - l);
    }
    return o;
}

fn sat(c: vec3<f32>) -> f32 {
    return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    // The channels' order as the CPU's stable sort gives it (ties keep r, g, b).
    var i = vec3<u32>(0u, 1u, 2u);
    var v = c;
    if v[i.y] < v[i.x] { i = vec3<u32>(i.y, i.x, i.z); }
    if v[i.z] < v[i.y] { i = vec3<u32>(i.x, i.z, i.y); }
    if v[i.y] < v[i.x] { i = vec3<u32>(i.y, i.x, i.z); }
    let lo = v[i.x];
    let mid = v[i.y];
    let hi = v[i.z];
    var o = vec3<f32>(0.0);
    if hi > lo {
        o[i.y] = (mid - lo) * s / (hi - lo);
        o[i.z] = s;
    }
    return o;
}

fn mix_mode(mode: u32, cb: vec3<f32>, cs: vec3<f32>, hi: f32) -> vec3<f32> {
    switch mode {
        case HUE: { return set_lum(set_sat(cs, sat(cb)), lum(cb)); }
        case SATURATION: { return set_lum(set_sat(cb, sat(cs)), lum(cb)); }
        case COLOR: { return set_lum(cs, lum(cb)); }
        case LUMINOSITY: { return set_lum(cb, lum(cs)); }
        default: { return vec3<f32>(separable(mode, cb.r, cs.r, hi), separable(mode, cb.g, cs.g, hi), separable(mode, cb.b, cs.b, hi)); }
    }
}

fn straight(c: vec4<f32>) -> vec3<f32> {
    if c.a > 0.0 {
        return c.rgb / c.a;
    }
    return vec3<f32>(0.0);
}

// `compose::blend_px`.
@fragment
fn fs_blend(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec2<i32>(pos.xy);
    let d = textureLoad(dst, q, 0);
    let s = textureLoad(src, q, 0);
    let mode = p.k.x;
    let group = p.k.y != 0u;
    let o = p.f.x;
    let mask = group && mode == MULTIPLY;
    if s.a <= 0.0 && !mask {
        return d;
    }
    if mode == SOURCE_ALPHA || mode == THREE_D {
        return over(d, s, o);
    }
    let sp = s * o;
    let ab = d.a;
    let a = sp.a;
    if mode == CUT {
        let k = 1.0 - a;
        return vec4<f32>(d.rgb * k, select(ab * k, 1.0, ab >= 1.0));
    }
    if mask {
        let m = min(lum(straight(sp)), 1.0) * a + (1.0 - o);
        return d * m;
    }
    if a <= 0.0 {
        return d;
    }
    let b = clamp(mix_mode(mode, straight(d), straight(sp), p.f.z), vec3<f32>(p.f.y), vec3<f32>(p.f.z));
    let c = sp.rgb * (1.0 - ab) + d.rgb * (1.0 - a) + a * ab * b;
    return vec4<f32>(c, ab + a * (1.0 - ab));
}

// `compose::fade_to`: each pixel g.x of the way to colour f.rgb at its own alpha.
@fragment
fn fs_fade(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(dst, vec2<i32>(pos.xy), 0);
    let t = p.g.x;
    if t <= 0.0 {
        return d;
    }
    let c = vec4<f32>(p.f.rgb * d.a, d.a);
    if t >= 1.0 {
        return c;
    }
    return d + (c - d) * t;
}

// ---- the view for the Video Preview (`Working::preview_px`) ----

const ENC_VIDEO: u32 = 0u;
const ENC_LINEAR: u32 = 1u;
const ENC_CC: u32 = 2u;
const ENC_CCT: u32 = 3u;
const OUT_VIDEO: u32 = 0u;
const OUT_SRGB: u32 = 1u;
const OUT_709: u32 = 2u;

fn spow(v: f32, e: f32) -> f32 {
    if v >= 0.0 { return pow(v, e); }
    return -pow(-v, e);
}

fn decode(v: f32) -> f32 {
    switch p.k.x {
        case ENC_VIDEO: { return spow(v, 2.222); }
        case ENC_CC: {
            if v < (9.72 - 15.0) / 17.52 { return (exp2(v * 17.52 - 9.72) - exp2(-16.0)) * 2.0; }
            return min(exp2(v * 17.52 - 9.72), 65504.0);
        }
        case ENC_CCT: {
            if v <= 0.15525114 { return (v - 0.07290553) / 10.540237; }
            return min(exp2(v * 17.52 - 9.72), 65504.0);
        }
        default: { return v; }
    }
}

fn rrt_odt(v: f32) -> f32 {
    return (v * (v + 0.0245786) - 0.000090537) / (v * (0.983729 * v + 0.432951) + 0.238081);
}

fn mobius(x: f32, j: f32, peak: f32) -> f32 {
    if x <= j { return x; }
    let a = -j * j * (peak - 1.0) / (j * j - 2.0 * j + peak);
    let b = (j * j - 2.0 * j * peak + peak) / max(peak - 1.0, 1e-6);
    return (b * b + 2.0 * b * j + j * j) / (b - a) * (x + a) / (x + b);
}

fn srgb(v: f32) -> f32 {
    let x = clamp(v, 0.0, 1.0);
    if x <= 0.0031308 { return x * 12.92; }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

fn view(c: vec3<f32>) -> vec3<f32> {
    let w = vec3<f32>(decode(c.r), decode(c.g), decode(c.b));
    let l = vec3<f32>(dot(p.m0.xyz, w), dot(p.m1.xyz, w), dot(p.m2.xyz, w));
    switch p.k.y {
        case OUT_VIDEO: {
            if p.k.z != 0u { return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)); }
            return clamp(vec3<f32>(spow(l.r, 0.45004500), spow(l.g, 0.45004500), spow(l.b, 0.45004500)), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        case OUT_SRGB, OUT_709: {
            let i = mat3x3<f32>(vec3<f32>(0.59719, 0.07600, 0.02840), vec3<f32>(0.35458, 0.90834, 0.13383), vec3<f32>(0.04823, 0.01566, 0.83777)) * l;
            let f = vec3<f32>(rrt_odt(i.r), rrt_odt(i.g), rrt_odt(i.b));
            let o = clamp(mat3x3<f32>(vec3<f32>(1.60475, -0.10208, -0.00327), vec3<f32>(-0.53108, 1.10813, -0.07276), vec3<f32>(-0.07367, -0.00605, 1.07602)) * f, vec3<f32>(0.0), vec3<f32>(1.0));
            if p.k.y == OUT_SRGB { return vec3<f32>(srgb(o.r), srgb(o.g), srgb(o.b)); }
            return pow(o, vec3<f32>(1.0 / 2.4));
        }
        default: {
            // An HDR view, tone-mapped to SDR for the display.
            let lp = max(l, vec3<f32>(0.0));
            let m = max(max(lp.r, lp.g), lp.b);
            var k = 1.0;
            if m > 1e-9 { k = mobius(m, 0.5, p.g.x) / m; }
            let o = min(lp * k, vec3<f32>(1.0));
            return vec3<f32>(spow(o.r, 0.45004500), spow(o.g, 0.45004500), spow(o.b, 0.45004500));
        }
    }
}

// The frame through the view, premultiplied, into the 8-bit output the preview samples.
@fragment
fn fs_view(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(dst, vec2<i32>(pos.xy), 0);
    let a = clamp(d.a, 0.0, 1.0);
    if a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let a8 = round(a * 255.0) / 255.0;
    let c = clamp(view(d.rgb / a), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(round(c * a8 * 255.0) / 255.0, a8);
}
