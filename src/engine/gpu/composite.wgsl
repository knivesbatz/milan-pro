// R21a: the GPU compositor's passes (`engine/gpu/mod.rs`; math.wgsl comes first). Each pass draws the whole frame
// into a new Rgba8Uint target from `dst` (copied where the pass changes nothing), with the CPU compositor's own
// integer maths (`engine/composite.rs`) and its f32 maths through math.wgsl's emulation, so every mode is bit-exact.

struct P {
    // The pixels the pass may change: x0, y0, x1, y1.
    rect: vec4<i32>,
    // draw: 16.16 source step per pixel along x (du, dv) and the source size in 16.16 (umax, vmax).
    uv: vec4<i32>,
    // draw: whole-pixel offset (ox, oy), whole-pixel (1) or stepped (0), bilinear (1) or nearest (0).
    off: vec4<i32>,
    // draw: alpha (0..255); blend: opacity (0..255), mode (`Composite as u32`), group (1); fade: r, g, b. w is 0.
    k: vec4<u32>,
    // blend: opacity (0..1); fade: y = how far (0..1).
    f: vec4<f32>,
}

@group(0) @binding(0) var dst: texture_2d<u32>;
@group(0) @binding(1) var src: texture_2d<u32>;
@group(0) @binding(2) var<uniform> p: P;
// draw: each row's 16.16 source position at x0 (from the CPU, in f64 as `composite::draw` computes it).
@group(0) @binding(3) var<storage, read> rows: array<vec2<i32>>;

// math.wgsl's `rd`: a zero the compiler cannot see through.
fn zero() -> u32 {
    return p.k.w;
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

fn texel(x: i32, y: i32) -> vec4<u32> {
    return textureLoad(src, vec2<i32>(x, y), 0);
}

// `composite::bilerp`: 16.16 position, pixel centres at integers, edges clamped.
fn bilerp(u: i32, v: i32) -> vec4<u32> {
    let size = vec2<i32>(textureDimensions(src));
    let iu = u >> 16u;
    let iv = v >> 16u;
    let fu = u32((u >> 8u) & 255);
    let fv = u32((v >> 8u) & 255);
    let x0 = clamp(iu, 0, size.x - 1);
    let x1 = clamp(iu + 1, 0, size.x - 1);
    let y0 = clamp(iv, 0, size.y - 1);
    let y1 = clamp(iv + 1, 0, size.y - 1);
    let top = texel(x0, y0) * (256u - fu) + texel(x1, y0) * fu;
    let bot = texel(x0, y1) * (256u - fu) + texel(x1, y1) * fu;
    return (top * (256u - fv) + bot * fv + vec4<u32>(32768u)) >> vec4<u32>(16u);
}

// `composite::draw`: the source through an affine map at alpha k.x, source-over.
@fragment
fn fs_draw(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let d = textureLoad(dst, q, 0);
    if q.x < p.rect.x || q.y < p.rect.y || q.x >= p.rect.z || q.y >= p.rect.w {
        return d;
    }
    if p.off.z != 0 {
        return over(d, textureLoad(src, q - p.off.xy, 0), p.k.x);
    }
    let r = rows[q.y - p.rect.y];
    let n = q.x - p.rect.x;
    // The CPU steps in i64: a position this far out is outside any source (and would wrap in i32).
    if abs(f32(r.x) + f32(p.uv.x) * f32(n)) > 2.0e9 || abs(f32(r.y) + f32(p.uv.y) * f32(n)) > 2.0e9 {
        return d;
    }
    let u = r.x + p.uv.x * n;
    let v = r.y + p.uv.y * n;
    if u < 0 || v < 0 || u >= p.uv.z || v >= p.uv.w {
        return d;
    }
    var s: vec4<u32>;
    if p.off.w != 0 {
        s = bilerp(u - 32768, v - 32768);
    } else {
        s = texel(u >> 16u, v >> 16u);
    }
    return over(d, s, p.k.x);
}

// `composite::blend_channel`: the integer modes on one channel of an opaque backdrop.
fn channel(mode: u32, cd: u32, cs: u32) -> u32 {
    switch mode {
        case ADD: { return min(cd + cs, 255u); }
        case SUBTRACT: { return select(0u, cd - cs, cd >= cs); }
        case MULTIPLY: { return m1(cs, cd); }
        case SCREEN: { return cs + cd - m1(cs, cd); }
        case OVERLAY: {
            if cd < 128u {
                return m1(2u * cs, cd);
            }
            return 255u - m1(2u * (255u - cs), 255u - cd);
        }
        case DARKEN: { return min(cs, cd); }
        case LIGHTEN: { return max(cs, cd); }
        case DIFFERENCE: { return select(cs - cd, cd - cs, cd > cs); }
        default: { return cs; }
    }
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        return mul(mul(2.0, b), s);
    }
    return sub(1.0, mul(mul(2.0, sub(1.0, b)), sub(1.0, s)));
}

