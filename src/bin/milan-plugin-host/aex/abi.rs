//! Milan's own declarations of the After Effects effect API as Milan's plug-in host uses it (64-bit Windows), written
//! by hand from public documentation (owner's decision of 2026-11-26, `docs/AE_PREMIERE_PLUGINS.md` §2.6). No Adobe SDK
//! file was used. Every item says where it comes from, in the classes of `docs/compat/aex-provenance.md`:
//! G = stated on a public guide page, G-inf = inferred from one, R = not on any public page found: the implementer's
//! understanding, UNVERIFIED until an open-source plug-in built by its authors runs (plan §2.4 (3)), M = Milan's choice.
//! Shared with `tools/aex-test-plugin` (`#[path]`), which therefore proves self-consistency only.
#![allow(dead_code, non_camel_case_types)]
use std::ffi::{c_char, c_void};

// ---------------------------------------------------------------- scalar types

/// Error code returned by every call (G: the entry point returns PF_Err; R: 32-bit).
pub type Err = i32;
/// A host memory handle: a pointer to the pointer to the block (G-inf: "handles" on the PF_OutData page; R: the
/// double indirection).
pub type Handle = *mut *mut c_void;
/// The opaque effect reference the callbacks take (G: `effect_ref`, `PF_ProgPtr`).
pub type ProgPtr = *mut c_void;
/// 16.16 fixed point (G: "16.16 fixed" for FIX_SLIDER on the parameters page; R: i32).
pub type Fixed = i32;

pub const fn fixed(v: f64) -> Fixed {
    (v * 65536.0) as i32
}

pub fn unfixed(v: Fixed) -> f64 {
    v as f64 / 65536.0
}

// ---------------------------------------------------------------- numeric constants

/// Command selectors (G: names and their order on the command selectors page; R: the values, which follow the
/// declaration order with an unused slot after GLOBAL_SETUP).
pub mod cmd {
    pub const ABOUT: i32 = 0;
    pub const GLOBAL_SETUP: i32 = 1;
    pub const GLOBAL_SETDOWN: i32 = 3;
    pub const PARAMS_SETUP: i32 = 4;
    pub const SEQUENCE_SETUP: i32 = 5;
    pub const SEQUENCE_RESETUP: i32 = 6;
    pub const SEQUENCE_FLATTEN: i32 = 7;
    pub const SEQUENCE_SETDOWN: i32 = 8;
    pub const DO_DIALOG: i32 = 9;
    pub const FRAME_SETUP: i32 = 10;
    pub const RENDER: i32 = 11;
    pub const FRAME_SETDOWN: i32 = 12;
    pub const USER_CHANGED_PARAM: i32 = 13;
    pub const UPDATE_PARAMS_UI: i32 = 14;
    pub const EVENT: i32 = 15;
    pub const ARBITRARY_CALLBACK: i32 = 22;
    pub const SMART_PRE_RENDER: i32 = 23;
    pub const SMART_RENDER: i32 = 24;
}

/// Error codes (G: names on the errors page; R: values).
pub mod err {
    pub const NONE: i32 = 0;
    pub const OUT_OF_MEMORY: i32 = 4;
    pub const INTERNAL_STRUCT_DAMAGED: i32 = 512;
    pub const INVALID_INDEX: i32 = 513;
    pub const UNRECOGNIZED_PARAM_TYPE: i32 = 514;
    pub const INVALID_CALLBACK: i32 = 515;
    pub const BAD_CALLBACK_PARAM: i32 = 516;
    pub const INTERRUPT_CANCEL: i32 = 517;
}

/// Parameter types (G: names on the parameters page; R: values).
pub mod param {
    pub const LAYER: i32 = 0;
    pub const SLIDER: i32 = 1;
    pub const FIX_SLIDER: i32 = 2;
    pub const ANGLE: i32 = 3;
    pub const CHECKBOX: i32 = 4;
    pub const COLOR: i32 = 5;
    pub const POINT: i32 = 6;
    pub const POPUP: i32 = 7;
    pub const CUSTOM: i32 = 8;
    pub const NO_DATA: i32 = 9;
    pub const FLOAT_SLIDER: i32 = 10;
    pub const ARBITRARY_DATA: i32 = 11;
    pub const PATH: i32 = 12;
    pub const GROUP_START: i32 = 13;
    pub const GROUP_END: i32 = 14;
    pub const BUTTON: i32 = 15;
    pub const POINT_3D: i32 = 18;
}

