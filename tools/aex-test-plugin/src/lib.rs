//! Milan Pro's AE-format test plug-in, written against Milan's own declarations (`abi.rs`, shared with the host by
//! `#[path]`), with no Adobe SDK material: so it proves the host self-consistent, not conformant. One file, one PiPL
//! per effect (`effects.rs`, written into a `.res` by `build.rs`). The effects:
//! Invert (iterate suite), Gain (a float slider, the utils iterate callback), Crash (in render), Crash Setup (in
//! global setup), Hang (render never returns), Params (every parameter type, a hidden and a greyed one), Marker (paints
//! known corners: row order and channel order), Counter (sequence data holding a pointer: setup, flatten, resetup),
//! Callbacks (every tier-a callback and suite, checked), Previous Frame (checks out its input one frame earlier),
//! Supervisor (a supervised checkbox greys and sets its sibling), Layer (a layer parameter), Echo (ten checkouts).
#![allow(clippy::missing_safety_doc, dead_code)]
#[path = "../../../src/bin/milan-plugin-host/aex/abi.rs"]
mod abi;
mod effects;

use abi::*;
use std::ffi::{CStr, c_char, c_void};

// ---------------------------------------------------------------- helpers

unsafe fn suite<T>(ind: &InData, s: (&CStr, i32)) -> Option<&'static T> {
    let b = unsafe { ind.pica_basic.as_ref() }?;
    let mut p: *const c_void = std::ptr::null();
    let e = unsafe { (b.acquire_suite)(s.0.as_ptr(), s.1, &mut p) };
    if e != 0 || p.is_null() { None } else { Some(unsafe { &*(p as *const T) }) }
}

fn name(d: &mut ParamDef, n: &str) {
    for (k, b) in n.bytes().take(31).enumerate() {
        d.name[k] = b as c_char;
    }
}

unsafe fn add(ind: &InData, mut d: ParamDef) -> Err {
    match ind.inter.add_param {
        Some(f) => unsafe { f(ind.effect_ref, -1, &mut d) },
        None => err::INVALID_CALLBACK,
    }
}

fn float(id: i32, n: &str, dflt: f32, lo: f32, hi: f32, flags: i32) -> ParamDef {
    let mut d = ParamDef { param_type: param::FLOAT_SLIDER, uu: id, flags, ..Default::default() };
    name(&mut d, n);
    d.u.fs_d = FloatSliderDef { value: dflt as f64, dephault: dflt, valid_min: lo, valid_max: hi, slider_min: lo, slider_max: hi, precision: 2, ..Default::default() };
    d
}

fn checkbox(id: i32, n: &str, on: bool, flags: i32) -> ParamDef {
    let mut d = ParamDef { param_type: param::CHECKBOX, uu: id, flags, ..Default::default() };
    name(&mut d, n);
    d.u.bd = CheckBoxDef { value: on as i32, dephault: on as u8, reserved: 0, reserved1: 0, name: c"On".as_ptr() };
    d
}

fn message(out: &mut OutData, s: &str) {
    for (k, b) in s.bytes().take(255).enumerate() {
        out.return_msg[k] = b as c_char;
    }
}

unsafe fn px(w: &World, x: i32, y: i32) -> &mut Pixel {
    unsafe { &mut *(w.data.add(y as usize * w.rowbytes as usize + x as usize * 4) as *mut Pixel) }
}

unsafe fn copy_world(src: &World, dst: &World) {
    for y in 0..dst.height.min(src.height) {
        for x in 0..dst.width.min(src.width) {
            unsafe { *px(dst, x, y) = *px(src, x, y) };
        }
    }
}

/// The parameters' setup that every effect without parameters of its own does.
unsafe fn no_params(out: &mut OutData) -> Err {
    out.num_params = 1;
    err::NONE
}

type Fx = unsafe fn(i32, &mut InData, &mut OutData, *mut *mut ParamDef, *mut World, *mut c_void) -> Err;