// `composite::separable`.
fn separable(mode: u32, b: f32, s: f32) -> f32 {
    switch mode {
        case ADD: { return min(add(b, s), 1.0); }
        case SUBTRACT: { return max(sub(b, s), 0.0); }
        case MULTIPLY: { return mul(b, s); }
        case SCREEN: { return sub(add(b, s), mul(b, s)); }
        case OVERLAY: { return hard_light(s, b); }
        case DARKEN: { return min(b, s); }
        case LIGHTEN: { return max(b, s); }
        case DIFFERENCE: { return abs(sub(b, s)); }
        case HARD_LIGHT: { return hard_light(b, s); }
        case DODGE: {
            if b <= 0.0 {
                return 0.0;
            }
            if s >= 1.0 {
                return 1.0;
            }
            return min(div(b, sub(1.0, s)), 1.0);
        }
        case BURN: {
            if b >= 1.0 {
                return 1.0;
            }
            if s <= 0.0 {
                return 0.0;
            }
            return sub(1.0, min(div(sub(1.0, b), s), 1.0));
        }
        case DIFFERENCE_SQUARED: { return mul(sub(b, s), sub(b, s)); }
        case SOFT_LIGHT: {
            if s <= 0.5 {
                return sub(b, mul(mul(sub(1.0, mul(2.0, s)), b), sub(1.0, b)));
            }
            var d = root(b);
            if b <= 0.25 {
                d = mul(add(mul(sub(mul(16.0, b), 12.0), b), 4.0), b);
            }
            return add(b, mul(sub(mul(2.0, s), 1.0), sub(d, b)));
        }
        case EXCLUSION: { return sub(add(b, s), mul(mul(2.0, b), s)); }
        case HARD_MIX: { return select(0.0, 1.0, add(b, s) >= 1.0); }
        default: { return s; }
    }
}

fn lum(c: vec3<f32>) -> f32 {
    return add(add(mul(0.3, c.r), mul(0.59, c.g)), mul(0.11, c.b));
}

// l + (c - l) * k / den per channel (ClipColor).
fn clip(c: vec3<f32>, l: f32, k: f32, den: f32) -> vec3<f32> {
    return vec3<f32>(add(l, div(mul(sub(c.r, l), k), den)), add(l, div(mul(sub(c.g, l), k), den)), add(l, div(mul(sub(c.b, l), k), den)));
}

// W3C SetLum with ClipColor (`composite::set_lum`).
fn set_lum(c0: vec3<f32>, l0: f32) -> vec3<f32> {
    let d = sub(l0, lum(c0));
    var c = add3(c0, vec3<f32>(d));
    let l = lum(c);
    let n = min(min(c.r, c.g), c.b);
    let x = max(max(c.r, c.g), c.b);
    // A grey out of range (l == n or x == l) is left for the caller's clamp, as the CPU leaves it: no 0 / 0.
    if n < 0.0 && l > n {
        c = clip(c, l, l, sub(l, n));
    }
    if x > 1.0 && x > l {
        c = clip(c, l, sub(1.0, l), sub(x, l));
    }
    return c;
}

fn sat(c: vec3<f32>) -> f32 {
    return sub(max(max(c.r, c.g), c.b), min(min(c.r, c.g), c.b));
}

// W3C SetSat (`composite::set_sat`): a stable sort of the three channels.
fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    var i = vec3<u32>(0u, 1u, 2u);
    if c[i.x] > c[i.y] {
        i = i.yxz;
    }
    if c[i.y] > c[i.z] {
        i = i.xzy;
    }
    if c[i.x] > c[i.y] {
        i = i.yxz;
    }
    let lo = c[i.x];
    let mid = c[i.y];
    let hi = c[i.z];
    var o = vec3<f32>(0.0);
    if hi > lo {
        o[i.y] = div(mul(sub(mid, lo), s), sub(hi, lo));
        o[i.z] = s;
    }
    return o;
}

