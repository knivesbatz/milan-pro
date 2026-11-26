// R21b: the video FX, transitions and generators on the GPU (`engine/gpu/fx.rs`; math.wgsl comes first). Each pass
// draws a whole target of the picture's size from `dst` (the picture) and `src` (a second one: a blur, a mask, a
// transition's other side), replaying the CPU code's own integer and f32 maths (`engine/vfx.rs`, `transition.rs`,
// `generate.rs`) in order through math.wgsl's emulation, so a pass gives the CPU's picture, within 1 where the CPU
// uses the C library (`hypot`, `atan2`) and the GPU its own.

struct F {
    // Integers: what each pass says.
    i: vec4<i32>,
    j: vec4<i32>,
    // z.x is 0 (math.wgsl's `rd`); z.yzw and j.w: a premultiplied colour (r, g, b, a) where a pass takes one.
    z: vec4<u32>,
    f: array<vec4<f32>, 8>,
}

@group(0) @binding(0) var dst: texture_2d<u32>;
@group(0) @binding(1) var src: texture_2d<u32>;
@group(0) @binding(2) var<uniform> f: F;
// A per-channel LUT (3 x 256), a gradient's 256 colours, a .cube's entries (f32 bits), as each pass reads it.
@group(0) @binding(3) var<storage, read> data: array<u32>;

fn zero() -> u32 {
    return f.z.x;
}

fn at(q: vec2<i32>) -> vec4<u32> {
    return textureLoad(dst, q, 0);
}

// `vfx::ColorOp::pixel`: unpremultiply (only when a < 255), the matrix (0..255 space, i.x), the per-channel LUT in
// `data`, a mix (f[3].x) with the input, then premultiply again.
@fragment
fn fs_color_op(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let p = at(vec2<i32>(pos.xy));
    let a = p.a;
    if a == 0u {
        return p;
    }
    var orig = p.rgb;
    if a < 255u {
        orig = min((p.rgb * 255u + a / 2u) / a, vec3<u32>(255u));
    }
    var c = orig;
    if f.i.x != 0 {
        let v = vec3<f32>(c);
        for (var i = 0; i < 3; i++) {
            let m = f.f[i];
            let r = round_away(add(add(mul(m.x, v.x), mul(m.y, v.y)), mul(m.z, v.z)));
            c[i] = u32(clamp(r, 0.0, 255.0));
        }
    }
    c = vec3<u32>(data[c.r], data[256u + c.g], data[512u + c.b]);
    let mix = f.f[3].x;
    if mix < 1.0 {
        let o = vec3<f32>(orig);
        let n = vec3<f32>(c);
        c = vec3<u32>(u32(round_away(add(o.r, mul(sub(n.r, o.r), mix)))), u32(round_away(add(o.g, mul(sub(n.g, o.g), mix)))), u32(round_away(add(o.b, mul(sub(n.b, o.b), mix)))));
    }
    if a < 255u {
        c = vec3<u32>(m1(c.r, a), m1(c.g, a), m1(c.b, a));
    }
    return vec4<u32>(c, a);
}

fn cube_entry(base: u32, i: u32) -> vec3<f32> {
    let k = base + i * 3u;
    return vec3<f32>(bitcast<f32>(data[k]), bitcast<f32>(data[k + 1u]), bitcast<f32>(data[k + 2u]));
}

fn lerp1(a: f32, b: f32, t: f32) -> f32 {
    return add(a, mul(sub(b, a), t));
}

