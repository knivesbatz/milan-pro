// MVFX optics: the twins of the kernels in `engine/mvfx/optics.rs`.

@fragment
fn fs_lens(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let cx = u.f[0].x;
    let cy = u.f[0].y;
    let amt = u.f[1].x;
    let ox = sub(centre(q.x), cx);
    let oy = sub(centre(q.y), cy);
    let dx = mul(ox, u.f[0].z);
    let dy = mul(oy, u.f[0].w);
    let r2 = add(mul(dx, dx), mul(dy, dy));
    if r2 >= 1.0 {
        return pack(at_a(q.x, q.y));
    }
    let r = max(root(r2), 0.000001);
    var rs: f32;
    if u.i.x == 0 {
        let sp = sub(1.0, root(sub(1.0, r2)));
        rs = add(r, mul(amt, sub(sp, r)));
    } else {
        let k = mul(sub(1.0, r), sub(1.0, r));
        rs = mul(r, sub(1.0, mul(mul(amt, 0.75), k)));
    }
    let sc = div(rs, r);
    return pack(bil_a(add(cx, mul(ox, sc)), add(cy, mul(oy, sc)), true));
}

@fragment
fn fs_radial_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let cx = u.f[0].x;
    let cy = u.f[0].y;
    let s = u.f[0].z;
    let px = centre(q.x);
    let py = centre(q.y);
    let ox = sub(px, cx);
    let oy = sub(py, cy);
    let dist = root(add(mul(ox, ox), mul(oy, oy)));
    var acc = vec4<f32>(0.0);
    for (var i = 0u; i < u32(u.i.y); i++) {
        let t = fw(2u * i);
        let wgt = fw(2u * i + 1u);
        var qx = px;
        var qy = py;
        if u.i.x == 1 {
            if dist > 0.0 {
                qx = sub(px, mul(mul(div(ox, dist), s), t));
                qy = sub(py, mul(mul(div(oy, dist), s), t));
            }
        } else {
            let k = sub(1.0, mul(s, t));
            qx = add(cx, mul(ox, k));
            qy = add(cy, mul(oy, k));
        }
        acc = add4(acc, mul4(bil_a(qx, qy, true), wgt));
    }
    return pack(acc);
}

@fragment
fn fs_linear_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let px = centre(q.x);
    let py = centre(q.y);
    var acc = vec4<f32>(0.0);
    for (var i = 0u; i < u32(u.i.x); i++) {
        acc = add4(acc, bil_a(add(px, fw(2u * i)), add(py, fw(2u * i + 1u)), true));
    }
    return pack(mul4(acc, u.f[0].x));
}
