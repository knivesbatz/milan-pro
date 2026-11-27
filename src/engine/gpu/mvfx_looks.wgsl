// MVFX looks: the twins of the kernels in `engine/mvfx/looks.rs`, operation for operation.

@fragment
fn fs_gradient_map(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let c = straight(p);
    let g = data[q8(mul(luma(c), 255.0))];
    let gc = vec3<f32>(div(f32(g & 255u), 255.0), div(f32((g >> 8u) & 255u), 255.0), div(f32((g >> 16u) & 255u), 255.0));
    let amt = u.f[0].x;
    var o: vec3<f32>;
    for (var i = 0; i < 3; i++) {
        if u.i.x == 1 {
            o[i] = add(c[i], mul(gc[i], amt));
        } else {
            o[i] = mixf(c[i], gc[i], amt);
        }
    }
    return out3(o, p.w);
}

@fragment
fn fs_bright(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let l = luma(straight(p));
    var v = clamp(mul(sub(l, u.f[0].x), u.f[0].y), 0.0, 1.0);
    let s = u.f[0].z;
    v = mul(v, add(sub(1.0, s), mul(s, v)));
    let t = u.f[1];
    return pack(vec4<f32>(mul(mul(p.x, v), t.x), mul(mul(p.y, v), t.y), mul(mul(p.z, v), t.z), mul(p.w, v)));
}

@fragment
fn fs_add(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    return pack(add4(at_a(q.x, q.y), mul4(at_b(q.x, q.y), u.f[0].x)));
}

@fragment
fn fs_soft_contrast(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let c = straight(p);
    let bl = straight(at_b(q.x, q.y));
    let d = u.f[0].x;
    let kc = u.f[0].y;
    let vig = u.f[0].z;
    let dx = mul(sub(centre(q.x), u.f[2].x), u.f[2].z);
    let dy = mul(sub(centre(q.y), u.f[2].y), u.f[2].w);
    let v = sub(1.0, mul(vig, sstep(0.35, 1.6, add(mul(dx, dx), mul(dy, dy)))));
    var o: vec3<f32>;
    for (var i = 0; i < 3; i++) {
        let s = sub(1.0, mul(sub(1.0, c[i]), sub(1.0, mul(bl[i], d))));
        let k = add(add(0.5, mul(sub(s, 0.5), kc)), u.f[1][i]);
        o[i] = mul(k, v);
    }
    return out3(o, p.w);
}

fn vnoise(base: u32, fx: f32, fy: f32, ch: u32) -> f32 {
    let x0 = floor(fx);
    let y0 = floor(fy);
    let tx = sub(fx, x0);
    let ty = sub(fy, y0);
    let sx = mul(mul(tx, tx), sub(3.0, mul(2.0, tx)));
    let sy = mul(mul(ty, ty), sub(3.0, mul(2.0, ty)));
    let ix = bitcast<u32>(ifloor(x0));
    let iy = bitcast<u32>(ifloor(y0));
    let h00 = rnd(hash3(base, ix, iy * 4u + ch));
    let h10 = rnd(hash3(base, ix + 1u, iy * 4u + ch));
    let h01 = rnd(hash3(base, ix, (iy + 1u) * 4u + ch));
    let h11 = rnd(hash3(base, ix + 1u, (iy + 1u) * 4u + ch));
    let top = mixf(h00, h10, sx);
    let bot = mixf(h01, h11, sx);
    return mixf(top, bot, sy);
}

@fragment
fn fs_film_grain(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let c = straight(p);
    let amt = u.f[0].x;
    let cell = u.f[0].y;
    let chroma = u.f[0].z;
    let base = bitcast<u32>(u.j.x);
    let fx = div(centre(q.x), cell);
    let fy = div(centre(q.y), cell);
    let l = luma(c);
    let wgt = add(0.25, mul(mul(3.0, l), sub(1.0, l)));
    let mono = sub(vnoise(base, fx, fy, 0u), 0.5);
    var o: vec3<f32>;
    for (var i = 0u; i < 3u; i++) {
        let n = mixf(mono, sub(vnoise(base, fx, fy, 1u + i), 0.5), chroma);
        o[i] = add(c[i], mul(mul(mul(n, amt), 0.7), wgt));
    }
    return out3(o, p.w);
}

