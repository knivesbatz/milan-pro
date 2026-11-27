// MVFX: what the music-video FX and transition passes share (math.wgsl comes first, the pass file after). The twin
// of `engine/mvfx/pm.rs`: each function makes the same f32 operations in the same order (through math.wgsl's
// `add`, `sub`, `mul`, `div`, `root`), so a pass gives the CPU kernel's bits.

struct M {
    // Integers: what each pass says.
    i: vec4<i32>,
    j: vec4<i32>,
    // z.x is 0 (math.wgsl's `rd`).
    z: vec4<u32>,
    f: array<vec4<f32>, 12>,
}

@group(0) @binding(0) var dst: texture_2d<u32>;
@group(0) @binding(1) var src: texture_2d<u32>;
@group(0) @binding(2) var<uniform> u: M;
@group(0) @binding(3) var<storage, read> data: array<u32>;

fn zero() -> u32 {
    return u.z.x;
}

const PI: f32 = 3.1415927;
const TAU: f32 = 6.2831855;
const HALF_PI: f32 = 1.5707964;

fn hash(x0: u32) -> u32 {
    var x = x0;
    x ^= x >> 16u;
    x *= 0x7feb352du;
    x ^= x >> 15u;
    x *= 0x846ca68bu;
    return x ^ (x >> 16u);
}

fn hash3(a: u32, b: u32, c: u32) -> u32 {
    return hash(a ^ hash(b ^ hash(c)));
}

fn rnd(h: u32) -> f32 {
    return mul(f32(h >> 8u), 1.0 / 16777216.0);
}

fn q8(v: f32) -> u32 {
    return u32(floor(add(clamp(v, 0.0, 255.0), 0.5)));
}

fn mixf(a: f32, b: f32, t: f32) -> f32 {
    return add(a, mul(sub(b, a), t));
}

fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return select(0.0, 1.0, x >= e0);
    }
    let t = clamp(div(sub(x, e0), sub(e1, e0)), 0.0, 1.0);
    return mul(mul(t, t), sub(3.0, mul(2.0, t)));
}

fn patan2(y: f32, x: f32) -> f32 {
    let ax = abs(x);
    let ay = abs(y);
    let mx = max(ax, ay);
    if mx == 0.0 {
        return 0.0;
    }
    let a = div(min(ax, ay), mx);
    let s = mul(a, a);
    let p5 = add(0.05265332, mul(s, -0.0117212));
    let p4 = add(-0.11643287, mul(s, p5));
    let p3 = add(0.19354346, mul(s, p4));
    let p2 = add(-0.33262347, mul(s, p3));
    var r = mul(a, add(0.99997726, mul(s, p2)));
    if ay > ax {
        r = sub(HALF_PI, r);
    }
    if x < 0.0 {
        r = sub(PI, r);
    }
    return select(r, -r, y < 0.0);
}

fn pacos(x: f32) -> f32 {
    let a = min(abs(x), 1.0);
    let poly = add(1.5707288, mul(a, add(-0.2121144, mul(a, add(0.074261, mul(a, -0.0187293))))));
    let r = mul(root(sub(1.0, a)), poly);
    return select(r, sub(PI, r), x < 0.0);
}

fn pasin(x: f32) -> f32 {
    return sub(HALF_PI, pacos(x));
}

fn luma(c: vec3<f32>) -> f32 {
    return add(add(mul(0.2126, c.r), mul(0.7152, c.g)), mul(0.0722, c.b));
}

fn ifloor(v: f32) -> i32 {
    return i32(floor(clamp(v, -1048576.0, 1048576.0)));
}

fn get_a(x: i32, y: i32) -> vec4<f32> {
    return vec4<f32>(textureLoad(dst, vec2<i32>(x, y), 0));
}

fn get_b(x: i32, y: i32) -> vec4<f32> {
    return vec4<f32>(textureLoad(src, vec2<i32>(x, y), 0));
}

fn size_a() -> vec2<i32> {
    return vec2<i32>(textureDimensions(dst));
}

// `Img::tap` on picture A (dst) / B (src): the edge repeated (`edge`) or transparent outside.
fn tap_a(x: i32, y: i32, edge: bool) -> vec4<f32> {
    let s = size_a();
    if edge {
        return get_a(clamp(x, 0, s.x - 1), clamp(y, 0, s.y - 1));
    }
    if x < 0 || y < 0 || x >= s.x || y >= s.y {
        return vec4<f32>(0.0);
    }
    return get_a(x, y);
}