/// The common commands, then the effect's own.
unsafe fn run(c: i32, ind: *mut InData, out: *mut OutData, params: *mut *mut ParamDef, output: *mut World, extra: *mut c_void, fx: Fx, which: usize) -> Err {
    let (Some(ind), Some(out)) = (unsafe { ind.as_mut() }, unsafe { out.as_mut() }) else { return err::BAD_CALLBACK_PARAM };
    if c == cmd::GLOBAL_SETUP {
        // The host is honest about itself: a third-party host, no serial number.
        if ind.appl_id != APPL_ID_PRMR || ind.serial_num != 0 {
            message(out, "unexpected host identity");
            return err::BAD_CALLBACK_PARAM;
        }
        out.my_version = 1 << 19;
        // The flags its PiPL declares, as a plug-in must set them here.
        out.out_flags = effects::ALL[which].out_flags;
        out.out_flags2 = effects::ALL[which].out_flags2;
    }
    unsafe { fx(c, ind, out, params, output, extra) }
}

macro_rules! entry {
    ($name:ident, $fx:ident, $which:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(c: i32, ind: *mut InData, out: *mut OutData, params: *mut *mut ParamDef, output: *mut World, extra: *mut c_void) -> Err {
            unsafe { run(c, ind, out, params, output, extra, $fx, $which) }
        }
    };
}

unsafe fn param(params: *mut *mut ParamDef, i: usize) -> &'static mut ParamDef {
    unsafe { &mut **params.add(i) }
}

// ---------------------------------------------------------------- effects

unsafe extern "C" fn invert_px(_r: *mut c_void, _x: i32, _y: i32, i: *mut Pixel, o: *mut Pixel) -> Err {
    unsafe {
        let p = *i;
        *o = Pixel { alpha: p.alpha, red: p.alpha - p.red.min(p.alpha), green: p.alpha - p.green.min(p.alpha), blue: p.alpha - p.blue.min(p.alpha) };
    }
    err::NONE
}

unsafe fn invert(c: i32, ind: &mut InData, out: &mut OutData, params: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    match c {
        cmd::PARAMS_SETUP => unsafe { no_params(out) },
        cmd::RENDER => unsafe {
            let Some(it) = suite::<Iterate8Suite1>(ind, suite::ITERATE8) else { return err::INVALID_CALLBACK };
            (it.iterate)(ind, 0, 1, &mut param(params, 0).u.ld, std::ptr::null(), std::ptr::null_mut(), Some(invert_px), output)
        },
        _ => err::NONE,
    }
}
entry!(MilanTestInvert, invert, effects::INVERT);

unsafe extern "C" fn gain_px(r: *mut c_void, _x: i32, _y: i32, i: *mut Pixel, o: *mut Pixel) -> Err {
    unsafe {
        let g = *(r as *const f64);
        let p = *i;
        let m = |c: u8| ((c as f64 * g).round().clamp(0.0, p.alpha as f64)) as u8;
        *o = Pixel { alpha: p.alpha, red: m(p.red), green: m(p.green), blue: m(p.blue) };
    }
    err::NONE
}

unsafe fn gain(c: i32, ind: &mut InData, out: &mut OutData, params: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    match c {
        cmd::PARAMS_SETUP => unsafe {
            out.num_params = 2;
            add(ind, float(1, "Gain", 1.0, 0.0, 4.0, 0))
        },
        cmd::RENDER => unsafe {
            let mut g = param(params, 1).u.fs_d.value;
            let u = &*ind.utils;
            (u.iterate)(ind, 0, 1, &mut param(params, 0).u.ld, std::ptr::null(), &mut g as *mut f64 as *mut c_void, Some(gain_px), output)
        },
        _ => err::NONE,
    }
}
entry!(MilanTestGain, gain, effects::GAIN);

unsafe fn crash(c: i32, _i: &mut InData, out: &mut OutData, _p: *mut *mut ParamDef, _o: *mut World, _e: *mut c_void) -> Err {
    match c {
        cmd::PARAMS_SETUP => unsafe { no_params(out) },
        // An access violation, as a broken plug-in would make.
        cmd::RENDER => unsafe {
            std::ptr::write_volatile(std::ptr::null_mut::<u32>(), 1);
            err::NONE
        },
        _ => err::NONE,
    }
}
entry!(MilanTestCrash, crash, effects::CRASH);