fn noise_ch(base: u32, x: i32, y: i32, ch: u32, gauss: bool) -> f32 {
    let yy = bitcast<u32>(y) * 16u;
    let h0 = rnd(hash3(base, bitcast<u32>(x), yy + (ch * 4u + 0u)));
    if !gauss {
        return mul(sub(h0, 0.5), 2.0);
    }
    let h1 = rnd(hash3(base, bitcast<u32>(x), yy + (ch * 4u + 1u)));
    let h2 = rnd(hash3(base, bitcast<u32>(x), yy + (ch * 4u + 2u)));
    let h3 = rnd(hash3(base, bitcast<u32>(x), yy + (ch * 4u + 3u)));
    return mul(sub(add(add(add(h0, h1), h2), h3), 2.0), 0.8660254);
}

@fragment
fn fs_add_noise(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let c = straight(p);
    let base = bitcast<u32>(u.j.x);
    let gauss = u.i.y == 1;
    let mono = noise_ch(base, q.x, q.y, 0u, gauss);
    var o: vec3<f32>;
    for (var i = 0u; i < 3u; i++) {
        var n = mono;
        if u.i.x != 1 {
            n = noise_ch(base, q.x, q.y, 1u + i, gauss);
        }
        o[i] = add(c[i], mul(mul(n, u.f[0].x), 0.5));
    }
    return out3(o, p.w);
}

@fragment
fn fs_film_effects(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x - u.i.z, q.y - u.i.w);
    var c = straight(p);
    let l = luma(c);
    if u.j.y == 1 {
        c = vec3<f32>(l);
    }
    let grain = u.f[0].x;
    let tint = u.f[0].y;
    let gain = u.f[0].z;
    var o: vec3<f32>;
    for (var i = 0; i < 3; i++) {
        o[i] = mul(mixf(c[i], mul(l, u.f[1][i]), tint), gain);
    }
    let px = centre(q.x);
    let py = centre(q.y);
    for (var i = 0u; i < u32(u.i.y); i++) {
        let b = i * 8u;
        var hit = false;
        var tone = 0.0;
        var s = 0.0;
        if u.i.x == 1 {
            let dx = sub(px, fw(b));
            let dy = sub(py, fw(b + 1u));
            if add(mul(dx, dx), mul(dy, dy)) < fw(b + 2u) {
                hit = true;
                tone = fw(b + 3u);
                s = 0.85;
            }
        } else if u.i.x == 3 {
            let dx = sub(px, fw(b));
            let dy = sub(py, fw(b + 1u));
            let r = root(add(mul(dx, dx), mul(dy, dy)));
            if abs(sub(r, fw(b + 2u))) < fw(b + 3u) && r > 0.0 && div(add(mul(dx, fw(b + 4u)), mul(dy, fw(b + 5u))), r) > fw(b + 6u) {
                hit = true;
                tone = fw(b + 7u);
                s = 0.8;
            }
        } else if u.i.x == 5 {
            if abs(sub(px, fw(b))) < fw(b + 1u) && py >= fw(b + 2u) && py < fw(b + 3u) {
                hit = true;
                tone = fw(b + 4u);
                s = 0.6;
            }
        }
        if hit {
            o = vec3<f32>(mixf(o.x, tone, s), mixf(o.y, tone, s), mixf(o.z, tone, s));
        }
    }
    let g = mul(mul(sub(rnd(hash3(bitcast<u32>(u.j.x), bitcast<u32>(q.x), bitcast<u32>(q.y))), 0.5), grain), 0.4);
    return out3(vec3<f32>(add(o.x, g), add(o.y, g), add(o.z, g)), p.w);
}

@fragment
fn fs_light_leak(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let c = straight(p);
    let px = centre(q.x);
    let py = centre(q.y);
    var leak = vec3<f32>(0.0);
    for (var i = 0; i < 3; i++) {
        let b = u.f[i];
        let dx = sub(px, b.x);
        let dy = sub(py, b.y);
        let qq = mul(add(mul(dx, dx), mul(dy, dy)), b.z);
        var fall = 0.0;
        if qq < 1.0 {
            fall = mul(sub(1.0, qq), sub(1.0, qq));
        }
        let s = mul(fall, b.w);
        for (var ch = 0; ch < 3; ch++) {
            leak[ch] = add(leak[ch], mul(u.f[3 + i][ch], s));
        }
    }
    let k = u.f[6].x;
    var o: vec3<f32>;
    for (var i = 0; i < 3; i++) {
        o[i] = sub(1.0, mul(sub(1.0, c[i]), sub(1.0, min(mul(leak[i], k), 1.0))));
    }
    return out3(o, p.w);
}