/// `PF_OutData::out_flags` bits (G: the 32 names, reserved ones included, in table order on the PF_OutData page;
/// G-inf: bit n is the n-th row, the table listing exactly one name per bit).
pub mod out_flag {
    pub const WIDE_TIME_INPUT: u32 = 1 << 1;
    pub const NON_PARAM_VARY: u32 = 1 << 2;
    pub const SEQUENCE_DATA_NEEDS_FLATTENING: u32 = 1 << 4;
    pub const I_DO_DIALOG: u32 = 1 << 5;
    pub const SEND_DO_DIALOG: u32 = 1 << 7;
    pub const I_EXPAND_BUFFER: u32 = 1 << 9;
    pub const PIX_INDEPENDENT: u32 = 1 << 10;
    pub const CUSTOM_UI: u32 = 1 << 15;
    pub const I_USE_AUDIO: u32 = 1 << 20;
    pub const DEEP_COLOR_AWARE: u32 = 1 << 25;
    pub const SEND_UPDATE_PARAMS_UI: u32 = 1 << 26;
    pub const AUDIO_EFFECT_TOO: u32 = 1 << 30;
    pub const AUDIO_EFFECT_ONLY: u32 = 1 << 31;
}

/// `PF_OutData::out_flags2` bits (G: names on the PF_OutData page; R: bit positions, the table having gaps).
pub mod out_flag2 {
    pub const SUPPORTS_QUERY_DYNAMIC_FLAGS: u32 = 1 << 0;
    pub const I_USE_3D_CAMERA: u32 = 1 << 1;
    pub const I_USE_3D_LIGHTS: u32 = 1 << 2;
    pub const SUPPORTS_SMART_RENDER: u32 = 1 << 10;
    pub const FLOAT_COLOR_AWARE: u32 = 1 << 12;
    pub const SUPPORTS_GPU_RENDER_F32: u32 = 1 << 25;
    pub const SUPPORTS_THREADED_RENDERING: u32 = 1 << 27;
}

/// `ParamDef::flags` (G: names on the PF_ParamDef page; R: values).
pub mod param_flag {
    pub const CANNOT_TIME_VARY: i32 = 1 << 0;
    pub const CANNOT_INTERP: i32 = 1 << 1;
    pub const COLLAPSE_TWIRLY: i32 = 1 << 4;
    pub const SUPERVISE: i32 = 1 << 5;
    pub const START_COLLAPSED: i32 = COLLAPSE_TWIRLY;
}

/// `ParamDef::ui_flags` (G: the ten names in table order on the PF_ParamDef page; G-inf: bit n is the n-th row).
pub mod pui {
    pub const TOPIC: i32 = 1 << 0;
    pub const CONTROL: i32 = 1 << 1;
    pub const STD_CONTROL_ONLY: i32 = 1 << 2;
    pub const NO_ECW_UI: i32 = 1 << 3;
    pub const ECW_SEPARATOR: i32 = 1 << 4;
    pub const DISABLED: i32 = 1 << 5;
    pub const INVISIBLE: i32 = 1 << 9;
}

/// `ParamDef::uu.change_flags` (G: `PF_ChangeFlag_CHANGED_VALUE` on the parameter supervision page; R: value).
pub const CHANGED_VALUE: i32 = 1 << 0;
/// The percent display flag of a slider (G: `PF_ValueDisplayFlag_PERCENT`, "the first bit", PF_ParamDef page).
pub const DISPLAY_PERCENT: i16 = 1 << 0;

/// `in_data.quality` (G: names PF_Quality_HI / PF_Quality_LO; R: values).
pub const QUALITY_LO: i32 = 0;
pub const QUALITY_HI: i32 = 1;
/// `in_data.field`: a whole frame (R).
pub const FIELD_FRAME: i32 = 0;
/// World flags (G: names; R: values).
pub const WORLD_DEEP: i32 = 1 << 0;
pub const WORLD_WRITEABLE: i32 = 1 << 1;

/// The host identity of Premiere Pro and other hosts (G: "Premiere Pro and other hosts use 'PrMr'", PF_InData page),
/// as a four-character code.
pub const APPL_ID_PRMR: i32 = i32::from_be_bytes(*b"PrMr");
/// The API version Milan reports (G: 13.4 is what Premiere reports, "Premiere Pro & Other Hosts" page). M: Milan
/// reports the same version.
pub const SPEC_MAJOR: i16 = 13;
pub const SPEC_MINOR: i16 = 4;

/// The error of a suite that is not there (R: 'S!Fd').
pub const SUITE_NOT_FOUND: i32 = i32::from_be_bytes(*b"S!Fd");

// ---------------------------------------------------------------- structures (R: types and padding, natural x64
// alignment; G: the field names and their order, from the pages named)

/// (G: name; R: two shorts.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpecVersion {
    pub major: i16,
    pub minor: i16,
}

/// A rectangle (R: four 32-bit edges, left/top/right/bottom).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// (R: numerator, unsigned denominator.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RationalScale {
    pub num: i32,
    pub den: u32,
}

/// (G: `PF_Point`, `origin`; R: h, v.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub h: i32,
    pub v: i32,
}