unsafe fn crash_setup(c: i32, _i: &mut InData, _o: &mut OutData, _p: *mut *mut ParamDef, _w: *mut World, _e: *mut c_void) -> Err {
    if c == cmd::GLOBAL_SETUP {
        unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u32>(), 1) };
    }
    err::NONE
}
entry!(MilanTestCrashSetup, crash_setup, effects::CRASH_SETUP);

unsafe fn hang(c: i32, _i: &mut InData, out: &mut OutData, _p: *mut *mut ParamDef, _o: *mut World, _e: *mut c_void) -> Err {
    match c {
        cmd::PARAMS_SETUP => unsafe { no_params(out) },
        cmd::RENDER => loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        _ => err::NONE,
    }
}
entry!(MilanTestHang, hang, effects::HANG);

unsafe fn params_fx(c: i32, ind: &mut InData, out: &mut OutData, params: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    match c {
        // Asks for a suite Milan does not have (and copes): the host notes it for the Plug-In Manager.
        cmd::GLOBAL_SETUP => unsafe {
            let _ = suite::<HandleSuite1>(ind, (c"PF Made Up Suite", 2));
            err::NONE
        },
        cmd::PARAMS_SETUP => unsafe {
            let mut defs = Vec::new();
            let mut g = ParamDef { param_type: param::GROUP_START, uu: 10, ..Default::default() };
            name(&mut g, "Levels");
            defs.push(g);
            let mut f = float(1, "Amount", 50.0, 0.0, 100.0, 0);
            f.u.fs_d.display_flags = DISPLAY_PERCENT;
            defs.push(f);
            let mut s = ParamDef { param_type: param::SLIDER, uu: 2, ..Default::default() };
            name(&mut s, "Count");
            s.u.sd = SliderDef { value: 3, dephault: 3, valid_min: 0, valid_max: 10, slider_min: 0, slider_max: 10, ..Default::default() };
            defs.push(s);
            let mut e = ParamDef { param_type: param::GROUP_END, uu: 11, ..Default::default() };
            name(&mut e, "");
            defs.push(e);
            let mut x = ParamDef { param_type: param::FIX_SLIDER, uu: 3, ..Default::default() };
            name(&mut x, "Fixed");
            x.u.fd = FixedSliderDef { value: fixed(1.5), dephault: fixed(1.5), valid_min: 0, valid_max: fixed(8.0), slider_min: 0, slider_max: fixed(4.0), ..Default::default() };
            defs.push(x);
            let mut a = ParamDef { param_type: param::ANGLE, uu: 4, ..Default::default() };
            name(&mut a, "Angle");
            a.u.ad = AngleDef { value: fixed(30.0), dephault: fixed(30.0), ..Default::default() };
            defs.push(a);
            defs.push(checkbox(5, "Enabled", true, 0));
            let mut col = ParamDef { param_type: param::COLOR, uu: 6, ..Default::default() };
            name(&mut col, "Tint");
            let p = Pixel { alpha: 255, red: 255, green: 128, blue: 0 };
            col.u.cd = ColorDef { value: p, dephault: p };
            defs.push(col);
            let mut pt = ParamDef { param_type: param::POINT, uu: 7, ..Default::default() };
            name(&mut pt, "Center");
            pt.u.td = PointDef { x_dephault: fixed(50.0), y_dephault: fixed(25.0), ..Default::default() };
            defs.push(pt);
            let mut pop = ParamDef { param_type: param::POPUP, uu: 8, ..Default::default() };
            name(&mut pop, "Mode");
            pop.u.pd = PopupDef { value: 4, num_choices: 4, dephault: 4, names: c"One|Two|(-|Three".as_ptr() };
            defs.push(pop);
            let mut b = ParamDef { param_type: param::BUTTON, uu: 9, ..Default::default() };
            name(&mut b, "Reset");
            b.u.button_d = ButtonDef { value: 0, names: c"Reset".as_ptr() };
            defs.push(b);
            let mut l = ParamDef { param_type: param::LAYER, uu: 12, ..Default::default() };
            name(&mut l, "Matte");
            defs.push(l);
            let mut hidden = float(13, "Hidden", 2.0, 0.0, 4.0, 0);
            hidden.ui_flags = pui::INVISIBLE;
            defs.push(hidden);
            let mut grey = float(14, "Greyed", 1.0, 0.0, 4.0, 0);
            grey.ui_flags = pui::DISABLED;
            defs.push(grey);
            out.num_params = defs.len() as i32 + 1;
            for d in defs {
                let e = add(ind, d);
                if e != err::NONE {
                    return e;
                }
            }
            err::NONE
        },
        cmd::RENDER => unsafe { ((*ind.utils).copy)(ind.effect_ref, &mut param(params, 0).u.ld, output, std::ptr::null(), std::ptr::null()) },
        _ => err::NONE,
    }
}
entry!(MilanTestParams, params_fx, effects::PARAMS);