// `composite::mix`: B(Cb, Cs) of straight colours.
fn mix_mode(mode: u32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    switch mode {
        case HUE: { return set_lum(set_sat(cs, sat(cb)), lum(cb)); }
        case SATURATION: { return set_lum(set_sat(cb, sat(cs)), lum(cb)); }
        case COLOR: { return set_lum(cs, lum(cb)); }
        case LUMINOSITY: { return set_lum(cb, lum(cs)); }
        default: { return vec3<f32>(separable(mode, cb.r, cs.r), separable(mode, cb.g, cs.g), separable(mode, cb.b, cs.b)); }
    }
}

fn straight(c: vec4<f32>, a: f32) -> vec3<f32> {
    if a > 0.0 {
        return vec3<f32>(div(c.r, a), div(c.g, a), div(c.b, a));
    }
    return vec3<f32>(0.0);
}

// 0..255 to 0..1.
fn unit(v: vec4<u32>) -> vec4<f32> {
    let f = vec4<f32>(v);
    return vec4<f32>(div(f.r, 255.0), div(f.g, 255.0), div(f.b, 255.0), div(f.a, 255.0));
}

// `composite::blend_px`: the W3C general formula, any backdrop alpha.
fn blend_px(d: vec4<u32>, s: vec4<u32>, mode: u32, o: f32, group: bool) -> vec4<u32> {
    let dp = unit(d);
    let su = unit(s);
    let sp = vec4<f32>(mul(su.r, o), mul(su.g, o), mul(su.b, o), mul(su.a, o));
    let ab = dp.a;
    let a = sp.a;
    if mode == CUT {
        let k = sub(1.0, a);
        return quantize(vec4<f32>(mul3(dp.rgb, k), select(mul(ab, k), 1.0, ab >= 1.0)));
    }
    if mode == MULTIPLY && group {
        // Multiply (Mask): at opacity o the mask goes 1 - o of the way back to clear.
        let m = add(mul(min(lum(straight(sp, a)), 1.0), a), sub(1.0, o));
        return quantize(vec4<f32>(mul3(dp.rgb, m), mul(dp.a, m)));
    }
    if a <= 0.0 {
        return d;
    }
    let b = clamp(mix_mode(mode, straight(dp, ab), straight(sp, a)), vec3<f32>(0.0), vec3<f32>(1.0));
    let c = add3(add3(mul3(sp.rgb, sub(1.0, ab)), mul3(dp.rgb, sub(1.0, a))), mul3(b, mul(a, ab)));
    return quantize(vec4<f32>(c, sub(add(a, ab), mul(a, ab))));
}

// One channel of the integer path of `composite::blend`.
fn int_channel(mode: u32, cd: u32, s: u32, sa: u32) -> u32 {
    let cs = min((s * 255u + sa / 2u) / sa, 255u);
    return min(m1(cd, 255u - sa) + m1(sa, channel(mode, cd, cs)), 255u);
}

// `composite::blend` (not Custom): the layer `src` onto `dst` with mode k.y at opacity k.x.
@fragment
fn fs_blend(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let d = textureLoad(dst, q, 0);
    let s = textureLoad(src, q, 0);
    let o8 = p.k.x;
    let mode = p.k.y;
    let group = p.k.z != 0u;
    let mask = group && mode == MULTIPLY;
    if s.a == 0u && !mask {
        return d;
    }
    if mode == SOURCE_ALPHA || mode == THREE_D {
        return over(d, s, o8);
    }
    if d.a < 255u || mask || mode < ADD || mode > DIFFERENCE {
        return blend_px(d, s, mode, p.f.x, group);
    }
    let sm = m4(s, o8);
    if sm.a == 0u {
        return d;
    }
    return vec4<u32>(int_channel(mode, d.r, sm.r, sm.a), int_channel(mode, d.g, sm.g, sm.a), int_channel(mode, d.b, sm.b, sm.a), 255u);
}

// `composite::fade_to`: each pixel f.y of the way to colour k.rgb at its own alpha (`vfx::lerp_px`).
@fragment
fn fs_fade(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let d = textureLoad(dst, vec2<i32>(pos.xy), 0);
    let c = vec4<u32>(m1(p.k.x, d.a), m1(p.k.y, d.a), m1(p.k.z, d.a), d.a);
    return lerp_px(d, c, p.f.y);
}
