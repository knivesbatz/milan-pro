// MVFX temporal effects: the twins of the kernels in `engine/mvfx/temporal.rs`.

@fragment
fn fs_strobe(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    switch u.i.x {
        case 1: { return pack(u.f[0]); }
        case 2: { return vec4<u32>(0u); }
        case 3: { return pack(vec4<f32>(sub(p.w, p.x), sub(p.w, p.y), sub(p.w, p.z), p.w)); }
        default: { return pack(p); }
    }
}

@fragment
fn fs_echo(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let p = at_a(q.x, q.y);
    let b = at_b(q.x, q.y);
    let t = u.f[0].x;
    let wgt = u.f[0].y;
    var o: vec4<f32>;
    for (var i = 0; i < 4; i++) {
        switch u.i.x {
            case 1: { o[i] = max(p[i], mul(b[i], wgt)); }
            case 2: { o[i] = add(p[i], mul(mul(b[i], wgt), 0.5)); }
            default: { o[i] = add(p[i], mul(sub(b[i], p[i]), t)); }
        }
    }
    return pack(o);
}

@fragment
fn fs_copy(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    return pack(at_a(q.x, q.y));
}

@fragment
fn fs_mosh(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<u32> {
    let q = px_of(pos);
    let bs = u.i.x;
    let i = min(q.x / bs, u.i.y - 1);
    let j = min(q.y / bs, u.i.z - 1);
    let at = u32((j * u.i.y + i) * 2);
    let vx = bitcast<i32>(data[at]);
    let vy = bitcast<i32>(data[at + 1u]);
    let p = at_a(q.x - vx, q.y - vy);
    let b = at_b(q.x, q.y);
    return pack(add4(p, mul4(sub4(b, p), u.f[0].x)));
}