/// An 8-bit pixel (G: "Alpha, Red, Green, Blue from the low byte to the high byte", PF_EffectWorld page).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pixel {
    pub alpha: u8,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// A float pixel (G-inf: the same channel order as `Pixel`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PixelFloat {
    pub alpha: f32,
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

/// Audio of a layer (G: `src_snd` / `dest_snd` are a `PF_SoundWorld`; R: layout). Milan sends no audio.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SoundWorld {
    pub rate: f64,
    pub num_channels: i16,
    pub format: i16,
    pub bytes_per_sample: i16,
    pub num_samples: i32,
    pub data: *mut c_void,
}

/// An image (G: PF_EffectWorld page: world_flags, data, rowbytes, width, height, extent_hint, pix_aspect_ratio,
/// platform_ref, dephault; R: the reserved slots, their positions and the origin fields).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct World {
    pub reserved0: *mut c_void,
    pub reserved1: *mut c_void,
    pub world_flags: i32,
    pub data: *mut u8,
    pub rowbytes: i32,
    pub width: i32,
    pub height: i32,
    pub extent_hint: Rect,
    pub platform_ref: *mut c_void,
    pub reserved_long1: i32,
    pub reserved_long4: *mut c_void,
    pub pix_aspect_ratio: RationalScale,
    pub reserved_long2: *mut c_void,
    pub origin_x: i32,
    pub origin_y: i32,
    pub reserved_long3: i32,
    pub dephault: i32,
}

impl Default for World {
    fn default() -> Self {
        // SAFETY: all-zero is a valid empty world (null pointers, zero sizes).
        unsafe { std::mem::zeroed() }
    }
}

/// The interaction callbacks, held by value at the start of `InData` (G: "the un-macro'd function pointers are in
/// PF_InData", interaction callbacks page, which gives each prototype with `in_data` where the function takes
/// `effect_ref`; R: their order and the reserved slots).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InteractCallbacks {
    pub checkout_param: Option<unsafe extern "C" fn(ProgPtr, i32, i32, i32, u32, *mut ParamDef) -> Err>,
    pub checkin_param: Option<unsafe extern "C" fn(ProgPtr, *mut ParamDef) -> Err>,
    pub add_param: Option<unsafe extern "C" fn(ProgPtr, i32, *mut ParamDef) -> Err>,
    pub abort: Option<unsafe extern "C" fn(ProgPtr) -> Err>,
    pub progress: Option<unsafe extern "C" fn(ProgPtr, i32, i32) -> Err>,
    pub register_ui: Option<unsafe extern "C" fn(ProgPtr, *mut c_void) -> Err>,
    pub checkout_layer_audio: Option<unsafe extern "C" fn() -> Err>,
    pub checkin_layer_audio: Option<unsafe extern "C" fn() -> Err>,
    pub get_audio_data: Option<unsafe extern "C" fn() -> Err>,
    pub reserved_str: [*mut c_void; 3],
    pub reserved: [*mut c_void; 10],
}

/// What the host tells the plug-in with every command (G: the field order of the PF_InData page; R: types, the
/// `reserved` slot after `num_params`, and that `inter` is held by value).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InData {
    pub inter: InteractCallbacks,
    pub utils: *mut UtilCallbacks,
    pub effect_ref: ProgPtr,
    pub quality: i32,
    pub version: SpecVersion,
    pub serial_num: i32,
    pub appl_id: i32,
    pub num_params: i32,
    pub reserved: i32,
    pub what_cpu: i32,
    pub what_fpu: i32,
    pub current_time: i32,
    pub time_step: i32,
    pub total_time: i32,
    pub local_time_step: i32,
    pub time_scale: u32,
    pub field: i32,
    pub shutter_angle: Fixed,
    pub width: i32,
    pub height: i32,
    pub extent_hint: Rect,
    pub output_origin_x: i32,
    pub output_origin_y: i32,
    pub downsample_x: RationalScale,
    pub downsample_y: RationalScale,
    pub pixel_aspect_ratio: RationalScale,
    pub in_flags: i32,
    pub global_data: Handle,
    pub sequence_data: Handle,
    pub frame_data: Handle,
    pub start_samp: i32,
    pub dur_samp: i32,
    pub total_samp: i32,
    pub src_snd: SoundWorld,
    pub pica_basic: *mut BasicSuite,
    pub pre_effect_source_origin_x: i32,
    pub pre_effect_source_origin_y: i32,
    pub shutter_phase: Fixed,
}

/// What the plug-in tells the host back (G: the field order of the PF_OutData page; R: types and the lengths of `name`
/// (32) and `return_msg` (256)).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OutData {
    pub my_version: u32,
    pub name: [c_char; 32],
    pub global_data: Handle,
    pub num_params: i32,
    pub sequence_data: Handle,
    pub flat_sdata_size: i32,
    pub frame_data: Handle,
    pub width: i32,
    pub height: i32,
    pub origin: Point,
    pub out_flags: u32,
    pub return_msg: [c_char; 256],
    pub start_samp: i32,
    pub dur_samp: i32,
    pub dest_snd: SoundWorld,
    pub out_flags2: u32,
}