unsafe fn marker(c: i32, _i: &mut InData, out: &mut OutData, _p: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    match c {
        cmd::PARAMS_SETUP => unsafe { no_params(out) },
        cmd::RENDER => unsafe {
            let w = &*output;
            // Row 0 is the top row: red at its left, green at its right; blue at the bottom left.
            *px(w, 0, 0) = Pixel { alpha: 255, red: 255, green: 0, blue: 0 };
            *px(w, w.width - 1, 0) = Pixel { alpha: 255, red: 0, green: 255, blue: 0 };
            *px(w, 0, w.height - 1) = Pixel { alpha: 255, red: 0, green: 0, blue: 255 };
            err::NONE
        },
        _ => err::NONE,
    }
}
entry!(MilanTestMarker, marker, effects::MARKER);

/// The counter's live sequence data: it holds a pointer, so it must be flattened to be saved.
#[repr(C)]
struct Live {
    magic: u32,
    count: u32,
    label: *mut String,
}

#[repr(C)]
struct Flat {
    magic: u32,
    count: u32,
    len: u32,
    text: [u8; 16],
}

const LIVE: u32 = 0x4C495645;
const FLAT: u32 = 0x464C4154;

unsafe fn handle_suite(ind: &InData) -> &'static HandleSuite1 {
    unsafe { suite::<HandleSuite1>(ind, suite::HANDLE).expect("the handle suite") }
}

unsafe fn new_live(ind: &InData, count: u32) -> Handle {
    unsafe {
        let hs = handle_suite(ind);
        let h = (hs.new_handle)(std::mem::size_of::<Live>() as u64);
        let p = (hs.lock_handle)(h) as *mut Live;
        *p = Live { magic: LIVE, count, label: Box::into_raw(Box::new("count".to_string())) };
        h
    }
}