// `lut::Cube::apply` on one table: `base` into `data`, `n` a side, `three_d`, the domain `lo` and its span `den`.
fn cube_apply(c: vec3<f32>, base: u32, n: u32, three_d: bool, lo: vec3<f32>, den: vec3<f32>) -> vec3<f32> {
    let nf = f32(n - 1u);
    var x: vec3<f32>;
    var i: vec3<u32>;
    var t: vec3<f32>;
    for (var ch = 0; ch < 3; ch++) {
        x[ch] = mul(clamp(div(sub(c[ch], lo[ch]), den[ch]), 0.0, 1.0), nf);
        i[ch] = min(u32(floor(x[ch])), n - 2u);
        t[ch] = sub(x[ch], f32(i[ch]));
    }
    if !three_d {
        var o: vec3<f32>;
        for (var ch = 0; ch < 3; ch++) {
            o[ch] = lerp1(cube_entry(base, i[ch])[ch], cube_entry(base, i[ch] + 1u)[ch], t[ch]);
        }
        return o;
    }
    var o: vec3<f32>;
    for (var ch = 0; ch < 3; ch++) {
        var e: array<f32, 4>;
        for (var g = 0u; g < 2u; g++) {
            for (var b = 0u; b < 2u; b++) {
                let k = (i.r) + (i.g + g) * n + (i.b + b) * n * n;
                e[g + 2u * b] = lerp1(cube_entry(base, k)[ch], cube_entry(base, k + 1u)[ch], t.r);
            }
        }
        o[ch] = lerp1(lerp1(e[0], e[1], t.g), lerp1(e[2], e[3], t.g), t.b);
    }
    return o;
}

// `vfx::cube_img`: a .cube LUT on every pixel's straight colour, alpha kept. i.x: a shaper first (its side i.y, at
// j.x, domain f[0], span f[1]); the LUT: side i.z, 3D when i.w, at j.y, domain f[2], span f[3].
@fragment
fn fs_cube(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let p = at(vec2<i32>(pos.xy));
    if p.a == 0u {
        return p;
    }
    var c = unpremul(p);
    if f.i.x != 0 {
        c = cube_apply(c, u32(f.j.x), u32(f.i.y), false, f.f[0].xyz, f.f[1].xyz);
    }
    c = cube_apply(c, u32(f.j.y), u32(f.i.z), f.i.w != 0, f.f[2].xyz, f.f[3].xyz);
    return repremul(c, p.a);
}

// `vfx::lerp_px` of `dst` to `src` by f[0].x (`composite::adjust`, a soft blur's mix back).
@fragment
fn fs_lerp(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    return lerp_px(at(q), textureLoad(src, q, 0), f.f[0].x);
}

// `composite::blend_frames`: (1 - w) dst + w src per channel, w = i.x / 256.
@fragment
fn fs_resample(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let w8 = u32(f.i.x);
    return (at(q) * (256u - w8) + textureLoad(src, q, 0) * w8 + vec4<u32>(128u)) >> vec4<u32>(8u);
}

// `composite::reduce_flicker`: (above + 2 x row + below) / 4, the edge rows repeated.
@fragment
fn fs_flicker(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let h = i32(textureDimensions(dst).y);
    let u = at(vec2<i32>(q.x, max(q.y - 1, 0)));
    let d = at(vec2<i32>(q.x, min(q.y + 1, h - 1)));
    return (u + 2u * at(q) + d + vec4<u32>(2u)) >> vec4<u32>(2u);
}

// `vfx::sharpen`: dst the picture, src its blur; each channel pushed away from the blur by f[0].x where they differ
// by more than f[0].y.
@fragment
fn fs_sharpen(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let s = at(q);
    let bl = textureLoad(src, q, 0);
    let amount = f.f[0].x;
    let thr = f.f[0].y;
    var o = s;
    for (var c = 0; c < 3; c++) {
        let d = sub(f32(s[c]), f32(bl[c]));
        if abs(d) > thr {
            o[c] = u32(clamp(round_away(add(f32(s[c]), mul(amount, d))), 0.0, f32(s.a)));
        }
    }
    return o;
}

// `vfx::vignette`: f[0] = (amount, e0, e1, cx), f[1].x = cy, the colour z.yzw (premultiplied, opaque).
@fragment
fn fs_vignette(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let p = at(q);
    let cx = f.f[0].w;
    let cy = f.f[1].x;
    let dy = div(sub(add(f32(q.y), 0.5), cy), cy);
    let dx = div(sub(add(f32(q.x), 0.5), cx), cx);
    // 0 at the centre, 1 in the corners.
    let r = div(root(add(mul(dx, dx), mul(dy, dy))), 1.4142135);
    let t = mul(f.f[0].x, smoothstep_cpu(f.f[0].y, f.f[0].z, r));
    return lerp_px(p, vec4<u32>(f.z.y, f.z.z, f.z.w, u32(f.j.w)), t);
}