/// (R: the members of each parameter's definition; G: their union member names on the parameters page.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SliderDef {
    pub value: i32,
    pub value_str: [u8; 32],
    pub value_desc: [u8; 32],
    pub valid_min: i32,
    pub valid_max: i32,
    pub slider_min: i32,
    pub slider_max: i32,
    pub dephault: i32,
}

/// (G: valid_min / slider_min / valid_max / slider_max named in the parameters page's text; R: the rest.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedSliderDef {
    pub value: Fixed,
    pub value_str: [u8; 32],
    pub value_desc: [u8; 32],
    pub valid_min: Fixed,
    pub valid_max: Fixed,
    pub slider_min: Fixed,
    pub slider_max: Fixed,
    pub dephault: Fixed,
    pub precision: i16,
    pub display_flags: i16,
}

/// (G: value is a double (`PF_FPLong`), a `PF_ValueDisplayFlags` member; R: layout and the 32-bit float range fields.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FloatSliderDef {
    pub value: f64,
    pub phase: f64,
    pub value_desc: [u8; 32],
    pub valid_min: f32,
    pub valid_max: f32,
    pub slider_min: f32,
    pub slider_max: f32,
    pub dephault: f32,
    pub precision: i16,
    pub display_flags: i16,
    pub fs_flags: u32,
    pub curve_tolerance: f32,
    pub use_exponent: u8,
    pub pad: [u8; 3],
    pub exponent: f32,
}

/// (G: value is `PF_Fixed` degrees; R: layout.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AngleDef {
    pub value: Fixed,
    pub dephault: Fixed,
    pub valid_min: Fixed,
    pub valid_max: Fixed,
}

/// (G: value is `PF_Boolean`; R: layout, a 32-bit value and a name pointer for the box's label.)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CheckBoxDef {
    pub value: i32,
    pub dephault: u8,
    pub reserved: u8,
    pub reserved1: i16,
    pub name: *const c_char,
}

/// (G: value is a `PF_Pixel`; R: layout.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ColorDef {
    pub value: Pixel,
    pub dephault: Pixel,
}

/// (G: value is `PF_Fixed`, layer coordinates; R: layout.)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PointDef {
    pub x_value: Fixed,
    pub y_value: Fixed,
    pub reserved: [u8; 3],
    pub restrict_bounds: u8,
    pub x_dephault: Fixed,
    pub y_dephault: Fixed,
}

/// (G: value is a 1-based index, the names one string, entries separated by '|' (the guide's rendering shows '/';
/// the plan notes this, R), an entry "(-" a separator; R: layout.)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PopupDef {
    pub value: i32,
    pub num_choices: i16,
    pub dephault: i16,
    pub names: *const c_char,
}

/// (R.)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ButtonDef {
    pub value: i32,
    pub names: *const c_char,
}

/// (G: an opaque handle; R: layout.)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ArbitraryDef {
    pub id: i16,
    pub pad: i16,
    pub dephault: Handle,
    pub value: Handle,
    pub refcon: *mut c_void,
}

/// The per-type part of a parameter (G: the union member names `ld`, `sd`, `fd`, `fs_d`, `ad`, `bd`, `cd`, `td`, `pd`,
/// `arb_d`, `button_d`).
#[repr(C)]
#[derive(Clone, Copy)]
pub union ParamDefUnion {
    pub ld: World,
    pub sd: SliderDef,
    pub fd: FixedSliderDef,
    pub fs_d: FloatSliderDef,
    pub ad: AngleDef,
    pub bd: CheckBoxDef,
    pub cd: ColorDef,
    pub td: PointDef,
    pub pd: PopupDef,
    pub button_d: ButtonDef,
    pub arb_d: ArbitraryDef,
}

/// A parameter (G: PF_ParamDef page: id, change_flags, ui_flags, ui_width, ui_height, param_type, name[32], flags, u;
/// G-inf: `id` and `change_flags` share a union `uu`, which the parameter supervision page names as
/// `uu.change_flags`; R: the unused 32-bit slot before `u`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ParamDef {
    /// `id` during PARAMS_SETUP, `change_flags` afterwards.
    pub uu: i32,
    pub ui_flags: i32,
    pub ui_width: i16,
    pub ui_height: i16,
    pub param_type: i32,
    pub name: [c_char; 32],
    pub flags: i32,
    pub unused: i32,
    pub u: ParamDefUnion,
}

impl Default for ParamDef {
    fn default() -> Self {
        // SAFETY: all-zero is a valid (empty) definition.
        unsafe { std::mem::zeroed() }
    }
}