unsafe fn counter(c: i32, ind: &mut InData, out: &mut OutData, _p: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    unsafe {
        let hs = handle_suite(ind);
        match c {
            cmd::PARAMS_SETUP => no_params(out),
            cmd::SEQUENCE_SETUP => {
                out.sequence_data = new_live(ind, 0);
                err::NONE
            }
            cmd::SEQUENCE_FLATTEN => {
                let h = ind.sequence_data;
                let live = &*((hs.lock_handle)(h) as *const Live);
                if live.magic != LIVE {
                    return err::INTERNAL_STRUCT_DAMAGED;
                }
                let f = (hs.new_handle)(std::mem::size_of::<Flat>() as u64);
                let label = &*live.label;
                let mut text = [0u8; 16];
                text[..label.len()].copy_from_slice(label.as_bytes());
                *((hs.lock_handle)(f) as *mut Flat) = Flat { magic: FLAT, count: live.count, len: label.len() as u32, text };
                drop(Box::from_raw(live.label));
                (hs.dispose_handle)(h);
                out.sequence_data = f;
                err::NONE
            }
            cmd::SEQUENCE_RESETUP => {
                let f = ind.sequence_data;
                if f.is_null() || (hs.get_handle_size)(f) < std::mem::size_of::<Flat>() as u64 {
                    return err::INTERNAL_STRUCT_DAMAGED;
                }
                let flat = &*((hs.lock_handle)(f) as *const Flat);
                if flat.magic != FLAT || &flat.text[..flat.len as usize] != b"count" {
                    message(out, "the flat sequence data came back damaged");
                    return err::INTERNAL_STRUCT_DAMAGED;
                }
                out.sequence_data = new_live(ind, flat.count);
                (hs.dispose_handle)(f);
                err::NONE
            }
            cmd::SEQUENCE_SETDOWN => {
                let h = ind.sequence_data;
                if !h.is_null() {
                    let live = &*((hs.lock_handle)(h) as *const Live);
                    if live.magic == LIVE {
                        drop(Box::from_raw(live.label));
                    }
                    (hs.dispose_handle)(h);
                }
                out.sequence_data = std::ptr::null_mut();
                err::NONE
            }
            cmd::RENDER => {
                let live = &mut *((hs.lock_handle)(ind.sequence_data) as *mut Live);
                if live.magic != LIVE {
                    return err::INTERNAL_STRUCT_DAMAGED;
                }
                live.count += 1;
                // The count shows as the red channel of an opaque frame.
                let p = Pixel { alpha: 255, red: live.count.min(255) as u8, green: 0, blue: 0 };
                ((*ind.utils).fill)(ind.effect_ref, &p, std::ptr::null(), output)
            }
            _ => err::NONE,
        }
    }
}
entry!(MilanTestCounter, counter, effects::COUNTER);

/// What Callbacks does to a pixel of its input (the test's Rust reference does the same).
unsafe extern "C" fn rotate_px(_r: *mut c_void, _x: i32, _y: i32, i: *mut Pixel, o: *mut Pixel) -> Err {
    unsafe {
        let p = *i;
        *o = Pixel { alpha: p.alpha, red: p.green, green: p.blue, blue: p.red };
    }
    err::NONE
}

unsafe extern "C" fn count_generic(r: *mut c_void, _t: i32, i: i32, _n: i32) -> Err {
    unsafe { *(r as *mut i32) += i };
    err::NONE
}