// `vfx::mirror`: direction i.x about the axis i.y (pixels): 0 keeps the left half, 1 the right, 2 the top, 3 the bottom.
@fragment
fn fs_mirror(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let size = vec2<i32>(textureDimensions(dst));
    let dir = f.i.x;
    let axis = f.i.y;
    let keep_low = (dir % 2) == 0;
    var s = q;
    let i = select(q.y, q.x, dir < 2);
    let m = select(i, 2 * axis - 1 - i, (i >= axis) == keep_low);
    if dir < 2 {
        s.x = m;
    } else {
        s.y = m;
    }
    if s.x < 0 || s.y < 0 || s.x >= size.x || s.y >= size.y {
        return vec4<u32>(0u);
    }
    return at(s);
}

// Coverage of a pixel centre `d` px inside an edge feathered over `fe` px (`vfx::crop`).
fn edge(d: f32, fe: f32) -> f32 {
    if fe <= 0.0 {
        return select(0.0, 1.0, d > 0.0);
    }
    return clamp(div(d, fe), 0.0, 1.0);
}

// `vfx::crop`: the edges f[0] = (l, t, r, b) in pixels, the feather f[1].x.
@fragment
fn fs_crop(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let fe = f.f[1].x;
    let yc = add(f32(q.y), 0.5);
    let xc = add(f32(q.x), 0.5);
    let cy = mul(edge(sub(yc, f.f[0].y), fe), edge(sub(f.f[0].w, yc), fe));
    let v = mul(mul(mul(cy, edge(sub(xc, f.f[0].x), fe)), edge(sub(f.f[0].z, xc), fe)), 255.0);
    return scale8(at(q), u32(round_half_up(vec4<f32>(v)).x));
}

// `milan.fx.border` (`vfx::apply`): the colour z.yzw over the pixels within f[0].x px of an edge.
@fragment
fn fs_border(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let p = at(q);
    let size = vec2<f32>(textureDimensions(dst));
    let dy = min(add(f32(q.y), 0.5), sub(sub(size.y, f32(q.y)), 0.5));
    let d = min(min(dy, add(f32(q.x), 0.5)), sub(sub(size.x, f32(q.x)), 0.5));
    if d < f.f[0].x {
        return over(p, vec4<u32>(f.z.y, f.z.z, f.z.w, u32(f.j.w)), 255u);
    }
    return p;
}

// `vfx::cbcr`: BT.601 chroma of straight RGB 0..1.
fn cbcr(c: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(add(sub(mul(-0.168736, c.r), mul(0.331264, c.g)), mul(0.5, c.b)), sub(sub(mul(0.5, c.r), mul(0.418688, c.g)), mul(0.081312, c.b)));
}

// `vfx::chroma_key`, the mask: alpha from the chroma distance to the key's f[0].xy, 0 below f[0].z, 1 above f[0].w.
@fragment
fn fs_chroma_mask(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let p = at(vec2<i32>(pos.xy));
    let c = cbcr(unpremul(p));
    let db = sub(c.x, f.f[0].x);
    let dr = sub(c.y, f.f[0].y);
    let dist = root(add(mul(db, db), mul(dr, dr)));
    let m = u32(round_half_up(vec4<f32>(mul(smoothstep_cpu(f.f[0].z, f.f[0].w, dist), 255.0))).x);
    return vec4<u32>(m);
}