/// `extra` of USER_CHANGED_PARAM (G: "PF_UserChangedParamExtra", carrying the changed parameter's index; G-inf: that
/// index is its only member).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UserChangedParamExtra {
    pub param_index: i32,
}

/// The entry point (G: entry point page: `PF_Err main(PF_Cmd cmd, PF_InData *in_data, PF_OutData *out_data,
/// PF_ParamDef *params[], PF_LayerDef *output, void *extra)`, named in the PiPL; R: the C calling convention, the only
/// one on x64).
pub type Entry = unsafe extern "C" fn(i32, *mut InData, *mut OutData, *mut *mut ParamDef, *mut World, *mut c_void) -> Err;

// ---------------------------------------------------------------- suites

/// The suite broker in `in_data.pica_basic` (G: `AcquireSuite` is a member, the accessing function suites page; R:
/// its prototype, the other members and their order).
#[repr(C)]
pub struct BasicSuite {
    pub acquire_suite: unsafe extern "C" fn(*const c_char, i32, *mut *const c_void) -> i32,
    pub release_suite: unsafe extern "C" fn(*const c_char, i32) -> i32,
    pub is_equal: unsafe extern "C" fn(*const c_char, *const c_char) -> i32,
    pub allocate_block: unsafe extern "C" fn(usize, *mut *mut c_void) -> i32,
    pub free_block: unsafe extern "C" fn(*mut c_void) -> i32,
    pub reallocate_block: unsafe extern "C" fn(*mut c_void, usize, *mut *mut c_void) -> i32,
    pub undefined: unsafe extern "C" fn() -> i32,
}

/// Suite names and versions (G: the suites, their version suffixes; R: the name strings).
pub mod suite {
    pub const HANDLE: (&std::ffi::CStr, i32) = (c"PF Handle Suite", 1);
    pub const ITERATE8: (&std::ffi::CStr, i32) = (c"PF Iterate8 Suite", 1);
    pub const WORLD: (&std::ffi::CStr, i32) = (c"PF World Suite", 2);
    pub const FILL_MATTE: (&std::ffi::CStr, i32) = (c"PF Fill Matte Suite", 2);
    pub const WORLD_TRANSFORM: (&std::ffi::CStr, i32) = (c"PF World Transform Suite", 1);
    pub const ANSI: (&std::ffi::CStr, i32) = (c"PF ANSI Suite", 1);
    pub const COLOR_PARAM: (&std::ffi::CStr, i32) = (c"PF ColorParamSuite", 1);
    pub const POINT_PARAM: (&std::ffi::CStr, i32) = (c"PF PointParamSuite", 1);
    pub const ANGLE_PARAM: (&std::ffi::CStr, i32) = (c"PF AngleParamSuite", 1);
    pub const PARAM_UTILS: (&std::ffi::CStr, i32) = (c"PF Param Utils Suite", 3);
}

/// The per-pixel function of the iterate callbacks (G: iteration suites page).
pub type PixelFn8 = unsafe extern "C" fn(*mut c_void, i32, i32, *mut Pixel, *mut Pixel) -> Err;
/// The per-index function of `iterate_generic` (G: iteration suites page).
pub type GenericFn = unsafe extern "C" fn(*mut c_void, i32, i32, i32) -> Err;

/// PF_HandleSuite1 (G: memory allocation page: the six functions in this order with these prototypes).
#[repr(C)]
pub struct HandleSuite1 {
    pub new_handle: unsafe extern "C" fn(u64) -> Handle,
    pub lock_handle: unsafe extern "C" fn(Handle) -> *mut c_void,
    pub unlock_handle: unsafe extern "C" fn(Handle),
    pub dispose_handle: unsafe extern "C" fn(Handle),
    pub get_handle_size: unsafe extern "C" fn(Handle) -> u64,
    pub resize_handle: unsafe extern "C" fn(u64, *mut Handle) -> Err,
}

/// PF_Iterate8Suite1 (G: iteration suites page: the five functions in this order with these prototypes).
#[repr(C)]
pub struct Iterate8Suite1 {
    pub iterate: unsafe extern "C" fn(*mut InData, i32, i32, *mut World, *const Rect, *mut c_void, Option<PixelFn8>, *mut World) -> Err,
    pub iterate_origin: unsafe extern "C" fn(*mut InData, i32, i32, *mut World, *const Rect, *const Point, *mut c_void, Option<PixelFn8>, *mut World) -> Err,
    pub iterate_lut: unsafe extern "C" fn(*mut InData, i32, i32, *mut World, *const Rect, *mut u8, *mut u8, *mut u8, *mut u8, *mut World) -> Err,
    pub iterate_origin_non_clip_src: unsafe extern "C" fn(*mut InData, i32, i32, *mut World, *const Rect, *const Point, *mut c_void, Option<PixelFn8>, *mut World) -> Err,
    pub iterate_generic: unsafe extern "C" fn(i32, *mut c_void, Option<GenericFn>) -> Err,
}