fn burn_level(x: i32, y: i32) -> f32 {
    let px = centre(x);
    let py = centre(y);
    let uu = mul(px, u.f[2].x);
    let vv = mul(py, u.f[2].y);
    var g: f32;
    switch u.i.x {
        case 0: { g = uu; }
        case 1: { g = sub(1.0, uu); }
        case 2: { g = vv; }
        case 3: { g = sub(1.0, vv); }
        default: {
            let du = sub(uu, 0.5);
            let dv = sub(vv, 0.5);
            g = mul(root(add(mul(du, du), mul(dv, dv))), 1.6);
        }
    }
    var n0 = 0.0;
    var n1 = 0.0;
    var sc = u.f[0].y;
    var wt = 0.5;
    for (var o = 0u; o < 3u; o++) {
        n0 = add(n0, mul(vnoise(bitcast<u32>(u.j.x), mul(px, sc), mul(py, sc), o), wt));
        n1 = add(n1, mul(vnoise(bitcast<u32>(u.j.y), mul(px, sc), mul(py, sc), o), wt));
        sc = mul(sc, 2.0);
        wt = mul(wt, 0.5);
    }
    let n = mixf(n0, n1, u.f[0].z);
    return add(sub(mul(u.f[0].x, 1.4), g), mul(sub(n, 0.44), 0.7));
}

// (colour, cover) packed as vec4: rgb and cover in w.
fn burn(c: vec3<f32>, lv: f32, col: vec4<f32>) -> vec4<f32> {
    let hot = vec3<f32>(1.0, 0.97, 0.88);
    if lv > 0.25 {
        return vec4<f32>(hot, 1.0);
    }
    if lv > 0.0 {
        let t = mul(lv, 4.0);
        let tt = mul(t, t);
        return vec4<f32>(mixf(col.x, hot.x, tt), mixf(col.y, hot.y, tt), mixf(col.z, hot.z, tt), 1.0);
    }
    if lv > -0.12 {
        let t = mul(add(lv, 0.12), 8.333333);
        var o: vec3<f32>;
        for (var i = 0; i < 3; i++) {
            o[i] = mixf(c[i], add(mul(c[i], 0.25), mul(col[i], 0.3)), t);
        }
        return vec4<f32>(o, 0.0);
    }
    return vec4<f32>(c, 0.0);
}

@fragment
fn fs_film_burn(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let b = burn(straight(p), burn_level(q.x, q.y), u.f[1]);
    var al = p.w;
    if b.w > 0.0 {
        al = 255.0;
    }
    return out3(b.xyz, al);
}

@fragment
fn fs_rgb_split(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    var r: vec4<f32>;
    var b: vec4<f32>;
    let g = at_a(q.x, q.y);
    if u.i.x == 0 {
        r = at_a(q.x - u.i.y, q.y - u.i.z);
        b = at_a(q.x + u.i.y, q.y + u.i.z);
    } else {
        let cx = u.f[0].x;
        let cy = u.f[0].y;
        let dx = sub(centre(q.x), cx);
        let dy = sub(centre(q.y), cy);
        r = bil_a(add(cx, div(dx, u.f[0].z)), add(cy, div(dy, u.f[0].z)), true);
        b = bil_a(add(cx, div(dx, u.f[0].w)), add(cy, div(dy, u.f[0].w)), true);
    }
    return pack(vec4<f32>(r.x, g.y, b.z, max(max(r.w, g.w), b.w)));
}