// `vfx::chroma_key`, the picture through its (blurred) mask `src`: i.x shows the mask only; f[0].x the spill
// suppression (i.y the key's strongest channel).
@fragment
fn fs_chroma_apply(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let p = at(q);
    let m = textureLoad(src, q, 0).a;
    if f.i.x != 0 {
        return vec4<u32>(m, m, m, 255u);
    }
    let spill = f.f[0].x;
    if spill > 0.0 && p.a > 0u {
        let s = scale8(p, m);
        var c = unpremul(s);
        let d = f.i.y;
        var limit = 0.0;
        for (var i = 0; i < 3; i++) {
            if i != d {
                limit = max(limit, c[i]);
            }
        }
        if c[d] > limit {
            c[d] = sub(c[d], mul(clamp(spill, 0.0, 1.0), sub(c[d], limit)));
        }
        return repremul(c, s.a);
    }
    return scale8(p, m);
}

// `transition::band`: the weight of B at `s` (0..1 along the wipe) at progress f[0].x with a soft band f[0].y wide
// (f[0].z = p (1 + f) - f, as the CPU computes it).
fn band(s: f32) -> f32 {
    let fe = f.f[0].y;
    let e = f.f[0].z;
    if fe <= 0.0 {
        return select(0.0, 1.0, s < e);
    }
    return clamp(div(sub(add(e, fe), s), fe), 0.0, 1.0);
}

// `transition::add`: s + c t per channel, clamped.
fn add_px(s: vec4<u32>, c: vec4<u32>, t: f32) -> vec4<u32> {
    let v = vec4<f32>(add(f32(s.r), mul(f32(c.r), t)), add(f32(s.g), mul(f32(c.g), t)), add(f32(s.b), mul(f32(c.b), t)), add(f32(s.a), mul(f32(c.a), t)));
    return min(vec4<u32>(round_half_up(v)), vec4<u32>(255u));
}

const TR_LERP: i32 = 0;
const TR_ADDITIVE: i32 = 1;
const TR_FADE_LERP: i32 = 2;
const TR_FADE_ADD: i32 = 3;
const TR_LINEAR: i32 = 4;
const TR_IRIS: i32 = 5;
const TR_CLOCK: i32 = 6;
const TR_SHIFT: i32 = 7;

// `transition::mix` at progress f[0].x: `dst` is A, `src` is B (the zoom's sides already zoomed). i.x the kind;
// f[1] = (cx, cy, ...), f[2] the kind's own: a wipe's cos and sin and extent, an iris's shape (i.y) and its
// denominator, a push's offsets (j = oa.x, oa.y, ob.x, ob.y); the fade's colour z.yzw and its t f[0].w.
@fragment
fn fs_transition(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    let a = at(q);
    let b = textureLoad(src, q, 0);
    let p = f.f[0].x;
    let cx = f.f[1].x;
    let cy = f.f[1].y;
    let x = add(f32(q.x), 0.5);
    let y = add(f32(q.y), 0.5);
    switch f.i.x {
        case TR_ADDITIVE: {
            // A at (2 - 2p) and B at 2p, each at most 1, added.
            return add_px(add_px(vec4<u32>(0u), a, f.f[0].y), b, f.f[0].z);
        }
        case TR_FADE_LERP: { return lerp_px(a, vec4<u32>(f.z.y, f.z.z, f.z.w, u32(f.j.w)), f.f[0].w); }
        case TR_FADE_ADD: { return add_px(a, vec4<u32>(f.z.y, f.z.z, f.z.w, u32(f.j.w)), f.f[0].w); }
        case TR_LINEAR: {
            let s = add(div(add(mul(sub(x, cx), f.f[2].x), mul(sub(y, cy), f.f[2].y)), f.f[2].z), 0.5);
            return lerp_px(a, b, band(s));
        }
        case TR_IRIS: {
            let dx = abs(div(sub(x, cx), cx));
            let dy = abs(div(sub(y, cy), cy));
            var s: f32;
            switch f.i.y {
                case 1: { s = max(dx, dy); }
                case 2: { s = div(add(dx, dy), 2.0); }
                default: { s = div(root(add(mul(mul(mul(dx, dx), cx), cx), mul(mul(mul(dy, dy), cy), cy))), f.f[2].x); }
            }
            return lerp_px(a, b, band(s));
        }
        case TR_CLOCK: {
            // 0 at twelve o'clock, growing clockwise (y points down).
            var ang = atan2(sub(x, cx), sub(cy, y)) % 6.2831855;
            if ang < 0.0 {
                ang = ang + 6.2831855;
            }
            return lerp_px(a, b, band(div(ang, 6.2831855)));
        }
        case TR_SHIFT: {
            // B at whole-pixel offset (j.z, j.w) over A at (j.x, j.y); uncovered pixels are transparent.
            let size = vec2<i32>(textureDimensions(dst));
            let sb = q - f.j.zw;
            if sb.x >= 0 && sb.y >= 0 && sb.x < size.x && sb.y < size.y {
                return textureLoad(src, sb, 0);
            }
            let sa = q - f.j.xy;
            if sa.x >= 0 && sa.y >= 0 && sa.x < size.x && sa.y < size.y {
                return at(sa);
            }
            return vec4<u32>(0u);
        }
        default: { return lerp_px(a, b, p); }
    }
}