/// Every tier-a callback and suite in turn; any wrong answer fails the render with a message saying which.
unsafe fn callbacks(c: i32, ind: &mut InData, out: &mut OutData, params: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    if c == cmd::PARAMS_SETUP {
        out.num_params = 2;
        let mut col = ParamDef { param_type: param::COLOR, uu: 1, ..Default::default() };
        name(&mut col, "Fill");
        let p = Pixel { alpha: 255, red: 10, green: 20, blue: 30 };
        col.u.cd = ColorDef { value: p, dephault: p };
        return unsafe { add(ind, col) };
    }
    if c != cmd::RENDER {
        return err::NONE;
    }
    unsafe {
        let fail = |out: &mut OutData, what: &str| {
            message(out, what);
            err::BAD_CALLBACK_PARAM
        };
        let u = &*ind.utils;
        let input = &mut param(params, 0).u.ld;
        let (w, h) = (input.width, input.height);
        // 1. A world of our own, the input copied into it.
        let mut tmp = World::default();
        if (u.new_world)(ind.effect_ref, w, h, 0, &mut tmp) != err::NONE || tmp.data.is_null() {
            return fail(out, "new_world");
        }
        (u.copy)(ind.effect_ref, input, &mut tmp, std::ptr::null(), std::ptr::null());
        // 2. The top-left quarter filled with the colour parameter (its float value checked through its suite).
        let Some(fm) = suite::<FillMatteSuite2>(ind, suite::FILL_MATTE) else { return fail(out, "fill matte suite") };
        let Some(cs) = suite::<ColorParamSuite1>(ind, suite::COLOR_PARAM) else { return fail(out, "color param suite") };
        let mut fc = PixelFloat::default();
        (cs.get_float_color)(ind.effect_ref, param(params, 1), &mut fc);
        if (fc.red - 10.0 / 255.0).abs() > 1e-6 {
            return fail(out, "float colour");
        }
        let col = param(params, 1).u.cd.value;
        (fm.fill)(ind.effect_ref, &col, &Rect { left: 0, top: 0, right: w / 2, bottom: h / 2 }, &mut tmp);
        // 3. The channels rotated into the output with the iterate suite.
        let Some(it) = suite::<Iterate8Suite1>(ind, suite::ITERATE8) else { return fail(out, "iterate suite") };
        (it.iterate)(ind, 0, h, &mut tmp, std::ptr::null(), std::ptr::null_mut(), Some(rotate_px), output);
        // 4. Half of the input mixed back in, with the world transform suite.
        let Some(wt) = suite::<WorldTransformSuite1>(ind, suite::WORLD_TRANSFORM) else { return fail(out, "world transform suite") };
        (wt.blend)(ind.effect_ref, output, input, fixed(0.5), output);
        // 5. Handles keep their bytes through a resize.
        let Some(hs) = suite::<HandleSuite1>(ind, suite::HANDLE) else { return fail(out, "handle suite") };
        let mut hd = (hs.new_handle)(4);
        *((hs.lock_handle)(hd) as *mut u32) = 0xC0FFEE;
        if (hs.resize_handle)(64, &mut hd) != err::NONE || *((hs.lock_handle)(hd) as *const u32) != 0xC0FFEE || (hs.get_handle_size)(hd) != 64 {
            return fail(out, "handle resize");
        }
        (hs.dispose_handle)(hd);
        let uh = (u.host_new_handle)(8);
        if (u.host_get_handle_size)(uh) != 8 {
            return fail(out, "utils handles");
        }
        (u.host_dispose_handle)(uh);
        // 6. The C library: maths and sprintf, through the suite and through utils.
        let Some(ansi) = suite::<AnsiSuite1>(ind, suite::ANSI) else { return fail(out, "ANSI suite") };
        let mut buf = [0 as c_char; 32];
        (ansi.sprintf)(buf.as_mut_ptr(), c"%d/%.1f".as_ptr(), 7i32, (ansi.sqrt)(16.0));
        if CStr::from_ptr(buf.as_ptr()).to_bytes() != b"7/4.0" || (u.ansi.pow)(2.0, 10.0) != 1024.0 {
            return fail(out, "ANSI");
        }
        // 7. The world suite: a world of its pixel format, made and disposed of.
        let Some(ws) = suite::<WorldSuite2>(ind, suite::WORLD) else { return fail(out, "world suite") };
        let mut w2 = World::default();
        let mut fmt = 0;
        if (ws.new_world)(ind.effect_ref, 4, 4, 1, PIXEL_FORMAT_ARGB32, &mut w2) != err::NONE || (ws.get_pixel_format)(&w2, &mut fmt) != err::NONE || fmt != PIXEL_FORMAT_ARGB32 {
            return fail(out, "world suite");
        }
        (ws.dispose_world)(ind.effect_ref, &mut w2);
        // 8. iterate_generic visits every index; abort and progress answer.
        let mut sum = 0;
        (u.iterate_generic)(5, &mut sum as *mut i32 as *mut c_void, Some(count_generic));
        if sum != 10 {
            return fail(out, "iterate_generic");
        }
        if (ind.inter.abort.expect("abort"))(ind.effect_ref) != err::NONE || (ind.inter.progress.expect("progress"))(ind.effect_ref, 1, 2) != err::NONE {
            return fail(out, "abort / progress");
        }
        // 9. A suite that does not exist is refused, not crashed.
        if suite::<HandleSuite1>(ind, (c"PF No Such Suite", 1)).is_some() {
            return fail(out, "a made-up suite was served");
        }
        (u.dispose_world)(ind.effect_ref, &mut tmp);
        err::NONE
    }
}
entry!(MilanTestCallbacks, callbacks, effects::CALLBACKS);

