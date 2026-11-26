//! Test plug-ins for Milan Pro's VST3 host, written against the raw VST3 COM API as a C++ plug-in would be:
//! "Milan Test Gain" (a component and a separate controller: gain and wet parameters, a 64-sample delay line whose
//! latency is reported, a program list of three programs, state, and an IPlugView that fills a colour),
//! "Milan Test Crash" (a single-component effect whose process writes to a null pointer) and "Milan Test Hang"
//! (a single-component effect whose process never returns).
#![allow(non_snake_case, clippy::missing_safety_doc)]
use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, uid};

const VENDOR: &str = "Milan Test";
const DELAY: usize = 64;
const PROGRAMS: [&str; 3] = ["Unity", "Half", "Double"];
const PROGRAM_GAIN: [f64; 3] = [0.5, 0.25, 1.0];

fn put8(s: &str, dst: &mut [c_char]) {
    for (d, c) in dst.iter_mut().zip(s.bytes().chain([0])) {
        *d = c as c_char;
    }
    if let Some(l) = dst.last_mut() {
        *l = 0;
    }
}

fn put16(s: &str, dst: &mut [u16]) {
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().chain([0])) {
        *d = c;
    }
    if let Some(l) = dst.last_mut() {
        *l = 0;
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------- bus boilerplate shared by the effects

unsafe fn bus_info(media: MediaType, dir: BusDirection, index: i32, bus: *mut BusInfo) -> tresult {
    if media != MediaTypes_::kAudio as MediaType || index != 0 {
        return kInvalidArgument;
    }
    unsafe {
        let b = &mut *bus;
        b.mediaType = media;
        b.direction = dir;
        b.channelCount = 2;
        put16(if dir == BusDirections_::kInput as BusDirection { "Input" } else { "Output" }, &mut b.name);
        b.busType = BusTypes_::kMain as BusType;
        b.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
    }
    kResultOk
}

unsafe fn arrangement(dir: BusDirection, index: i32, arr: *mut SpeakerArrangement) -> tresult {
    let _ = dir;
    if index != 0 {
        return kInvalidArgument;
    }
    unsafe { *arr = SpeakerArr::kStereo };
    kResultOk
}

/// Read the last point of every parameter change queue.
unsafe fn changes(data: &ProcessData, mut f: impl FnMut(u32, f64)) {
    unsafe {
        if let Some(pc) = ComRef::from_raw(data.inputParameterChanges) {
            for i in 0..pc.getParameterCount() {
                if let Some(q) = ComRef::from_raw(pc.getParameterData(i)) {
                    let n = q.getPointCount();
                    let (mut off, mut v) = (0, 0.0);
                    if n > 0 && q.getPoint(n - 1, &mut off, &mut v) == kResultOk {
                        f(q.getParameterId(), v);
                    }
                }
            }
        }
    }
}

/// The main bus's channel slices of a stereo float block.
unsafe fn channels<'a>(data: &ProcessData) -> Option<([&'a [f32]; 2], [&'a mut [f32]; 2])> {
    unsafe {
        if data.numInputs < 1 || data.numOutputs < 1 || data.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as i32 {
            return None;
        }
        let (i, o) = (&*data.inputs, &*data.outputs);
        if i.numChannels < 2 || o.numChannels < 2 {
            return None;
        }
        let n = data.numSamples as usize;
        let ic = std::slice::from_raw_parts(i.__field0.channelBuffers32, 2);
        let oc = std::slice::from_raw_parts(o.__field0.channelBuffers32, 2);
        Some(([std::slice::from_raw_parts(ic[0], n), std::slice::from_raw_parts(ic[1], n)], [std::slice::from_raw_parts_mut(oc[0], n), std::slice::from_raw_parts_mut(oc[1], n)]))
    }
}

// ---------------------------------------------------------------- Milan Test Gain: processor

#[derive(Clone, Copy)]
struct GainState {
    /// Normalized: gain = 2 * value.
    gain: f64,
    wet: f64,
    program: i32,
}

impl Default for GainState {
    fn default() -> Self {
        GainState { gain: 0.5, wet: 1.0, program: 0 }
    }
}