@fragment
fn fs_glitch(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    if u.i.x == 0 {
        return pack(at_a(q.x, q.y));
    }
    let base = bitcast<u32>(u.j.x);
    let bh = u.i.y;
    let bnd = u32(q.y / bh);
    let hb = hash3(base, bnd, 7u);
    let amt = u.f[0].x;
    var dx = 0;
    if rnd(hb) < mul(amt, 0.6) {
        dx = ifloor(add(mul(mul(sub(rnd(hash(hb)), 0.5), amt), u.f[0].y), 0.5));
    }
    let sx = q.x + dx;
    let s = u.i.w;
    let m = at_a(sx, q.y);
    var c = vec4<f32>(at_a(sx + s, q.y).x, m.y, at_a(sx - s, q.y).z, m.w);
    let hc = hash3(base, bnd, u32(max(sx, 0) / (bh * 2)) + 101u);
    if rnd(hc) < mul(amt, 0.15) {
        switch (hc >> 4u) % 3u {
            case 0u: { c = vec4<f32>(c.z, c.x, c.y, c.w); }
            case 1u: { c = vec4<f32>(sub(c.w, c.x), sub(c.w, c.y), sub(c.w, c.z), c.w); }
            default: { c = vec4<f32>(mul(floor(mul(c.x, 0.015625)), 64.0), mul(floor(mul(c.y, 0.015625)), 64.0), mul(floor(mul(c.z, 0.015625)), 64.0), mul(floor(mul(c.w, 0.015625)), 64.0)); }
        }
    }
    return pack(c);
}

@fragment
fn fs_vhs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let x = q.x;
    let y = q.y;
    let base = bitcast<u32>(u.j.x);
    let colr = u.f[0].w;
    let scan = u.f[0].z;
    let noise = u.f[1].w;
    var jx = ifloor(add(mul(sub(rnd(hash3(base, bitcast<u32>(y), 1u)), 0.5), u.f[2].x), 0.5));
    let inb = abs(sub(centre(y), u.f[1].y)) < u.f[1].z;
    if inb {
        jx += ifloor(add(mul(sub(rnd(hash3(base, bitcast<u32>(y), 2u)), 0.5), u.f[2].y), 0.5));
    }
    let sx = x + jx;
    let p = at_a(sx, y);
    let l = luma(straight(p));
    let bi = i32(u.f[0].x);
    let ci = i32(u.f[0].y);
    var sum = vec4<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        sum = add4(sum, at_a(sx - ci - j * bi, y));
    }
    let ch = straight(mul4(sum, 0.25));
    let cl = luma(ch);
    let r = add(l, sub(ch.x, cl));
    let b = add(l, sub(ch.z, cl));
    let g = div(sub(sub(l, mul(0.2126, r)), mul(0.0722, b)), 0.7152);
    let sat = sub(1.0, mul(colr, 0.35));
    var o = vec3<f32>(add(mixf(l, r, sat), mul(colr, 0.04)), sub(mixf(l, g, sat), mul(colr, 0.02)), add(mixf(l, b, sat), mul(colr, 0.03)));
    let nz = mul(mul(mul(sub(rnd(hash3(base, bitcast<u32>(x), bitcast<u32>(y) ^ 0x90000000u)), 0.5), noise), 0.25), select(1.0, 4.0, inb));
    o = vec3<f32>(add(o.x, nz), add(o.y, nz), add(o.z, nz));
    if ((y / u.i.z) & 1) == 1 {
        let k = sub(1.0, mul(scan, 0.35));
        o = vec3<f32>(mul(o.x, k), mul(o.y, k), mul(o.z, k));
    }
    let cols = u.i.x;
    if cols > 0 {
        let x0 = i32(u.f[2].z);
        let y0 = i32(u.f[2].w);
        let s = u.i.y;
        if x >= x0 && y >= y0 {
            let mx = (x - x0) / s;
            let my = (y - y0) / s;
            if mx < cols && my < 7 && data[u32(my * cols + mx)] == 1u {
                o = vec3<f32>(mixf(o.x, 1.0, 0.9), mixf(o.y, 1.0, 0.9), mixf(o.z, 0.85, 0.9));
            }
        }
    }
    return out3(o, p.w);
}