/// Check out layer `index` at `frames` frames from now and copy it to the output.
unsafe fn checkout_copy(ind: &mut InData, index: i32, frames: i32, output: *mut World) -> Err {
    unsafe {
        let mut d = ParamDef::default();
        let t = ind.current_time + frames * ind.time_step;
        let e = (ind.inter.checkout_param.expect("checkout"))(ind.effect_ref, index, t, ind.time_step, ind.time_scale, &mut d);
        if e != err::NONE {
            return e;
        }
        copy_world(&d.u.ld, &*output);
        (ind.inter.checkin_param.expect("checkin"))(ind.effect_ref, &mut d)
    }
}

unsafe fn previous(c: i32, ind: &mut InData, out: &mut OutData, _p: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    match c {
        cmd::PARAMS_SETUP => unsafe { no_params(out) },
        cmd::RENDER => unsafe { checkout_copy(ind, 0, -1, output) },
        _ => err::NONE,
    }
}
entry!(MilanTestPrevious, previous, effects::PREVIOUS);

unsafe fn supervisor(c: i32, ind: &mut InData, out: &mut OutData, params: *mut *mut ParamDef, output: *mut World, extra: *mut c_void) -> Err {
    unsafe {
        match c {
            cmd::PARAMS_SETUP => {
                out.num_params = 3;
                let e = add(ind, checkbox(1, "Use Amount", true, param_flag::SUPERVISE));
                if e != err::NONE {
                    return e;
                }
                add(ind, float(2, "Amount", 1.0, 0.0, 4.0, 0))
            }
            cmd::USER_CHANGED_PARAM => {
                // Turning the switch off sets the amount to 0.25 (an edit Milan writes to the project).
                let ex = &*(extra as *const UserChangedParamExtra);
                if ex.param_index == 1 && param(params, 1).u.bd.value == 0 {
                    let a = param(params, 2);
                    a.u.fs_d.value = 0.25;
                    a.uu = CHANGED_VALUE;
                }
                err::NONE
            }
            cmd::UPDATE_PARAMS_UI => {
                let Some(pu) = suite::<ParamUtilsSuite3>(ind, suite::PARAM_UTILS) else { return err::INVALID_CALLBACK };
                let mut copy = *param(params, 2);
                if param(params, 1).u.bd.value == 0 {
                    copy.ui_flags |= pui::DISABLED;
                } else {
                    copy.ui_flags &= !pui::DISABLED;
                }
                (pu.update_param_ui)(ind.effect_ref, 2, &copy)
            }
            cmd::RENDER => ((*ind.utils).copy)(ind.effect_ref, &mut param(params, 0).u.ld, output, std::ptr::null(), std::ptr::null()),
            _ => err::NONE,
        }
    }
}
entry!(MilanTestSupervisor, supervisor, effects::SUPERVISOR);

unsafe fn layer(c: i32, ind: &mut InData, out: &mut OutData, _p: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    match c {
        cmd::PARAMS_SETUP => unsafe {
            out.num_params = 2;
            let mut l = ParamDef { param_type: param::LAYER, uu: 1, ..Default::default() };
            name(&mut l, "Other Layer");
            add(ind, l)
        },
        cmd::RENDER => unsafe { checkout_copy(ind, 1, 0, output) },
        _ => err::NONE,
    }
}
entry!(MilanTestLayer, layer, effects::LAYER);

/// Ten checkouts of earlier frames; the output is the oldest.
unsafe fn echo(c: i32, ind: &mut InData, out: &mut OutData, _p: *mut *mut ParamDef, output: *mut World, _e: *mut c_void) -> Err {
    match c {
        cmd::PARAMS_SETUP => unsafe { no_params(out) },
        cmd::RENDER => unsafe {
            for k in 0..10 {
                let e = checkout_copy(ind, 0, -k, output);
                if e != err::NONE {
                    return e;
                }
            }
            err::NONE
        },
        _ => err::NONE,
    }
}
entry!(MilanTestEcho, echo, effects::ECHO);

#[test]
fn every_effect_has_an_entry() {
    assert_eq!(effects::ALL.len(), 13);
}