/// PF_WorldSuite2 (G: image buffer management page: the three functions in this order with these prototypes; R: the
/// pixel format codes).
#[repr(C)]
pub struct WorldSuite2 {
    pub new_world: unsafe extern "C" fn(ProgPtr, i32, i32, u8, i32, *mut World) -> Err,
    pub dispose_world: unsafe extern "C" fn(ProgPtr, *mut World) -> Err,
    pub get_pixel_format: unsafe extern "C" fn(*const World, *mut i32) -> Err,
}

/// 8-bit ARGB (G: name `PF_PixelFormat_ARGB32`; R: value).
pub const PIXEL_FORMAT_ARGB32: i32 = i32::from_be_bytes(*b"argb");

/// PF_FillMatteSuite2 (G: graphics utility suites page: the seven functions in this order; prototypes of fill,
/// premultiply and premultiply_color; R: the other three's, which Milan does not serve).
#[repr(C)]
pub struct FillMatteSuite2 {
    pub fill: unsafe extern "C" fn(ProgPtr, *const Pixel, *const Rect, *mut World) -> Err,
    pub fill16: unsafe extern "C" fn() -> Err,
    pub fill_float: unsafe extern "C" fn() -> Err,
    pub premultiply: unsafe extern "C" fn(ProgPtr, i32, *mut World) -> Err,
    pub premultiply_color: unsafe extern "C" fn(ProgPtr, *mut World, *const Pixel, i32, *mut World) -> Err,
    pub premultiply_color16: unsafe extern "C" fn() -> Err,
    pub premultiply_color_float: unsafe extern "C" fn() -> Err,
}

/// PF_WorldTransformSuite1 (G: graphics utility suites page: the seven functions in this order; prototypes of all but
/// copy_hq, "the same parameters" as copy).
#[repr(C)]
pub struct WorldTransformSuite1 {
    pub composite_rect: unsafe extern "C" fn() -> Err,
    pub blend: unsafe extern "C" fn(ProgPtr, *const World, *const World, Fixed, *mut World) -> Err,
    pub convolve: unsafe extern "C" fn() -> Err,
    pub copy: unsafe extern "C" fn(ProgPtr, *mut World, *mut World, *const Rect, *const Rect) -> Err,
    pub copy_hq: unsafe extern "C" fn(ProgPtr, *mut World, *mut World, *const Rect, *const Rect) -> Err,
    pub transfer_rect: unsafe extern "C" fn() -> Err,
    pub transform_world: unsafe extern "C" fn() -> Err,
}

/// PF_ANSICallbackSuite1 (G: graphics utility suites page: the nineteen functions in this order; the maths ones return
/// a double, angles in radians; R: their argument lists, the C library's).
#[repr(C)]
pub struct AnsiSuite1 {
    pub acos: unsafe extern "C" fn(f64) -> f64,
    pub asin: unsafe extern "C" fn(f64) -> f64,
    pub atan: unsafe extern "C" fn(f64) -> f64,
    pub atan2: unsafe extern "C" fn(f64, f64) -> f64,
    pub ceil: unsafe extern "C" fn(f64) -> f64,
    pub cos: unsafe extern "C" fn(f64) -> f64,
    pub exp: unsafe extern "C" fn(f64) -> f64,
    pub fabs: unsafe extern "C" fn(f64) -> f64,
    pub floor: unsafe extern "C" fn(f64) -> f64,
    pub fmod: unsafe extern "C" fn(f64, f64) -> f64,
    pub hypot: unsafe extern "C" fn(f64, f64) -> f64,
    pub log: unsafe extern "C" fn(f64) -> f64,
    pub log10: unsafe extern "C" fn(f64) -> f64,
    pub pow: unsafe extern "C" fn(f64, f64) -> f64,
    pub sin: unsafe extern "C" fn(f64) -> f64,
    pub sqrt: unsafe extern "C" fn(f64) -> f64,
    pub tan: unsafe extern "C" fn(f64) -> f64,
    pub sprintf: unsafe extern "C" fn(*mut c_char, *const c_char, ...) -> i32,
    pub strcpy: unsafe extern "C" fn(*mut c_char, *const c_char) -> *mut c_char,
}

/// PF_ColorParamSuite1, PF_PointParamSuite1, PF_AngleParamSuite1 (G: parameters and floating point values page: one
/// function each, with these prototypes).
#[repr(C)]
pub struct ColorParamSuite1 {
    pub get_float_color: unsafe extern "C" fn(ProgPtr, *const ParamDef, *mut PixelFloat) -> Err,
}
#[repr(C)]
pub struct PointParamSuite1 {
    pub get_float_point: unsafe extern "C" fn(ProgPtr, *const ParamDef, *mut [f64; 2]) -> Err,
}
#[repr(C)]
pub struct AngleParamSuite1 {
    pub get_float_angle: unsafe extern "C" fn(ProgPtr, *const ParamDef, *mut f64) -> Err,
}