impl GainState {
    fn write(&self, s: &ComPtr<IBStream>) -> tresult {
        let mut bytes = Vec::new();
        bytes.extend(self.gain.to_le_bytes());
        bytes.extend(self.wet.to_le_bytes());
        bytes.extend(self.program.to_le_bytes());
        let mut n = 0;
        unsafe { s.write(bytes.as_ptr() as *mut c_void, bytes.len() as i32, &mut n) }
    }

    fn read(s: &ComPtr<IBStream>) -> Option<GainState> {
        let mut bytes = [0u8; 20];
        let mut n = 0;
        unsafe { s.read(bytes.as_mut_ptr() as *mut c_void, 20, &mut n) };
        if n != 20 {
            return None;
        }
        Some(GainState { gain: f64::from_le_bytes(bytes[0..8].try_into().unwrap()), wet: f64::from_le_bytes(bytes[8..16].try_into().unwrap()), program: i32::from_le_bytes(bytes[16..20].try_into().unwrap()) })
    }
}

struct GainProcessor {
    st: Mutex<GainState>,
    lines: Mutex<[Vec<f32>; 2]>,
    pos: Mutex<usize>,
}

impl GainProcessor {
    const CID: TUID = uid(0x4D694C61, 0x6E546573, 0x74476169, 0x6E000001);

    fn new() -> Self {
        GainProcessor { st: Mutex::new(GainState::default()), lines: Mutex::new([vec![0.0; DELAY], vec![0.0; DELAY]]), pos: Mutex::new(0) }
    }
}

impl Class for GainProcessor {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements, IConnectionPoint);
}