// `generate::hash3`.
fn hash3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    var h = bitcast<u32>(x) * 0x8da6b343u ^ bitcast<u32>(y) * 0xd8163841u ^ bitcast<u32>(z) * 0xcb1ab31fu ^ seed * 0x9e3779b9u;
    h ^= h >> 13u;
    h *= 0x5bd1e995u;
    h ^= h >> 15u;
    return div(f32(h & 0xffffffu), 16777215.0);
}

fn smooth_t(t: f32) -> f32 {
    return mul(mul(t, t), sub(3.0, mul(2.0, t)));
}

// `generate::value3`: value noise in 0..1, smoothly interpolated between lattice points.
fn value3(x: f32, y: f32, z: f32, seed: u32) -> f32 {
    let xi = floor(x);
    let yi = floor(y);
    let zi = floor(z);
    let fx = smooth_t(sub(x, xi));
    let fy = smooth_t(sub(y, yi));
    let fz = smooth_t(sub(z, zi));
    let ix = i32(xi);
    let iy = i32(yi);
    let iz = i32(zi);
    var planes: array<f32, 2>;
    for (var dz = 0; dz < 2; dz++) {
        let c00 = hash3(ix, iy, iz + dz, seed);
        let c10 = hash3(ix + 1, iy, iz + dz, seed);
        let c01 = hash3(ix, iy + 1, iz + dz, seed);
        let c11 = hash3(ix + 1, iy + 1, iz + dz, seed);
        planes[dz] = lerp1(lerp1(c00, c10, fx), lerp1(c01, c11, fx), fy);
    }
    return lerp1(planes[0], planes[1], fz);
}

// `generate::fbm`.
fn fbm(x: f32, y: f32, z: f32, octaves: u32, seed: u32) -> f32 {
    var sum = 0.0;
    var amp = 1.0;
    var norm = 0.0;
    var fr = 1.0;
    for (var o = 0u; o < octaves; o++) {
        sum = add(sum, mul(amp, value3(mul(x, fr), mul(y, fr), mul(z, fr), seed + o * 1013u)));
        norm = add(norm, amp);
        amp = mul(amp, 0.5);
        fr = mul(fr, 2.0);
    }
    return div(sum, norm);
}

// `vfx::premul`: a straight RGBA 0..1 colour, premultiplied.
fn premul(c: vec4<f32>) -> vec4<u32> {
    let a = u32(round_half_up(vec4<f32>(mul(clamp(c.a, 0.0, 1.0), 255.0))).x);
    return repremul(c.rgb, a);
}

const GEN_SOLID: i32 = 0;
const GEN_GRADIENT: i32 = 1;
const GEN_CHECKER: i32 = 2;
const GEN_NOISE: i32 = 3;

