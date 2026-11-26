// R21a: what the GPU compositor's shaders share (`engine/gpu/mod.rs` prepends this file to each of them): the
// fullscreen vertex shader, the CPU compositor's integer helpers (`engine/composite.rs`), and an f32 emulation that
// replays the CPU's maths exactly. A GPU compiler may fuse a multiply into an add and reassociate sums; the CPU does
// neither. So every float result goes through `rd` (an xor with a uniform zero the compiler cannot see through, which
// each shader supplies as `zero()`), which pins each operation as written, and division and square root take one
// correction step from the GPU's approximations to the correctly rounded result (the residual exact by Dekker's
// product: a driver may lower `fma` unfused). `gpu_float_ops_round_as_the_cpu` checks all three.

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole target.
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

// round(x * y / 255), `composite::mul255`.
fn m1(x: u32, y: u32) -> u32 {
    let t = x * y + 128u;
    return (t + (t >> 8u)) >> 8u;
}

fn m4(x: vec4<u32>, y: u32) -> vec4<u32> {
    return vec4<u32>(m1(x.r, y), m1(x.g, y), m1(x.b, y), m1(x.a, y));
}

// `composite::over`: premultiplied source-over of `s` scaled by `a8`.
fn over(d: vec4<u32>, s0: vec4<u32>, a8: u32) -> vec4<u32> {
    var s = s0;
    if a8 != 255u {
        s = m4(s0, a8);
    }
    if s.a == 255u {
        return s;
    }
    return min(s + m4(d, 255u - s.a), vec4<u32>(255u));
}

// `vfx::scale8`: every channel scaled by `m` / 255 (a coverage); unchanged at 255.
fn scale8(p: vec4<u32>, m: u32) -> vec4<u32> {
    if m == 255u {
        return p;
    }
    return m4(p, m);
}

fn rd(x: f32) -> f32 {
    return bitcast<f32>(bitcast<u32>(x) ^ zero());
}

fn add(a: f32, b: f32) -> f32 {
    return rd(a + b);
}

fn sub(a: f32, b: f32) -> f32 {
    return rd(a - b);
}

fn mul(a: f32, b: f32) -> f32 {
    return rd(a * b);
}

// x = hi + lo exactly, each with at most 12 significant bits (Veltkamp).
fn split(x: f32) -> vec2<f32> {
    let c = mul(4097.0, x);
    let hi = sub(c, sub(c, x));
    return vec2<f32>(hi, sub(x, hi));
}

// a - x * y exactly, for x * y within a factor of 2 of a (a correction step's residual).
fn residual(a: f32, x: f32, y: f32) -> f32 {
    let pr = mul(x, y);
    let xs = split(x);
    let ys = split(y);
    let err = add(add(add(sub(mul(xs.x, ys.x), pr), mul(xs.x, ys.y)), mul(xs.y, ys.x)), mul(xs.y, ys.y));
    return sub(sub(a, pr), err);
}

fn div(a: f32, b: f32) -> f32 {
    let q = rd(a / b);
    if b == 0.0 || q == 0.0 {
        return q;
    }
    return add(q, rd(residual(a, q, b) / b));
}

fn root(x: f32) -> f32 {
    let s = rd(sqrt(x));
    if s == 0.0 {
        return s;
    }
    return add(s, rd(residual(x, s, s) / mul(2.0, s)));
}

fn mul3(v: vec3<f32>, s: f32) -> vec3<f32> {
    return vec3<f32>(mul(v.r, s), mul(v.g, s), mul(v.b, s));
}

fn add3(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(add(a.r, b.r), add(a.g, b.g), add(a.b, b.b));
}

// Rust's `f32::round` of a non-negative value: to the nearest integer, a half up (WGSL's `round` takes a half to the
// even integer, and `floor(x + 0.5)` is wrong just under a half, where the sum rounds up to the integer).
fn round_half_up(x: vec4<f32>) -> vec4<f32> {
    return select(round(x), floor(x) + 1.0, fract(x) == vec4<f32>(0.5));
}

// Rust's `f32::round` (a half away from zero) of one value of either sign.
fn round_away(x: f32) -> f32 {
    let r = round_half_up(vec4<f32>(abs(x))).x;
    return select(r, -r, x < 0.0);
}

// 0..1 to 0..255, rounded as the CPU's `(v.clamp(0, 1) * 255).round()`.
fn quantize(c: vec4<f32>) -> vec4<u32> {
    let v = clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<u32>(round_half_up(vec4<f32>(mul(v.r, 255.0), mul(v.g, 255.0), mul(v.b, 255.0), mul(v.a, 255.0))));
}

// `vfx::lerp_px`: `a` to `b` by `t`; exactly `a` at 0 and `b` at 1, each channel `(a + (b - a) * t).round()`.
fn lerp_px(a: vec4<u32>, b: vec4<u32>, t: f32) -> vec4<u32> {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let x = vec4<f32>(a);
    let to = vec4<f32>(b) - x;
    return vec4<u32>(round_half_up(vec4<f32>(add(x.r, mul(to.r, t)), add(x.g, mul(to.g, t)), add(x.b, mul(to.b, t)), add(x.a, mul(to.a, t)))));
}

// `vfx::smoothstep`.
fn smoothstep_cpu(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return select(0.0, 1.0, x >= e0);
    }
    let t = clamp(div(sub(x, e0), sub(e1, e0)), 0.0, 1.0);
    return mul(mul(t, t), sub(3.0, mul(2.0, t)));
}

// `vfx::unpremul`: straight RGB 0..1 of a premultiplied pixel (black when transparent).
fn unpremul(p: vec4<u32>) -> vec3<f32> {
    if p.a == 0u {
        return vec3<f32>(0.0);
    }
    let a = f32(p.a);
    return vec3<f32>(div(f32(p.r), a), div(f32(p.g), a), div(f32(p.b), a));
}

// `vfx::repremul`: straight RGB 0..1 premultiplied by alpha byte `a`.
fn repremul(c: vec3<f32>, a: u32) -> vec4<u32> {
    let fa = f32(a);
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let q = round_half_up(vec4<f32>(mul(v.r, fa), mul(v.g, fa), mul(v.b, fa), 0.0));
    return vec4<u32>(u32(q.x), u32(q.y), u32(q.z), a);
}