impl IPluginBaseTrait for GainProcessor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IConnectionPointTrait for GainProcessor {
    unsafe fn connect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }

    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }

    unsafe fn notify(&self, _message: *mut IMessage) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for GainProcessor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        unsafe { *class_id = GainController::CID };
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media: MediaType, _dir: BusDirection) -> i32 {
        if media == MediaTypes_::kAudio as MediaType { 1 } else { 0 }
    }

    unsafe fn getBusInfo(&self, media: MediaType, dir: BusDirection, index: i32, bus: *mut BusInfo) -> tresult {
        unsafe { bus_info(media, dir, index, bus) }
    }

    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(&self, _media: MediaType, _dir: BusDirection, _index: i32, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, state: TBool) -> tresult {
        if state != 0 {
            for l in lock(&self.lines).iter_mut() {
                l.fill(0.0);
            }
            *lock(&self.pos) = 0;
        }
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        match unsafe { ComRef::from_raw(state) }.and_then(|s| GainState::read(&s.to_com_ptr())) {
            Some(st) => {
                *lock(&self.st) = st;
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        match unsafe { ComRef::from_raw(state) } {
            Some(s) => lock(&self.st).write(&s.to_com_ptr()),
            None => kInvalidArgument,
        }
    }
}

impl IAudioProcessorTrait for GainProcessor {
    unsafe fn setBusArrangements(&self, inputs: *mut SpeakerArrangement, num_ins: i32, outputs: *mut SpeakerArrangement, num_outs: i32) -> tresult {
        unsafe {
            if num_ins == 1 && num_outs == 1 && *inputs == SpeakerArr::kStereo && *outputs == SpeakerArr::kStereo {
                return kResultTrue;
            }
        }
        kResultFalse
    }

    unsafe fn getBusArrangement(&self, dir: BusDirection, index: i32, arr: *mut SpeakerArrangement) -> tresult {
        unsafe { arrangement(dir, index, arr) }
    }

    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 as i32 { kResultOk } else { kResultFalse }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        DELAY as u32
    }

    unsafe fn setupProcessing(&self, _setup: *mut ProcessSetup) -> tresult {
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = unsafe { &*data };
        {
            let mut st = lock(&self.st);
            unsafe {
                changes(data, |id, v| match id {
                    0 => st.gain = v,
                    1 => st.wet = v,
                    2 => {
                        st.program = (v * (PROGRAMS.len() - 1) as f64).round().clamp(0.0, 2.0) as i32;
                        st.gain = PROGRAM_GAIN[st.program as usize];
                    }
                    _ => {}
                })
            };
        }
        let st = *lock(&self.st);
        let (gain, wet) = ((st.gain * 2.0) as f32, st.wet as f32);
        let Some((inp, out)) = (unsafe { channels(data) }) else { return kResultOk };
        let mut lines = lock(&self.lines);
        let mut pos = lock(&self.pos);
        for k in 0..data.numSamples as usize {
            for c in 0..2 {
                let delayed = lines[c][*pos];
                lines[c][*pos] = inp[c][k];
                out[c][k] = gain * (inp[c][k] * (1.0 - wet) + delayed * wet);
            }
            *pos = (*pos + 1) % DELAY;
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> u32 {
        DELAY as u32
    }
}

impl IProcessContextRequirementsTrait for GainProcessor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

// ---------------------------------------------------------------- Milan Test Gain: controller

struct GainController {
    st: Mutex<GainState>,
    handler: Mutex<Option<ComPtr<IComponentHandler>>>,
}

impl GainController {
    const CID: TUID = uid(0x4D694C61, 0x6E546573, 0x74476169, 0x6E000002);
}

impl Class for GainController {
    type Interfaces = (IEditController, IUnitInfo, IConnectionPoint);
}

impl IPluginBaseTrait for GainController {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        *lock(&self.handler) = None;
        kResultOk
    }
}

impl IConnectionPointTrait for GainController {
    unsafe fn connect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }

    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }

    unsafe fn notify(&self, _message: *mut IMessage) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for GainController {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        match unsafe { ComRef::from_raw(state) }.and_then(|s| GainState::read(&s.to_com_ptr())) {
            Some(st) => {
                *lock(&self.st) = st;
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> i32 {
        3
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let info = unsafe { &mut *info };
        info.id = index as u32;
        info.unitId = kRootUnitId;
        match index {
            0 => {
                put16("Gain", &mut info.title);
                put16("Gain", &mut info.shortTitle);
                put16("x", &mut info.units);
                info.stepCount = 0;
                info.defaultNormalizedValue = 0.5;
                info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
            }
            1 => {
                put16("Wet", &mut info.title);
                put16("Wet", &mut info.shortTitle);
                put16("", &mut info.units);
                info.stepCount = 0;
                info.defaultNormalizedValue = 1.0;
                info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
            }
            2 => {
                put16("Program", &mut info.title);
                put16("Prog", &mut info.shortTitle);
                put16("", &mut info.units);
                info.stepCount = PROGRAMS.len() as i32 - 1;
                info.defaultNormalizedValue = 0.0;
                info.flags = ParameterInfo_::ParameterFlags_::kIsProgramChange | ParameterInfo_::ParameterFlags_::kIsList;
            }
            _ => return kInvalidArgument,
        }
        kResultOk
    }

    unsafe fn getParamStringByValue(&self, id: ParamID, value: ParamValue, string: *mut String128) -> tresult {
        let s = match id {
            0 => format!("{:.2}", value * 2.0),
            1 => format!("{:.0} %", value * 100.0),
            2 => PROGRAMS[(value * 2.0).round().clamp(0.0, 2.0) as usize].to_string(),
            _ => return kInvalidArgument,
        };
        unsafe { put16(&s, &mut *string) };
        kResultOk
    }

    unsafe fn getParamValueByString(&self, _id: ParamID, _string: *mut TChar, _value: *mut ParamValue) -> tresult {
        kResultFalse
    }

    unsafe fn normalizedParamToPlain(&self, id: ParamID, v: ParamValue) -> ParamValue {
        match id {
            0 => v * 2.0,
            2 => (v * 2.0).round(),
            _ => v,
        }
    }

    unsafe fn plainParamToNormalized(&self, id: ParamID, v: ParamValue) -> ParamValue {
        match id {
            0 | 2 => v / 2.0,
            _ => v,
        }
    }

    unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
        let st = lock(&self.st);
        match id {
            0 => st.gain,
            1 => st.wet,
            2 => st.program as f64 / 2.0,
            _ => 0.0,
        }
    }

    unsafe fn setParamNormalized(&self, id: ParamID, v: ParamValue) -> tresult {
        let mut st = lock(&self.st);
        match id {
            0 => st.gain = v,
            1 => st.wet = v,
            2 => {
                st.program = (v * 2.0).round().clamp(0.0, 2.0) as i32;
                st.gain = PROGRAM_GAIN[st.program as usize];
                // A program change moves the gain: the host hears it as the plug-in's own edit.
                let gain = st.gain;
                drop(st);
                if let Some(h) = lock(&self.handler).clone() {
                    unsafe {
                        h.beginEdit(0);
                        h.performEdit(0, gain);
                        h.endEdit(0);
                    }
                }
                return kResultOk;
            }
            _ => return kInvalidArgument,
        }
        kResultOk
    }

    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        *lock(&self.handler) = unsafe { ComRef::from_raw(handler) }.map(|r| r.to_com_ptr());
        kResultOk
    }

    unsafe fn createView(&self, name: *const c_char) -> *mut IPlugView {
        if name.is_null() || unsafe { std::ffi::CStr::from_ptr(name) }.to_bytes() != b"editor" {
            return std::ptr::null_mut();
        }
        ComWrapper::new(ColorView::default()).to_com_ptr::<IPlugView>().map_or(std::ptr::null_mut(), |p| p.into_raw())
    }
}