@fragment
fn fs_webcam(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let b = u.i.x;
    let mb = u.i.y;
    let p = at_a(q.x / b * b + b / 2, q.y / b * b + b / 2);
    let c = straight(p);
    let qq = straight(at_a(q.x / mb * mb + mb / 2, q.y / mb * mb + mb / 2));
    let levels = u.f[0].x;
    let noise = u.f[0].y;
    let tint_amt = u.f[0].z;
    let low = u.f[0].w;
    let l = div(floor(add(mul(luma(c), levels), 0.5)), levels);
    let ql = luma(qq);
    var o = vec3<f32>(add(l, sub(qq.x, ql)), add(l, sub(qq.y, ql)), add(l, sub(qq.z, ql)));
    o = vec3<f32>(mul(o.x, sub(1.0, mul(tint_amt, 0.12))), mul(o.y, add(1.0, mul(tint_amt, 0.04))), mul(o.z, add(1.0, mul(tint_amt, 0.14))));
    let dark = sub(1.0, mul(low, 0.45));
    let nz = mul(sub(rnd(hash3(bitcast<u32>(u.j.x), bitcast<u32>(q.x / b), bitcast<u32>(q.y / b))), 0.5), add(mul(noise, 0.2), mul(low, 0.15)));
    return out3(vec3<f32>(add(mul(o.x, dark), nz), add(mul(o.y, dark), nz), add(mul(o.z, dark), nz)), p.w);
}

@fragment
fn fs_night_vision(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    var l = mul(luma(straight(p)), u.f[0].x);
    l = add(l, mul(mul(sub(rnd(hash3(bitcast<u32>(u.j.x), bitcast<u32>(q.x), bitcast<u32>(q.y))), 0.5), u.f[0].y), 0.5));
    if ((q.y / u.i.x) & 1) == 1 {
        l = mul(l, sub(1.0, mul(u.f[0].z, 0.4)));
    }
    let dx = mul(sub(centre(q.x), u.f[2].x), u.f[2].z);
    let dy = mul(sub(centre(q.y), u.f[2].y), u.f[2].w);
    l = mul(l, sub(1.0, mul(u.f[0].w, sstep(0.2, 1.4, add(mul(dx, dx), mul(dy, dy))))));
    let hot = mul(max(sub(l, 1.0), 0.0), 0.5);
    var o: vec3<f32>;
    for (var i = 0; i < 3; i++) {
        o[i] = mixf(mul(u.f[1][i], l), 1.0, hot);
    }
    return out3(o, p.w);
}

@fragment
fn fs_letterbox(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    var s = centre(q.x);
    if u.i.x == 1 {
        s = centre(q.y);
    }
    let start = u.f[0].x;
    let end = u.f[0].y;
    let fe = u.f[0].z;
    let inside = min(sub(s, start), sub(end, s));
    var bar = select(0.0, 1.0, inside < 0.0);
    if fe > 0.0 {
        bar = sub(1.0, clamp(add(div(inside, fe), 0.5), 0.0, 1.0));
    }
    let col = u.f[1];
    let ba = mul(mul(col.w, 255.0), bar);
    let keep = sub(1.0, div(ba, 255.0));
    return pack(vec4<f32>(add(mul(col.x, ba), mul(p.x, keep)), add(mul(col.y, ba), mul(p.y, keep)), add(mul(col.z, ba), mul(p.z, keep)), add(ba, mul(p.w, keep))));
}

@fragment
fn fs_tv_sim(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let x = q.x;
    let y = q.y;
    let base = bitcast<u32>(u.j.x);
    let ap = u.f[0].x;
    let inter = u.f[0].y;
    let ls = u.f[0].z;
    let phos = u.f[0].w;
    let stat = u.f[1].x;
    let gap = u.f[1].y;
    let hi = i32(u.f[1].z);
    var sy = y + u.i.y;
    if sy >= hi {
        sy -= hi;
    }
    let rolling = u.i.y > 0 && abs(sub(f32(hi - u.i.y), centre(y))) < gap;
    let jx = ifloor(add(mul(sub(rnd(hash3(base, bitcast<u32>(y), 3u)), 0.5), ls), 0.5));
    let sx = x + jx;
    let ph = u.i.w;
    let p = at_a(sx, sy);
    var c = vec3<f32>(straight(at_a(sx + ph, sy)).x, straight(p).y, straight(at_a(sx - ph, sy)).z);
    let tri = (x / u.i.x) % 3;
    for (var i = 0; i < 3; i++) {
        var wgt = sub(1.0, mul(ap, 0.5));
        if i == tri {
            wgt = add(1.0, ap);
        }
        c[i] = mul(c[i], wgt);
    }
    if ((y + u.i.z) & 1) == 1 {
        let k = sub(1.0, mul(inter, 0.35));
        c = vec3<f32>(mul(c.x, k), mul(c.y, k), mul(c.z, k));
    }
    let pk = mul(phos, 0.25);
    c = vec3<f32>(add(c.x, mul(mul(pk, c.x), c.x)), add(c.y, mul(mul(pk, c.y), c.y)), add(c.z, mul(mul(pk, c.z), c.z)));
    let nz = rnd(hash3(base, bitcast<u32>(x), bitcast<u32>(y) ^ 0x40000000u));
    let st = mul(stat, 0.6);
    c = vec3<f32>(mixf(c.x, nz, st), mixf(c.y, nz, st), mixf(c.z, nz, st));
    if rolling {
        c = vec3<f32>(0.0);
    }
    return out3(c, p.w);
}