/// PF_ParamUtilSuite3 (G: parameter supervision page: the functions and prototypes in this order, two of them marked
/// "no longer supported" in version 3; G-inf: those two are not in the version-3 table; R: that the page's order is
/// the table's). Milan serves `update_param_ui` and `get_keyframe_count`; the rest return an error.
#[repr(C)]
pub struct ParamUtilsSuite3 {
    pub update_param_ui: unsafe extern "C" fn(ProgPtr, i32, *const ParamDef) -> Err,
    pub get_current_state: unsafe extern "C" fn() -> Err,
    pub are_states_identical: unsafe extern "C" fn() -> Err,
    pub is_identical_checkout: unsafe extern "C" fn() -> Err,
    pub find_keyframe_time: unsafe extern "C" fn() -> Err,
    pub get_keyframe_count: unsafe extern "C" fn(ProgPtr, i32, *mut i32) -> Err,
    pub checkout_keyframe: unsafe extern "C" fn() -> Err,
    pub checkin_keyframe: unsafe extern "C" fn() -> Err,
    pub key_index_to_time: unsafe extern "C" fn() -> Err,
}

/// The utility callbacks in `in_data.utils` (R, all of it: no public page gives this table; the names and prototypes
/// of the members Milan serves match the same-named suite functions above, which are G). Every slot is filled; the
/// ones Milan does not serve log their slot and return an error (`suites.rs`).
#[repr(C)]
pub struct UtilCallbacks {
    pub begin_sampling: *const c_void,
    pub subpixel_sample: *const c_void,
    pub area_sample: *const c_void,
    pub get_batch_func: *const c_void,
    pub end_sampling: *const c_void,
    pub composite_rect: *const c_void,
    pub blend: unsafe extern "C" fn(ProgPtr, *const World, *const World, Fixed, *mut World) -> Err,
    pub convolve: *const c_void,
    pub copy: unsafe extern "C" fn(ProgPtr, *mut World, *mut World, *const Rect, *const Rect) -> Err,
    pub fill: unsafe extern "C" fn(ProgPtr, *const Pixel, *const Rect, *mut World) -> Err,
    pub gaussian_kernel: *const c_void,
    pub iterate: unsafe extern "C" fn(*mut InData, i32, i32, *mut World, *const Rect, *mut c_void, Option<PixelFn8>, *mut World) -> Err,
    pub premultiply: unsafe extern "C" fn(ProgPtr, i32, *mut World) -> Err,
    pub premultiply_color: unsafe extern "C" fn(ProgPtr, *mut World, *const Pixel, i32, *mut World) -> Err,
    pub new_world: unsafe extern "C" fn(ProgPtr, i32, i32, i32, *mut World) -> Err,
    pub dispose_world: unsafe extern "C" fn(ProgPtr, *mut World) -> Err,
    pub iterate_origin: unsafe extern "C" fn(*mut InData, i32, i32, *mut World, *const Rect, *const Point, *mut c_void, Option<PixelFn8>, *mut World) -> Err,
    pub iterate_lut: unsafe extern "C" fn(*mut InData, i32, i32, *mut World, *const Rect, *mut u8, *mut u8, *mut u8, *mut u8, *mut World) -> Err,
    pub transfer_rect: *const c_void,
    pub transform_world: *const c_void,
    pub host_new_handle: unsafe extern "C" fn(u64) -> Handle,
    pub host_lock_handle: unsafe extern "C" fn(Handle) -> *mut c_void,
    pub host_unlock_handle: unsafe extern "C" fn(Handle),
    pub host_dispose_handle: unsafe extern "C" fn(Handle),
    pub get_callback_addr: *const c_void,
    pub app: *const c_void,
    /// The C maths and string functions, held by value (R: their order differs from the ANSI suite's).
    pub ansi: AnsiCallbacks,
    pub color_cb: [*const c_void; 8],
    pub get_platform_data: *const c_void,
    pub host_get_handle_size: unsafe extern "C" fn(Handle) -> u64,
    pub iterate_origin_non_clip_src: unsafe extern "C" fn(*mut InData, i32, i32, *mut World, *const Rect, *const Point, *mut c_void, Option<PixelFn8>, *mut World) -> Err,
    pub iterate_generic: unsafe extern "C" fn(i32, *mut c_void, Option<GenericFn>) -> Err,
    pub host_resize_handle: unsafe extern "C" fn(u64, *mut Handle) -> Err,
    pub subpixel_sample16: *const c_void,
    pub area_sample16: *const c_void,
    pub fill16: *const c_void,
    pub premultiply_color16: *const c_void,
    pub iterate16: *const c_void,
    pub iterate_origin16: *const c_void,
    pub iterate_origin_non_clip_src16: *const c_void,
    pub get_pixel_data8: *const c_void,
    pub get_pixel_data16: *const c_void,
    pub reserved: [*const c_void; 1],
}