impl IUnitInfoTrait for GainController {
    unsafe fn getUnitCount(&self) -> i32 {
        1
    }

    unsafe fn getUnitInfo(&self, index: i32, info: *mut UnitInfo) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        unsafe {
            let i = &mut *info;
            i.id = kRootUnitId;
            i.parentUnitId = kNoParentUnitId;
            put16("Root", &mut i.name);
            i.programListId = 0;
        }
        kResultOk
    }

    unsafe fn getProgramListCount(&self) -> i32 {
        1
    }

    unsafe fn getProgramListInfo(&self, index: i32, info: *mut ProgramListInfo) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        unsafe {
            let i = &mut *info;
            i.id = 0;
            put16("Programs", &mut i.name);
            i.programCount = PROGRAMS.len() as i32;
        }
        kResultOk
    }

    unsafe fn getProgramName(&self, list: ProgramListID, index: i32, name: *mut String128) -> tresult {
        match (list, PROGRAMS.get(index as usize)) {
            (0, Some(n)) => {
                unsafe { put16(n, &mut *name) };
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn getProgramInfo(&self, _list: ProgramListID, _index: i32, _attr: vst3::Steinberg::Vst::CString, _value: *mut String128) -> tresult {
        kResultFalse
    }

    unsafe fn hasProgramPitchNames(&self, _list: ProgramListID, _index: i32) -> tresult {
        kResultFalse
    }

    unsafe fn getProgramPitchName(&self, _list: ProgramListID, _index: i32, _pitch: i16, _name: *mut String128) -> tresult {
        kResultFalse
    }

    unsafe fn getSelectedUnit(&self) -> UnitID {
        kRootUnitId
    }

    unsafe fn selectUnit(&self, _unit: UnitID) -> tresult {
        kResultOk
    }

    unsafe fn getUnitByBus(&self, _media: MediaType, _dir: BusDirection, _bus: i32, _channel: i32, unit: *mut UnitID) -> tresult {
        unsafe { *unit = kRootUnitId };
        kResultOk
    }

    unsafe fn setUnitProgramData(&self, _list: i32, _index: i32, _data: *mut IBStream) -> tresult {
        kNotImplemented
    }
}

// ---------------------------------------------------------------- the view: a child window filled with a colour

#[derive(Default)]
struct ColorView {
    hwnd: Mutex<usize>,
    size: Mutex<(i32, i32)>,
}

impl Class for ColorView {
    type Interfaces = (IPlugView,);
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const CLASS: &[u16] = &[b'M' as u16, b'i' as u16, b'l' as u16, b'a' as u16, b'n' as u16, b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, b'V' as u16, b'i' as u16, b'e' as u16, b'w' as u16, 0];

    unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_PAINT {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let brush = CreateSolidBrush(0x0040_80C0); // a warm orange (0x00BBGGRR)
                FillRect(hdc, &ps.rcPaint, brush);
                DeleteObject(brush as _);
                EndPaint(hwnd, &ps);
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }

    pub unsafe fn create(parent: HWND, w: i32, h: i32) -> HWND {
        unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW { style: 0, lpfnWndProc: Some(proc_), cbClsExtra: 0, cbWndExtra: 0, hInstance: hinst, hIcon: std::ptr::null_mut(), hCursor: std::ptr::null_mut(), hbrBackground: std::ptr::null_mut(), lpszMenuName: std::ptr::null(), lpszClassName: CLASS.as_ptr() };
            RegisterClassW(&wc);
            CreateWindowExW(0, CLASS.as_ptr(), std::ptr::null(), WS_CHILD | WS_VISIBLE, 0, 0, w, h, parent, std::ptr::null_mut(), hinst, std::ptr::null())
        }
    }

    pub unsafe fn resize(hwnd: HWND, w: i32, h: i32) {
        unsafe { MoveWindow(hwnd, 0, 0, w, h, 1) };
    }

    pub unsafe fn destroy(hwnd: HWND) {
        unsafe { DestroyWindow(hwnd) };
    }
}

impl IPlugViewTrait for ColorView {
    unsafe fn isPlatformTypeSupported(&self, t: FIDString) -> tresult {
        if !t.is_null() && unsafe { std::ffi::CStr::from_ptr(t) }.to_bytes() == b"HWND" { kResultTrue } else { kResultFalse }
    }

    unsafe fn attached(&self, parent: *mut c_void, _t: FIDString) -> tresult {
        let (w, h) = *lock(&self.size);
        let (w, h) = if w > 0 { (w, h) } else { (320, 200) };
        #[cfg(windows)]
        {
            let hwnd = unsafe { win::create(parent as _, w, h) };
            *lock(&self.hwnd) = hwnd as usize;
            return if hwnd.is_null() { kResultFalse } else { kResultOk };
        }
        #[cfg(not(windows))]
        {
            let _ = parent;
            kResultFalse
        }
    }

    unsafe fn removed(&self) -> tresult {
        let h = std::mem::take(&mut *lock(&self.hwnd));
        #[cfg(windows)]
        if h != 0 {
            unsafe { win::destroy(h as _) };
        }
        let _ = h;
        kResultOk
    }

    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyDown(&self, _key: char16, _code: i16, _mods: i16) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyUp(&self, _key: char16, _code: i16, _mods: i16) -> tresult {
        kResultFalse
    }

    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        let (w, h) = *lock(&self.size);
        let (w, h) = if w > 0 { (w, h) } else { (320, 200) };
        unsafe { *size = ViewRect { left: 0, top: 0, right: w, bottom: h } };
        kResultOk
    }

    unsafe fn onSize(&self, new_size: *mut ViewRect) -> tresult {
        let r = unsafe { *new_size };
        let (w, h) = ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
        *lock(&self.size) = (w, h);
        let hwnd = *lock(&self.hwnd);
        #[cfg(windows)]
        if hwnd != 0 {
            unsafe { win::resize(hwnd as _, w, h) };
        }
        let _ = hwnd;
        kResultOk
    }

    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setFrame(&self, _frame: *mut IPlugFrame) -> tresult {
        kResultOk
    }

    unsafe fn canResize(&self) -> tresult {
        kResultTrue
    }

    unsafe fn checkSizeConstraint(&self, _rect: *mut ViewRect) -> tresult {
        kResultTrue
    }
}