// `generate::render` for the ported generators (i.x). Gradient: type i.y, `data` its 256 colours, f[0] = (cos, sin,
// lo, span), f[1] = (cx, cy, radial denominator, cx max, ...), f[2].x = cy max. Checkerboard: cell i.y, colours
// z.yzw and j (premultiplied). Noise: f[0] = (cell, z, ..), i.y octaves, i.z seed, f[1] and f[2] the straight colours.
@fragment
fn fs_generate(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = vec2<i32>(pos.xy);
    switch f.i.x {
        case GEN_GRADIENT: {
            let x = f32(q.x);
            let y = f32(q.y);
            let cx = f.f[1].x;
            let cy = f.f[1].y;
            var s: f32;
            switch f.i.y {
                case 1: { s = div(root(add(mul(sub(x, cx), sub(x, cx)), mul(sub(y, cy), sub(y, cy)))), f.f[1].z); }
                case 2: { s = max(div(abs(sub(x, cx)), f.f[1].w), div(abs(sub(y, cy)), f.f[2].x)); }
                default: { s = div(sub(add(mul(x, f.f[0].x), mul(y, f.f[0].y)), f.f[0].z), f.f[0].w); }
            }
            let i = u32(round_half_up(vec4<f32>(mul(clamp(s, 0.0, 1.0), 255.0))).x);
            let c = data[i];
            return vec4<u32>(c & 255u, (c >> 8u) & 255u, (c >> 16u) & 255u, c >> 24u);
        }
        case GEN_CHECKER: {
            let cell = f.i.y;
            if ((q.x / cell + q.y / cell) % 2) == 0 {
                return vec4<u32>(f.z.y, f.z.z, f.z.w, u32(f.j.w));
            }
            return vec4<u32>(u32(f.j.x), u32(f.j.y), u32(f.j.z), u32(f.i.z));
        }
        case GEN_NOISE: {
            let cell = f.f[0].x;
            let v = fbm(div(f32(q.x), cell), div(f32(q.y), cell), f.f[0].y, u32(f.i.y), u32(f.i.z));
            let a = f.f[1];
            let b = f.f[2];
            return premul(vec4<f32>(add(a.x, mul(sub(b.x, a.x), v)), add(a.y, mul(sub(b.y, a.y), v)), add(a.z, mul(sub(b.z, a.z), v)), add(a.w, mul(sub(b.w, a.w), v))));
        }
        default: { return vec4<u32>(f.z.y, f.z.z, f.z.w, u32(f.j.w)); }
    }
}

// `composite::finish`: after the Video Output FX the output is opaque (whatever an FX made transparent shows black).
@fragment
fn fs_opaque(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let p = at(vec2<i32>(pos.xy));
    return vec4<u32>(p.rgb, 255u);
}

// `vfx::box_row`, three times along x (i.y = 0) or y (i.y = 1) at radius i.x, edges clamped: one invocation runs
// one line's sliding window as the CPU does, so the sum and its rounding are exact at any radius.
@group(0) @binding(4) var out: texture_storage_2d<rgba8uint, write>;

@compute @workgroup_size(64)
fn cs_box(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(dst));
    let along_x = f.i.y == 0;
    let line = i32(id.x);
    let lines = select(size.x, size.y, along_x);
    if line >= lines {
        return;
    }
    let n = select(size.y, size.x, along_x);
    let r = f.i.x;
    let d = u32(2 * r + 1);
    var sum = vec4<u32>(0u);
    for (var i = -r; i <= r; i++) {
        sum += tap(line, clamp(i, 0, n - 1), along_x);
    }
    for (var x = 0; x < n; x++) {
        let o = (sum + vec4<u32>(d / 2u)) / vec4<u32>(d);
        if along_x {
            textureStore(out, vec2<i32>(x, line), o);
        } else {
            textureStore(out, vec2<i32>(line, x), o);
        }
        sum = sum + tap(line, clamp(x + r + 1, 0, n - 1), along_x) - tap(line, clamp(x - r, 0, n - 1), along_x);
    }
}

fn tap(line: i32, i: i32, along_x: bool) -> vec4<u32> {
    if along_x {
        return at(vec2<i32>(i, line));
    }
    return at(vec2<i32>(line, i));
}