fn tap_b(x: i32, y: i32, edge: bool) -> vec4<f32> {
    let s = vec2<i32>(textureDimensions(src));
    if edge {
        return get_b(clamp(x, 0, s.x - 1), clamp(y, 0, s.y - 1));
    }
    if x < 0 || y < 0 || x >= s.x || y >= s.y {
        return vec4<f32>(0.0);
    }
    return get_b(x, y);
}

fn near_a(x: f32, y: f32, edge: bool) -> vec4<f32> {
    return tap_a(ifloor(x), ifloor(y), edge);
}

fn near_b(x: f32, y: f32, edge: bool) -> vec4<f32> {
    return tap_b(ifloor(x), ifloor(y), edge);
}

fn lerp4(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> {
    return vec4<f32>(mixf(a.x, b.x, t), mixf(a.y, b.y, t), mixf(a.z, b.z, t), mixf(a.w, b.w, t));
}

fn bil_a(x: f32, y: f32, edge: bool) -> vec4<f32> {
    let fx = sub(x, 0.5);
    let fy = sub(y, 0.5);
    let x0 = floor(fx);
    let y0 = floor(fy);
    let tx = sub(fx, x0);
    let ty = sub(fy, y0);
    let ix = ifloor(x0);
    let iy = ifloor(y0);
    let top = lerp4(tap_a(ix, iy, edge), tap_a(ix + 1, iy, edge), tx);
    let bot = lerp4(tap_a(ix, iy + 1, edge), tap_a(ix + 1, iy + 1, edge), tx);
    return lerp4(top, bot, ty);
}

fn bil_b(x: f32, y: f32, edge: bool) -> vec4<f32> {
    let fx = sub(x, 0.5);
    let fy = sub(y, 0.5);
    let x0 = floor(fx);
    let y0 = floor(fy);
    let tx = sub(fx, x0);
    let ty = sub(fy, y0);
    let ix = ifloor(x0);
    let iy = ifloor(y0);
    let top = lerp4(tap_b(ix, iy, edge), tap_b(ix + 1, iy, edge), tx);
    let bot = lerp4(tap_b(ix, iy + 1, edge), tap_b(ix + 1, iy + 1, edge), tx);
    return lerp4(top, bot, ty);
}

fn pack(c: vec4<f32>) -> vec4<u32> {
    let a = q8(c.w);
    return vec4<u32>(min(q8(c.x), a), min(q8(c.y), a), min(q8(c.z), a), a);
}

fn straight(c: vec4<f32>) -> vec3<f32> {
    if c.w <= 0.0 {
        return vec3<f32>(0.0);
    }
    return vec3<f32>(div(c.x, c.w), div(c.y, c.w), div(c.z, c.w));
}

fn premulf(c: vec3<f32>, a: f32) -> vec4<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(floor(add(mul(v.x, a), 0.5)), floor(add(mul(v.y, a), 0.5)), floor(add(mul(v.z, a), 0.5)), a);
}

fn pos_i(pos: vec4<f32>) -> vec2<i32> {
    return vec2<i32>(pos.xy);
}

fn at_a(x: i32, y: i32) -> vec4<f32> {
    return tap_a(x, y, true);
}

fn at_b(x: i32, y: i32) -> vec4<f32> {
    return tap_b(x, y, true);
}

fn fw(i: u32) -> f32 {
    return bitcast<f32>(data[i]);
}

fn add4(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(add(a.x, b.x), add(a.y, b.y), add(a.z, b.z), add(a.w, b.w));
}

fn sub4(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(sub(a.x, b.x), sub(a.y, b.y), sub(a.z, b.z), sub(a.w, b.w));
}

fn mul4(a: vec4<f32>, s: f32) -> vec4<f32> {
    return vec4<f32>(mul(a.x, s), mul(a.y, s), mul(a.z, s), mul(a.w, s));
}

fn out3(c: vec3<f32>, a: f32) -> vec4<u32> {
    return pack(premulf(c, a));
}

// Pixel coordinates and their centre.
fn px_of(pos: vec4<f32>) -> vec2<i32> {
    return vec2<i32>(i32(pos.x), i32(pos.y));
}

fn centre(v: i32) -> f32 {
    return add(f32(v), 0.5);
}