// ---------------------------------------------------------------- Milan Test Crash and Milan Test Hang (single component)

struct Bad {
    hang: bool,
}

impl Bad {
    const CRASH: TUID = uid(0x4D694C61, 0x6E546573, 0x74437261, 0x73680001);
    const HANG: TUID = uid(0x4D694C61, 0x6E546573, 0x7448616E, 0x67000001);
}

impl Class for Bad {
    type Interfaces = (IComponent, IAudioProcessor, IEditController);
}

impl IPluginBaseTrait for Bad {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Bad {
    unsafe fn getControllerClassId(&self, _class_id: *mut TUID) -> tresult {
        kResultFalse
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media: MediaType, _dir: BusDirection) -> i32 {
        if media == MediaTypes_::kAudio as MediaType { 1 } else { 0 }
    }

    unsafe fn getBusInfo(&self, media: MediaType, dir: BusDirection, index: i32, bus: *mut BusInfo) -> tresult {
        unsafe { bus_info(media, dir, index, bus) }
    }

    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(&self, _media: MediaType, _dir: BusDirection, _index: i32, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
}

impl IAudioProcessorTrait for Bad {
    unsafe fn setBusArrangements(&self, _i: *mut SpeakerArrangement, _ni: i32, _o: *mut SpeakerArrangement, _no: i32) -> tresult {
        kResultTrue
    }

    unsafe fn getBusArrangement(&self, dir: BusDirection, index: i32, arr: *mut SpeakerArrangement) -> tresult {
        unsafe { arrangement(dir, index, arr) }
    }

    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 as i32 { kResultOk } else { kResultFalse }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }

    unsafe fn setupProcessing(&self, _setup: *mut ProcessSetup) -> tresult {
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn process(&self, _data: *mut ProcessData) -> tresult {
        if self.hang {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        // An access violation, as a buggy plug-in would have.
        let p: *mut u32 = std::ptr::null_mut();
        unsafe { std::ptr::write_volatile(p, 1) };
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

impl IEditControllerTrait for Bad {
    unsafe fn setComponentState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> i32 {
        1
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        info.id = 0;
        put16("Amount", &mut info.title);
        put16("Amt", &mut info.shortTitle);
        info.stepCount = 0;
        info.defaultNormalizedValue = 0.5;
        info.unitId = kRootUnitId;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
        kResultOk
    }

    unsafe fn getParamStringByValue(&self, _id: ParamID, value: ParamValue, string: *mut String128) -> tresult {
        unsafe { put16(&format!("{value:.2}"), &mut *string) };
        kResultOk
    }

    unsafe fn getParamValueByString(&self, _id: ParamID, _string: *mut TChar, _value: *mut ParamValue) -> tresult {
        kResultFalse
    }

    unsafe fn normalizedParamToPlain(&self, _id: ParamID, v: ParamValue) -> ParamValue {
        v
    }

    unsafe fn plainParamToNormalized(&self, _id: ParamID, v: ParamValue) -> ParamValue {
        v
    }

    unsafe fn getParamNormalized(&self, _id: ParamID) -> ParamValue {
        0.5
    }

    unsafe fn setParamNormalized(&self, _id: ParamID, _v: ParamValue) -> tresult {
        kResultOk
    }

    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }

    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        std::ptr::null_mut()
    }
}

// ---------------------------------------------------------------- factory

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory, IPluginFactory2);
}

/// (cid, category, name, sub categories)
const CLASSES: [(TUID, &str, &str, &str); 4] = [
    (GainProcessor::CID, "Audio Module Class", "Milan Test Gain", "Fx|Tools"),
    (GainController::CID, "Component Controller Class", "Milan Test Gain Controller", ""),
    (Bad::CRASH, "Audio Module Class", "Milan Test Crash", "Fx"),
    (Bad::HANG, "Audio Module Class", "Milan Test Hang", "Fx"),
];

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = unsafe { &mut *info };
        put8(VENDOR, &mut info.vendor);
        put8("https://github.com/MeridianNLE", &mut info.url);
        put8("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as i32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        CLASSES.len() as i32
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let Some(c) = CLASSES.get(index as usize) else { return kInvalidArgument };
        let info = unsafe { &mut *info };
        info.cid = c.0;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as i32;
        put8(c.1, &mut info.category);
        put8(c.2, &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, obj: *mut *mut c_void) -> tresult {
        let cid = unsafe { *(cid as *const TUID) };
        let instance: Option<ComPtr<FUnknown>> = if cid == GainProcessor::CID {
            ComWrapper::new(GainProcessor::new()).to_com_ptr::<FUnknown>()
        } else if cid == GainController::CID {
            ComWrapper::new(GainController { st: Mutex::new(GainState::default()), handler: Mutex::new(None) }).to_com_ptr::<FUnknown>()
        } else if cid == Bad::CRASH {
            ComWrapper::new(Bad { hang: false }).to_com_ptr::<FUnknown>()
        } else if cid == Bad::HANG {
            ComWrapper::new(Bad { hang: true }).to_com_ptr::<FUnknown>()
        } else {
            None
        };
        match instance {
            Some(i) => unsafe {
                let p = i.as_ptr();
                ((*(*p).vtbl).queryInterface)(p, iid as *mut TUID, obj)
            },
            None => kInvalidArgument,
        }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        let Some(c) = CLASSES.get(index as usize) else { return kInvalidArgument };
        let info = unsafe { &mut *info };
        info.cid = c.0;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as i32;
        put8(c.1, &mut info.category);
        put8(c.2, &mut info.name);
        info.classFlags = ComponentFlags_::kDistributable as u32;
        put8(c.3, &mut info.subCategories);
        put8(VENDOR, &mut info.vendor);
        put8("1.0.0", &mut info.version);
        put8("VST 3.7.9", &mut info.sdkVersion);
        kResultOk
    }
}

#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>().map_or(std::ptr::null_mut(), |p| p.into_raw())
}
