// MVFX transitions: the twins of the kernels in `engine/mvfx/trans.rs` (A is `dst`, B is `src`).

fn at_ab(use_b: bool, x: i32, y: i32) -> vec4<f32> {
    if use_b {
        return at_b(x, y);
    }
    return at_a(x, y);
}

@fragment
fn fs_lerp(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    return pack(add4(p, mul4(sub4(at_b(q.x, q.y), p), u.f[0].x)));
}

@fragment
fn fs_over(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let t = at_b(q.x, q.y);
    return pack(add4(t, mul4(p, sub(1.0, div(t.w, 255.0)))));
}

@fragment
fn fs_tint(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let c = vec4<f32>(u.f[0].x, u.f[0].y, u.f[0].z, 255.0);
    return pack(add4(p, mul4(sub4(c, p), u.f[0].w)));
}

@fragment
fn fs_zoom_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let cx = u.f[0].x;
    let cy = u.f[0].y;
    let s = u.f[0].z;
    let iz = u.f[0].w;
    let ox = sub(centre(q.x), cx);
    let oy = sub(centre(q.y), cy);
    var acc = vec4<f32>(0.0);
    for (var i = 0u; i < u32(u.i.y); i++) {
        let t = fw(2u * i);
        let wgt = fw(2u * i + 1u);
        let k = mul(sub(1.0, mul(s, t)), iz);
        acc = add4(acc, mul4(bil_a(add(cx, mul(ox, k)), add(cy, mul(oy, k)), true), wgt));
    }
    return pack(acc);
}

@fragment
fn fs_whip(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let axis = u.i.x;
    let sgn = u.i.y;
    let shift = u.i.z;
    let s = size_a();
    var len = s.x;
    var start = q.x;
    if axis != 0 {
        len = s.y;
        start = q.y;
    }
    var acc = vec4<f32>(0.0);
    for (var i = 0u; i < u32(u.i.w); i++) {
        let along = start + sgn * (shift + bitcast<i32>(data[i]));
        var use_b = false;
        var at = along;
        if sgn > 0 {
            if along >= len {
                use_b = true;
                at = along - len;
            }
        } else if along < 0 {
            use_b = true;
            at = along + len;
        }
        var c: vec4<f32>;
        if axis == 0 {
            c = at_ab(use_b, at, q.y);
        } else {
            c = at_ab(use_b, q.x, at);
        }
        acc = add4(acc, c);
    }
    return pack(mul4(acc, u.f[0].x));
}

@fragment
fn fs_glitch_tr(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let base = bitcast<u32>(u.j.x);
    let bh = u.i.x;
    let bnd = u32(q.y / bh);
    let hb = hash3(base, bnd, 7u);
    let amt = u.f[0].x;
    let use_b = rnd(hash3(base, bnd, 11u)) < u.f[0].z;
    var dx = 0;
    if rnd(hb) < mul(amt, 0.7) {
        dx = ifloor(add(mul(mul(sub(rnd(hash(hb)), 0.5), amt), u.f[0].y), 0.5));
    }
    let sx = q.x + dx;
    let s = u.i.y;
    let c = at_ab(use_b, sx, q.y);
    var o = vec4<f32>(at_ab(use_b, sx + s, q.y).x, c.y, at_ab(use_b, sx - s, q.y).z, c.w);
    let hc = hash3(base, bnd, u32(max(sx, 0) / (bh * 2)) + 101u);
    if rnd(hc) < mul(amt, 0.2) {
        o = vec4<f32>(o.z, o.x, o.y, o.w);
    }
    return pack(o);
}