@fragment
fn fs_newsprint(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let s = u.f[0].x;
    let off = u.f[0].y;
    let rmax = u.f[0].z;
    let px = centre(q.x);
    let py = centre(q.y);
    let mono = u.i.x == 1;
    let alpha = at_a(q.x, q.y).w;
    var o = vec3<f32>(1.0);
    var screens = 3;
    if mono {
        screens = 1;
    }
    for (var k = 0; k < screens; k++) {
        let cs = u.f[1 + k].x;
        let sn = u.f[1 + k].y;
        let kf = f32(k);
        let qx = add(px, mul(off, kf));
        let qy = sub(py, mul(off, kf));
        let uu = add(mul(qx, cs), mul(qy, sn));
        let vv = sub(mul(qy, cs), mul(qx, sn));
        let uc = mul(add(floor(div(uu, s)), 0.5), s);
        let vc = mul(add(floor(div(vv, s)), 0.5), s);
        let cx = sub(sub(mul(uc, cs), mul(vc, sn)), mul(off, kf));
        let cy = add(add(mul(uc, sn), mul(vc, cs)), mul(off, kf));
        let c = straight(near_a(cx, cy, true));
        var ink = sub(1.0, c[k]);
        if mono {
            ink = sub(1.0, luma(c));
        }
        let r = mul(rmax, root(max(ink, 0.0)));
        let du = sub(uu, uc);
        let dv = sub(vv, vc);
        let inked = select(0.0, 1.0, add(mul(du, du), mul(dv, dv)) < mul(r, r));
        if mono {
            o = vec3<f32>(sub(1.0, inked));
        } else {
            o[k] = sub(1.0, inked);
        }
    }
    return out3(o, alpha);
}

@fragment
fn fs_pixelate(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let bw = u.i.x;
    let bh = u.i.y;
    return pack(at_a(q.x / bw * bw + bw / 2, q.y / bh * bh + bh / 2));
}

@fragment
fn fs_starburst(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    var p = at_a(q.x, q.y);
    if u.i.y == 1 {
        p = vec4<f32>(0.0);
    }
    var s = vec4<f32>(0.0);
    for (var i = 0u; i < u32(u.i.x); i++) {
        let ox = bitcast<i32>(data[i * 3u]);
        let oy = bitcast<i32>(data[i * 3u + 1u]);
        let wgt = fw(i * 3u + 2u);
        s = add4(s, mul4(tap_b(q.x - ox, q.y - oy, false), wgt));
    }
    return pack(add4(p, mul4(s, u.f[0].x)));
}

@fragment
fn fs_light_rays(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let px = centre(q.x);
    let py = centre(q.y);
    let dx = sub(u.f[0].x, px);
    let dy = sub(u.f[0].y, py);
    var s = vec4<f32>(0.0);
    let n = u.i.x;
    for (var i = 0; i < n; i++) {
        let t = mul(f32(i), u.f[0].z);
        let c = near_b(add(px, mul(dx, t)), add(py, mul(dy, t)), false);
        let wgt = sub(1.0, div(f32(i), f32(n)));
        s = add4(s, mul4(c, wgt));
    }
    return pack(add4(p, mul4(s, u.f[0].w)));
}