/// (R.)
#[repr(C)]
pub struct AnsiCallbacks {
    pub atan: unsafe extern "C" fn(f64) -> f64,
    pub atan2: unsafe extern "C" fn(f64, f64) -> f64,
    pub ceil: unsafe extern "C" fn(f64) -> f64,
    pub cos: unsafe extern "C" fn(f64) -> f64,
    pub exp: unsafe extern "C" fn(f64) -> f64,
    pub fabs: unsafe extern "C" fn(f64) -> f64,
    pub floor: unsafe extern "C" fn(f64) -> f64,
    pub fmod: unsafe extern "C" fn(f64, f64) -> f64,
    pub hypot: unsafe extern "C" fn(f64, f64) -> f64,
    pub log: unsafe extern "C" fn(f64) -> f64,
    pub log10: unsafe extern "C" fn(f64) -> f64,
    pub pow: unsafe extern "C" fn(f64, f64) -> f64,
    pub sin: unsafe extern "C" fn(f64) -> f64,
    pub sqrt: unsafe extern "C" fn(f64) -> f64,
    pub tan: unsafe extern "C" fn(f64) -> f64,
    pub sprintf: unsafe extern "C" fn(*mut c_char, *const c_char, ...) -> i32,
    pub strcpy: unsafe extern "C" fn(*mut c_char, *const c_char) -> *mut c_char,
    pub asin: unsafe extern "C" fn(f64) -> f64,
    pub acos: unsafe extern "C" fn(f64) -> f64,
    pub ansi_procs: [*const c_void; 1],
}

// The layout as these declarations make it (x64): checked at compile time, so an edit that moves a field is noticed.
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<InteractCallbacks>() == 176);
    assert!(offset_of!(InData, utils) == 176);
    assert!(offset_of!(InData, effect_ref) == 184);
    assert!(offset_of!(InData, appl_id) == 204);
    assert!(offset_of!(InData, current_time) == 224);
    assert!(offset_of!(InData, extent_hint) == 260);
    assert!(offset_of!(InData, downsample_x) == 284);
    assert!(offset_of!(InData, global_data) == 312);
    assert!(offset_of!(InData, src_snd) == 352);
    assert!(offset_of!(InData, pica_basic) == 384);
    assert!(offset_of!(InData, shutter_phase) == 400);
    assert!(size_of::<InData>() == 408);
    assert!(size_of::<SoundWorld>() == 32);
    assert!(offset_of!(OutData, global_data) == 40);
    assert!(offset_of!(OutData, sequence_data) == 56);
    assert!(offset_of!(OutData, out_flags) == 96);
    assert!(offset_of!(OutData, return_msg) == 100);
    assert!(offset_of!(OutData, dest_snd) == 368);
    assert!(offset_of!(OutData, out_flags2) == 400);
    assert!(size_of::<OutData>() == 408);
    assert!(offset_of!(World, world_flags) == 16);
    assert!(offset_of!(World, data) == 24);
    assert!(offset_of!(World, rowbytes) == 32);
    assert!(offset_of!(World, extent_hint) == 44);
    assert!(offset_of!(World, platform_ref) == 64);
    assert!(offset_of!(World, pix_aspect_ratio) == 88);
    assert!(size_of::<World>() == 120);
    assert!(offset_of!(ParamDef, param_type) == 12);
    assert!(offset_of!(ParamDef, name) == 16);
    assert!(offset_of!(ParamDef, flags) == 48);
    assert!(offset_of!(ParamDef, u) == 56);
    assert!(size_of::<ParamDefUnion>() == 120);
    assert!(size_of::<ParamDef>() == 176);
    assert!(size_of::<FloatSliderDef>() == 88);
    assert!(size_of::<SliderDef>() == 88);
    assert!(size_of::<FixedSliderDef>() == 92);
    assert!(size_of::<PopupDef>() == 16);
    assert!(size_of::<CheckBoxDef>() == 16);
    assert!(size_of::<PointDef>() == 20);
    assert!(size_of::<ArbitraryDef>() == 32);
    assert!(size_of::<Pixel>() == 4);
    assert!(size_of::<BasicSuite>() == 56);
    assert!(size_of::<AnsiCallbacks>() == 160);
    assert!(offset_of!(UtilCallbacks, host_new_handle) == 160);
    assert!(offset_of!(UtilCallbacks, ansi) == 208);
    assert!(offset_of!(UtilCallbacks, host_get_handle_size) == 440);
    assert!(size_of::<UtilCallbacks>() == 552);
};