@fragment
fn fs_page(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let dx = u.f[0].x;
    let dy = u.f[0].y;
    let l = u.f[0].z;
    let r = u.f[0].w;
    let opacity = u.f[1].y;
    let px = centre(q.x);
    let py = centre(q.y);
    let s = add(mul(px, dx), mul(py, dy));
    let uu = sub(s, l);
    let lifted = sub(l, u.f[1].x);
    let roll = u.i.x == 1;
    var hit = false;
    var dl = 0.0;
    var back = false;
    var shade = 1.0;
    if uu >= 0.0 {
        let d = add(mul(PI, r), uu);
        if !roll && d <= lifted {
            hit = true;
            dl = d;
            back = true;
            shade = 0.9;
        }
    } else if uu > -r {
        let t = div(-uu, r);
        let t1 = pasin(t);
        let up = root(max(sub(1.0, mul(t, t)), 0.0));
        let d1 = mul(r, t1);
        let d2 = mul(r, sub(PI, t1));
        if d2 <= lifted {
            hit = true;
            dl = d2;
            back = true;
            shade = add(0.75, mul(0.25, up));
        } else if d1 <= lifted {
            hit = true;
            dl = d1;
            shade = add(0.4, mul(0.6, up));
        }
    }
    var under = at_b(q.x, q.y);
    if uu >= 0.0 {
        under = at_a(q.x, q.y);
    }
    if !hit {
        return pack(under);
    }
    let back_by = add(uu, dl);
    let mtl = bil_a(sub(px, mul(dx, back_by)), sub(py, mul(dy, back_by)), false);
    let mc = straight(mtl);
    var col: vec3<f32>;
    for (var i = 0; i < 3; i++) {
        if back {
            col[i] = mul(mixf(mc[i], u.f[2][i], 0.4), shade);
        } else {
            col[i] = mul(mc[i], shade);
        }
    }
    let top = premulf(col, mtl.w);
    let k = sub(1.0, div(mul(top.w, opacity), 255.0));
    return pack(add4(mul4(top, opacity), mul4(under, k)));
}

fn cube_face(face: i32, px: f32, py: f32) -> vec2<f32> {
    let r0 = u.f[face * 3];
    let r1 = u.f[face * 3 + 1];
    let r2 = u.f[face * 3 + 2];
    let den = add(add(mul(r1.z, px), mul(r1.w, py)), r2.x);
    if den == 0.0 {
        return vec2<f32>(-1.0);
    }
    let uu = div(add(add(mul(r0.x, px), mul(r0.y, py)), r0.z), den);
    let vv = div(add(add(mul(r0.w, px), mul(r1.x, py)), r1.y), den);
    return vec2<f32>(uu, vv);
}

@fragment
fn fs_cube(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let px = centre(q.x);
    let py = centre(q.y);
    if u.i.x != 0 {
        let uv = cube_face(0, px, py);
        if uv.x >= 0.0 && uv.x < 1.0 && uv.y >= 0.0 && uv.y < 1.0 {
            let s = size_a();
            let c = bil_a(mul(uv.x, f32(s.x)), mul(uv.y, f32(s.y)), true);
            let k = u.f[6].x;
            return pack(vec4<f32>(mul(c.x, k), mul(c.y, k), mul(c.z, k), c.w));
        }
    }
    if u.i.y != 0 {
        let uv = cube_face(1, px, py);
        if uv.x >= 0.0 && uv.x < 1.0 && uv.y >= 0.0 && uv.y < 1.0 {
            let s = vec2<i32>(textureDimensions(src));
            let c = bil_b(mul(uv.x, f32(s.x)), mul(uv.y, f32(s.y)), true);
            let k = u.f[6].y;
            return pack(vec4<f32>(mul(c.x, k), mul(c.y, k), mul(c.z, k), c.w));
        }
    }
    return vec4<u32>(0u);
}

fn band(s: f32, p: f32, f: f32) -> f32 {
    let e = sub(mul(p, add(1.0, f)), f);
    if f <= 0.0 {
        return select(0.0, 1.0, s < e);
    }
    return clamp(div(sub(add(e, f), s), f), 0.0, 1.0);
}

@fragment
fn fs_wipe(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = u.f[0].x;
    let fe = u.f[0].y;
    let border = u.f[0].z;
    let n = u.f[0].w;
    let px = centre(q.x);
    let py = centre(q.y);
    var s: f32;
    if u.i.x == 0 {
        var v = abs(mul(sub(px, u.f[2].x), u.f[2].z));
        if u.i.y != 0 {
            v = abs(mul(sub(py, u.f[2].y), u.f[2].w));
        }
        s = v;
        if u.i.z == 0 {
            s = sub(1.0, v);
        }
    } else if u.i.x == 1 {
        let t = mul(mul(add(mul(px, u.f[2].x), mul(py, u.f[2].y)), u.f[2].z), n);
        s = sub(t, floor(t));
    } else {
        let ox = sub(px, u.f[2].x);
        let oy = sub(py, u.f[2].y);
        let ang = add(mul(mul(patan2(oy, ox), 0.15915494), n), u.f[2].w);
        let fr = sub(ang, floor(ang));
        var wave = fr;
        if u.i.y == 0 {
            wave = sub(1.0, abs(sub(mul(2.0, fr), 1.0)));
        }
        let star = mixf(u.f[3].x, 1.0, wave);
        s = div(mul(root(add(mul(ox, ox), mul(oy, oy))), u.f[2].z), star);
    }
    let wt = band(s, p, fe);
    let pa = at_a(q.x, q.y);
    var o = add4(pa, mul4(sub4(at_b(q.x, q.y), pa), wt));
    if border > 0.0 {
        let e = sub(mul(p, add(1.0, fe)), fe);
        if abs(sub(s, e)) < border && p > 0.0 {
            let c = u.f[1];
            o = vec4<f32>(mul(c.x, 255.0), mul(c.y, 255.0), mul(c.z, 255.0), 255.0);
        }
    }
    return pack(o);
}

@fragment
fn fs_portals(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = u.f[0].x;
    let cell = u.f[0].y;
    let transp = u.f[0].z;
    let offset = u.f[0].w;
    let scale = u.f[1].x;
    let border = u.f[1].y;
    let base = bitcast<u32>(u.j.x);
    let px = centre(q.x);
    let py = centre(q.y);
    let ci = floor(div(px, cell));
    let cj = floor(div(py, cell));
    let ui = bitcast<u32>(ifloor(ci));
    let uj = bitcast<u32>(ifloor(cj)) * 8u;
    let delay = mul(rnd(hash3(base, ui, uj + 0u)), 0.5);
    let qq = clamp(mul(sub(p, delay), 2.0), 0.0, 1.0);
    let pa = at_a(q.x, q.y);
    if qq <= 0.0 {
        return pack(pa);
    }
    let rest = sub(1.0, qq);
    let cx = add(mul(add(ci, 0.5), cell), mul(mul(mul(sub(rnd(hash3(base, ui, uj + 1u)), 0.5), offset), cell), rest));
    let cy = add(mul(add(cj, 0.5), cell), mul(mul(mul(sub(rnd(hash3(base, ui, uj + 2u)), 0.5), offset), cell), rest));
    let hw = mul(mul(qq, cell), 0.5);
    let dx = sub(px, cx);
    let dy = sub(py, cy);
    if abs(dx) >= hw || abs(dy) >= hw {
        return pack(pa);
    }
    if border > 0.0 && (sub(hw, abs(dx)) < border || sub(hw, abs(dy)) < border) {
        let c = u.f[2];
        return pack(vec4<f32>(mul(c.x, 255.0), mul(c.y, 255.0), mul(c.z, 255.0), 255.0));
    }
    let z = add(1.0, mul(scale, rest));
    let pb = bil_b(add(cx, div(dx, z)), add(cy, div(dy, z)), true);
    let al = sub(1.0, mul(transp, rest));
    return pack(add4(pa, mul4(sub4(pb, pa), al)));
}

@fragment
fn fs_explode(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let px = centre(q.x);
    let py = centre(q.y);
    let under = at_b(q.x, q.y);
    var i = u.i.x;
    while i > 0 {
        i -= 1;
        let b = u32(i) * 12u;
        let sx = add(add(mul(fw(b), px), mul(fw(b + 1u), py)), fw(b + 4u));
        let sy = add(add(mul(fw(b + 2u), px), mul(fw(b + 3u), py)), fw(b + 5u));
        if sx >= fw(b + 7u) && sx < fw(b + 9u) && sy >= fw(b + 8u) && sy < fw(b + 10u) {
            let c = bil_a(sx, sy, true);
            let al = fw(b + 6u);
            let k = sub(1.0, div(mul(c.w, al), 255.0));
            return pack(add4(mul4(c, al), mul4(under, k)));
        }
    }
    return pack(under);
}
