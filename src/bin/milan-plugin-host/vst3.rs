//! R34a/R34b: one loaded VST3 module (a `.vst3` DLL) through the coupler-rs `vst3` bindings: its factory's audio
//! effect classes, instances (IComponent + IAudioProcessor, with their IEditController, connected directly),
//! 32-bit float stereo processing at the project rate, parameters both ways, state, program lists, and the plug-in's
//! IPlugView in a host-owned window (`editor.rs`). The host objects a plug-in talks to (IHostApplication with
//! IMessage/IAttributeList, IComponentHandler, IBStream, IParameterChanges, an empty IEventList, IPlugFrame) are below.
#![allow(clippy::missing_safety_doc)]
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::sync::{Arc, Mutex};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper};
use crate::editor::{Editor, View};
use crate::proto::{AInstance, AOut, AParam, AProcess, AState, AudioDesc, EditorOp, b64, unb64};
use crate::suites::lock;

const AUDIO_CLASS: &str = "Audio Module Class";

fn ok(r: tresult) -> bool {
    r == kResultOk || r == kResultTrue
}

fn utf16(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

fn ascii(s: &[c_char]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    s[..n].iter().map(|&c| c as u8 as char).collect()
}

fn put16(s: &str, dst: &mut [u16]) {
    let mut n = 0;
    for (d, c) in dst.iter_mut().zip(s.encode_utf16()) {
        *d = c;
        n += 1;
    }
    if n < dst.len() {
        dst[n] = 0;
    } else if let Some(l) = dst.last_mut() {
        *l = 0;
    }
}

/// A class id as `.vstpreset` and `moduleinfo.json` spell it: the 16 bytes in memory order, upper-case hex.
pub fn cid_hex(cid: &TUID) -> String {
    cid.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

fn hex_cid(s: &str) -> Option<TUID> {
    if s.len() != 32 {
        return None;
    }
    let mut out = [0 as c_char; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()? as c_char;
    }
    Some(out)
}

// ---------------------------------------------------------------- host objects

/// IHostApplication: the host's name, and IMessage objects for component-controller messaging.
struct HostApp;

impl Class for HostApp {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        unsafe { put16("Milan Pro", &mut *name) };
        kResultOk
    }

    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        unsafe {
            if *cid == IMessage_iid && (*iid == IMessage_iid || *iid == FUnknown_iid) {
                let m = ComWrapper::new(Message::default()).to_com_ptr::<IMessage>().expect("IMessage");
                *obj = m.into_raw() as *mut c_void;
                return kResultOk;
            }
            if *cid == IAttributeList_iid {
                let a = ComWrapper::new(Attrs::default()).to_com_ptr::<IAttributeList>().expect("IAttributeList");
                *obj = a.into_raw() as *mut c_void;
                return kResultOk;
            }
        }
        kResultFalse
    }
}

#[derive(Clone)]
enum Attr {
    Int(i64),
    Float(f64),
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

#[derive(Default)]
struct Attrs {
    map: Mutex<HashMap<String, Attr>>,
}

impl Class for Attrs {
    type Interfaces = (IAttributeList,);
}

unsafe fn attr_id(id: IAttrID) -> String {
    if id.is_null() { String::new() } else { unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned() }
}

impl IAttributeListTrait for Attrs {
    unsafe fn setInt(&self, id: IAttrID, value: int64) -> tresult {
        lock(&self.map).insert(unsafe { attr_id(id) }, Attr::Int(value));
        kResultOk
    }

    unsafe fn getInt(&self, id: IAttrID, value: *mut int64) -> tresult {
        match lock(&self.map).get(&unsafe { attr_id(id) }) {
            Some(Attr::Int(v)) => {
                unsafe { *value = *v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: IAttrID, value: f64) -> tresult {
        lock(&self.map).insert(unsafe { attr_id(id) }, Attr::Float(value));
        kResultOk
    }

    unsafe fn getFloat(&self, id: IAttrID, value: *mut f64) -> tresult {
        match lock(&self.map).get(&unsafe { attr_id(id) }) {
            Some(Attr::Float(v)) => {
                unsafe { *value = *v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: IAttrID, string: *const TChar) -> tresult {
        let mut v = Vec::new();
        if !string.is_null() {
            let mut i = 0;
            // SAFETY: a NUL-terminated UTF-16 string from the plug-in.
            while unsafe { *string.add(i) } != 0 {
                v.push(unsafe { *string.add(i) });
                i += 1;
            }
        }
        lock(&self.map).insert(unsafe { attr_id(id) }, Attr::Str(v));
        kResultOk
    }

    unsafe fn getString(&self, id: IAttrID, string: *mut TChar, size_in_bytes: uint32) -> tresult {
        match lock(&self.map).get(&unsafe { attr_id(id) }) {
            Some(Attr::Str(v)) => {
                let n = (size_in_bytes as usize / 2).saturating_sub(1).min(v.len());
                unsafe {
                    std::ptr::copy_nonoverlapping(v.as_ptr(), string, n);
                    *string.add(n) = 0;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setBinary(&self, id: IAttrID, data: *const c_void, size_in_bytes: uint32) -> tresult {
        let v = if data.is_null() { Vec::new() } else { unsafe { std::slice::from_raw_parts(data as *const u8, size_in_bytes as usize) }.to_vec() };
        lock(&self.map).insert(unsafe { attr_id(id) }, Attr::Bin(v));
        kResultOk
    }

    unsafe fn getBinary(&self, id: IAttrID, data: *mut *const c_void, size_in_bytes: *mut uint32) -> tresult {
        match lock(&self.map).get(&unsafe { attr_id(id) }) {
            Some(Attr::Bin(v)) => {
                // The plug-in reads it right away (as the SDK's own HostAttributeList allows); the bytes live in the map.
                unsafe {
                    *data = v.as_ptr() as *const c_void;
                    *size_in_bytes = v.len() as u32;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

struct Message {
    id: Mutex<Vec<u8>>,
    attrs: ComWrapper<Attrs>,
}

impl Default for Message {
    fn default() -> Self {
        Message { id: Mutex::new(vec![0]), attrs: ComWrapper::new(Attrs::default()) }
    }
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        lock(&self.id).as_ptr() as FIDString
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        let mut v = if id.is_null() { Vec::new() } else { unsafe { CStr::from_ptr(id) }.to_bytes().to_vec() };
        v.push(0);
        *lock(&self.id) = v;
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // Borrowed by the caller for the message's life, as the SDK's HostMessage does.
        self.attrs.as_com_ref::<IAttributeList>().map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}

/// What the plug-in's controller tells the host.
#[derive(Default)]
pub struct HandlerState {
    pub edits: Vec<(u32, f64)>,
    pub gestures: Vec<(u32, bool)>,
    pub restart: i32,
}

struct Handler {
    state: Arc<Mutex<HandlerState>>,
}

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, id: ParamID) -> tresult {
        lock(&self.state).gestures.push((id, true));
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        lock(&self.state).edits.push((id, value));
        kResultOk
    }

    unsafe fn endEdit(&self, id: ParamID) -> tresult {
        lock(&self.state).gestures.push((id, false));
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        lock(&self.state).restart |= flags;
        kResultOk
    }
}

/// A memory IBStream.
pub struct Stream {
    data: Mutex<(Vec<u8>, usize)>,
}

impl Stream {
    fn new(bytes: Vec<u8>) -> ComWrapper<Stream> {
        ComWrapper::new(Stream { data: Mutex::new((bytes, 0)) })
    }

    fn bytes(&self) -> Vec<u8> {
        lock(&self.data).0.clone()
    }
}

impl Class for Stream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for Stream {
    unsafe fn read(&self, buffer: *mut c_void, num_bytes: int32, num_read: *mut int32) -> tresult {
        let mut d = lock(&self.data);
        let n = (num_bytes.max(0) as usize).min(d.0.len().saturating_sub(d.1));
        unsafe {
            std::ptr::copy_nonoverlapping(d.0.as_ptr().add(d.1), buffer as *mut u8, n);
            if !num_read.is_null() {
                *num_read = n as i32;
            }
        }
        d.1 += n;
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, num_bytes: int32, num_written: *mut int32) -> tresult {
        let mut d = lock(&self.data);
        let n = num_bytes.max(0) as usize;
        let src = unsafe { std::slice::from_raw_parts(buffer as *const u8, n) };
        let pos = d.1;
        if d.0.len() < pos + n {
            d.0.resize(pos + n, 0);
        }
        d.0[pos..pos + n].copy_from_slice(src);
        d.1 = pos + n;
        if !num_written.is_null() {
            unsafe { *num_written = n as i32 };
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let mut d = lock(&self.data);
        let base = match mode as IBStream_::IStreamSeekMode {
            IBStream_::IStreamSeekMode_::kIBSeekCur => d.1 as i64,
            IBStream_::IStreamSeekMode_::kIBSeekEnd => d.0.len() as i64,
            _ => 0,
        };
        d.1 = (base + pos).clamp(0, d.0.len() as i64) as usize;
        if !result.is_null() {
            unsafe { *result = d.1 as i64 };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        unsafe { *pos = lock(&self.data).1 as i64 };
        kResultOk
    }
}

struct Queue {
    id: u32,
    points: Mutex<Vec<(i32, f64)>>,
}

impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }

    unsafe fn getPointCount(&self) -> int32 {
        lock(&self.points).len() as i32
    }

    unsafe fn getPoint(&self, index: int32, sample_offset: *mut int32, value: *mut ParamValue) -> tresult {
        match lock(&self.points).get(index as usize) {
            Some((o, v)) => {
                unsafe {
                    *sample_offset = *o;
                    *value = *v;
                }
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn addPoint(&self, sample_offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        let mut p = lock(&self.points);
        p.push((sample_offset, value));
        if !index.is_null() {
            unsafe { *index = p.len() as i32 - 1 };
        }
        kResultOk
    }
}

/// Parameter changes for one block, in or out.
#[derive(Default)]
struct Changes {
    queues: Mutex<Vec<ComWrapper<Queue>>>,
}

impl Changes {
    fn add(&self, id: u32, value: f64) {
        let mut q = lock(&self.queues);
        match q.iter().find(|q| q.id == id) {
            Some(q) => lock(&q.points).push((0, value)),
            None => q.push(ComWrapper::new(Queue { id, points: Mutex::new(vec![(0, value)]) })),
        }
    }

    /// Every (id, last value).
    fn values(&self) -> Vec<(u32, f64)> {
        lock(&self.queues).iter().filter_map(|q| lock(&q.points).last().map(|p| (q.id, p.1))).collect()
    }
}

impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        lock(&self.queues).len() as i32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        lock(&self.queues).get(index as usize).and_then(|q| q.as_com_ref::<IParamValueQueue>().map(|r| r.as_ptr())).unwrap_or(std::ptr::null_mut())
    }

    unsafe fn addParameterData(&self, id: *const ParamID, index: *mut int32) -> *mut IParamValueQueue {
        let id = unsafe { *id };
        let mut q = lock(&self.queues);
        let i = match q.iter().position(|q| q.id == id) {
            Some(i) => i,
            None => {
                q.push(ComWrapper::new(Queue { id, points: Mutex::new(Vec::new()) }));
                q.len() - 1
            }
        };
        if !index.is_null() {
            unsafe { *index = i as i32 };
        }
        q[i].as_com_ref::<IParamValueQueue>().map_or(std::ptr::null_mut(), |r| r.as_ptr())
    }
}

/// No events (Milan sends no MIDI); the plug-in's own output events are dropped.
struct NoEvents;

impl Class for NoEvents {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for NoEvents {
    unsafe fn getEventCount(&self) -> int32 {
        0
    }

    unsafe fn getEvent(&self, _index: int32, _e: *mut Event) -> tresult {
        kResultFalse
    }

    unsafe fn addEvent(&self, _e: *mut Event) -> tresult {
        kResultOk
    }
}

/// IPlugFrame: the plug-in asks for a new window size; the editor thread applies it after the current message.
struct Frame {
    wanted: Arc<Mutex<Option<(u32, u32)>>>,
}

impl Class for Frame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, _view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        let r = unsafe { *new_size };
        *lock(&self.wanted) = Some(((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32));
        kResultOk
    }
}

/// The plug-in's view as the editor window drives it.
struct PlugView {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<Frame>,
    wanted: Arc<Mutex<Option<(u32, u32)>>>,
    attached: bool,
}

impl View for PlugView {
    fn attached(&mut self, hwnd: windows_sys::Win32::Foundation::HWND) -> Result<(u32, u32), String> {
        unsafe {
            if !ok(self.view.isPlatformTypeSupported(kPlatformTypeHWND)) {
                return Err("the plug-in's editor does not support a Windows window (HWND)".into());
            }
            let frame = self.frame.as_com_ref::<IPlugFrame>().map_or(std::ptr::null_mut(), |r| r.as_ptr());
            self.view.setFrame(frame);
            let mut r = ViewRect { left: 0, top: 0, right: 400, bottom: 300 };
            self.view.getSize(&mut r);
            if !ok(self.view.attached(hwnd as *mut c_void, kPlatformTypeHWND)) {
                return Err("the plug-in's editor refused to attach to the window".into());
            }
            self.attached = true;
            Ok(((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32))
        }
    }

    fn resized(&mut self, w: u32, h: u32) {
        if self.attached {
            let mut r = ViewRect { left: 0, top: 0, right: w as i32, bottom: h as i32 };
            unsafe { self.view.onSize(&mut r) };
        }
    }

    fn removed(&mut self) {
        if self.attached {
            unsafe {
                self.view.removed();
                self.view.setFrame(std::ptr::null_mut());
            }
            self.attached = false;
        }
    }

    fn wanted(&mut self) -> Option<(u32, u32)> {
        lock(&self.wanted).take()
    }
}

// ---------------------------------------------------------------- the module

struct Instance {
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: ComPtr<IEditController>,
    /// The controller is the component itself (a single-component effect).
    single: bool,
    _handler: ComWrapper<Handler>,
    handler: Arc<Mutex<HandlerState>>,
    params: Vec<AParam>,
    /// What Milan last sent per parameter.
    sent: HashMap<u32, f64>,
    latency: u32,
    tail: u32,
    processing: bool,
    bufs: [Vec<f32>; 4],
    /// Values for the processor's next block (what Milan set, what the editor changed).
    pending: Vec<(u32, f64)>,
    editor: Option<Editor>,
    events: ComWrapper<NoEvents>,
}

pub struct Module {
    _lib: libloading::Library,
    factory: ComPtr<IPluginFactory>,
    host: ComWrapper<HostApp>,
    instances: HashMap<String, Instance>,
}

impl Module {
    pub fn load(path: &Path) -> Result<Module, String> {
        // SAFETY: loading a plug-in binary runs its code; that is this process's purpose.
        unsafe {
            let lib = libloading::Library::new(path).map_err(|e| format!("cannot load {}: {e}", path.display()))?;
            if let Ok(init) = lib.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0") {
                if !init() {
                    return Err("InitDll returned false".into());
                }
            }
            let get: libloading::Symbol<unsafe extern "system" fn() -> *mut IPluginFactory> = lib.get(b"GetPluginFactory\0").map_err(|e| format!("no GetPluginFactory export: {e}"))?;
            let factory = ComPtr::from_raw(get()).ok_or("GetPluginFactory returned NULL")?;
            let host = ComWrapper::new(HostApp);
            if let Some(f3) = factory.cast::<IPluginFactory3>() {
                f3.setHostContext(host.as_com_ref::<IHostApplication>().expect("host").as_ptr() as *mut FUnknown);
            }
            Ok(Module { _lib: lib, factory, host, instances: HashMap::new() })
        }
    }

    fn host_ptr(&self) -> *mut FUnknown {
        self.host.as_com_ref::<IHostApplication>().expect("host").as_ptr() as *mut FUnknown
    }

    /// The audio effect classes (described without instances, then each set up once for its parameters).
    pub fn scan(&mut self) -> Vec<AudioDesc> {
        let mut out = Vec::new();
        unsafe {
            let mut finfo: PFactoryInfo = std::mem::zeroed();
            self.factory.getFactoryInfo(&mut finfo);
            let vendor = ascii(&finfo.vendor);
            let f2 = self.factory.cast::<IPluginFactory2>();
            for i in 0..self.factory.countClasses() {
                let mut info: PClassInfo = std::mem::zeroed();
                if !ok(self.factory.getClassInfo(i, &mut info)) || ascii(&info.category) != AUDIO_CLASS {
                    continue;
                }
                let mut d = AudioDesc { format: "vst3".into(), id: cid_hex(&info.cid), name: ascii(&info.name), vendor: vendor.clone(), ..Default::default() };
                if let Some(f2) = &f2 {
                    let mut i2: PClassInfo2 = std::mem::zeroed();
                    if ok(f2.getClassInfo2(i, &mut i2)) {
                        d.category = ascii(&i2.subCategories);
                        d.version = ascii(&i2.version);
                        d.sdk = ascii(&i2.sdkVersion);
                        let v = ascii(&i2.vendor);
                        if !v.is_empty() {
                            d.vendor = v;
                        }
                    }
                }
                if !d.category.is_empty() && !d.category.starts_with("Fx") {
                    d.error = format!("it is a {} plug-in: Milan Pro runs audio effects (Fx)", d.category.split('|').next().unwrap_or_default());
                    out.push(d);
                    continue;
                }
                let probe = AInstance { id: d.id.clone(), instance: format!("scan-{}", d.id), rate: 48_000.0, block: 1024, ..Default::default() };
                match self.make(&probe) {
                    Ok(inst) => {
                        d.params = inst.params.clone();
                        d.latency = inst.latency;
                        d.tail = inst.tail;
                        d.has_editor = {
                            let v = inst.controller.createView(ViewType::kEditor);
                            match ComPtr::from_raw(v) {
                                Some(v) => ok(v.isPlatformTypeSupported(kPlatformTypeHWND)),
                                None => false,
                            }
                        };
                        Self::destroy(inst);
                    }
                    Err(e) => d.error = e,
                }
                out.push(d);
            }
        }
        out
    }

    fn class_of(&self, id: &str) -> Option<TUID> {
        let want = hex_cid(id)?;
        unsafe {
            for i in 0..self.factory.countClasses() {
                let mut info: PClassInfo = std::mem::zeroed();
                if ok(self.factory.getClassInfo(i, &mut info)) && info.cid == want {
                    return Some(info.cid);
                }
            }
        }
        None
    }

    /// Make an instance of class `inst.id`, set up for stereo float at `inst.rate`, with its state and values.
    fn make(&self, inst: &AInstance) -> Result<Instance, String> {
        let cid = self.class_of(&inst.id).ok_or_else(|| format!("the module has no class {}", inst.id))?;
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();
            if !ok(self.factory.createInstance(cid.as_ptr(), IComponent_iid.as_ptr(), &mut obj)) || obj.is_null() {
                return Err("createInstance(IComponent) failed".into());
            }
            let component = ComPtr::from_raw(obj as *mut IComponent).ok_or("no component")?;
            if !ok(component.initialize(self.host_ptr())) {
                return Err("IComponent::initialize failed".into());
            }
            let processor = component.cast::<IAudioProcessor>().ok_or("the component has no IAudioProcessor")?;
            let mut ccid: TUID = [0; 16];
            let (controller, single) = if ok(component.getControllerClassId(&mut ccid)) && ccid != [0; 16] && ccid != cid {
                let mut obj: *mut c_void = std::ptr::null_mut();
                if !ok(self.factory.createInstance(ccid.as_ptr(), IEditController_iid.as_ptr(), &mut obj)) || obj.is_null() {
                    return Err("createInstance(IEditController) failed".into());
                }
                let c = ComPtr::from_raw(obj as *mut IEditController).ok_or("no controller")?;
                if !ok(c.initialize(self.host_ptr())) {
                    return Err("IEditController::initialize failed".into());
                }
                (c, false)
            } else {
                (component.cast::<IEditController>().ok_or("the component has no IEditController and names no controller class")?, true)
            };
            let state = Arc::new(Mutex::new(HandlerState::default()));
            let handler = ComWrapper::new(Handler { state: state.clone() });
            controller.setComponentHandler(handler.as_com_ref::<IComponentHandler>().expect("handler").as_ptr());
            if !single {
                if let (Some(a), Some(b)) = (component.cast::<IConnectionPoint>(), controller.cast::<IConnectionPoint>()) {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
            }
            // Buses: the main stereo pair in and out; everything else stays off.
            let (ins, outs) = (component.getBusCount(MediaTypes_::kAudio as MediaType, BusDirections_::kInput as BusDirection), component.getBusCount(MediaTypes_::kAudio as MediaType, BusDirections_::kOutput as BusDirection));
            if ins < 1 || outs < 1 {
                return Err(format!("it has {ins} audio input and {outs} output buses: Milan Pro runs stereo effects"));
            }
            let mut arr_in = [SpeakerArr::kStereo];
            let mut arr_out = [SpeakerArr::kStereo];
            processor.setBusArrangements(arr_in.as_mut_ptr(), 1, arr_out.as_mut_ptr(), 1);
            let (mut a_in, mut a_out) = (0u64, 0u64);
            processor.getBusArrangement(BusDirections_::kInput as BusDirection, 0, &mut a_in);
            processor.getBusArrangement(BusDirections_::kOutput as BusDirection, 0, &mut a_out);
            if a_in.count_ones() != 2 || a_out.count_ones() != 2 {
                return Err(format!("its main buses are {} in, {} out: Milan Pro runs stereo effects", a_in.count_ones(), a_out.count_ones()));
            }
            if !ok(processor.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as i32)) {
                return Err("it cannot process 32-bit float samples".into());
            }
            component.activateBus(MediaTypes_::kAudio as MediaType, BusDirections_::kInput as BusDirection, 0, 1);
            component.activateBus(MediaTypes_::kAudio as MediaType, BusDirections_::kOutput as BusDirection, 0, 1);
            for k in 1..ins {
                component.activateBus(MediaTypes_::kAudio as MediaType, BusDirections_::kInput as BusDirection, k, 0);
            }
            for k in 1..outs {
                component.activateBus(MediaTypes_::kAudio as MediaType, BusDirections_::kOutput as BusDirection, k, 0);
            }
            let mut setup = ProcessSetup { processMode: ProcessModes_::kRealtime as i32, symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32, maxSamplesPerBlock: inst.block.max(1) as i32, sampleRate: inst.rate };
            if !ok(processor.setupProcessing(&mut setup)) {
                return Err(format!("setupProcessing({} Hz, {} frames, 32-bit float) failed", inst.rate, inst.block));
            }
            let mut me = Instance {
                component,
                processor,
                controller,
                single,
                _handler: handler,
                handler: state,
                params: Vec::new(),
                sent: HashMap::new(),
                latency: 0,
                tail: 0,
                processing: false,
                bufs: Default::default(),
                pending: Vec::new(),
                editor: None,
                events: ComWrapper::new(NoEvents),
            };
            me.params = me.read_params();
            // The controller learns the component's starting state (the SDK's way of syncing the two).
            if !single {
                let s = Stream::new(Vec::new());
                if ok(me.component.getState(s.as_com_ref::<IBStream>().expect("stream").as_ptr())) {
                    let s = Stream::new(s.bytes());
                    me.controller.setComponentState(s.as_com_ref::<IBStream>().expect("stream").as_ptr());
                }
            }
            if let Some(st) = &inst.state {
                me.load_state(st);
            }
            // What the plug-in holds now counts as sent: Milan's values reach it only where they differ (a program
            // change parameter at its default must not reset the program).
            me.values();
            for &(id, v) in &inst.params {
                me.set_param(id, v);
            }
            me.activate(true);
            Ok(me)
        }
    }

    fn destroy(mut inst: Instance) {
        if let Some(e) = inst.editor.take() {
            e.close();
        }
        unsafe {
            inst.activate(false);
            if !inst.single {
                if let (Some(a), Some(b)) = (inst.component.cast::<IConnectionPoint>(), inst.controller.cast::<IConnectionPoint>()) {
                    a.disconnect(b.as_ptr());
                    b.disconnect(a.as_ptr());
                }
                inst.controller.terminate();
            }
            inst.component.terminate();
        }
    }

    fn get(&mut self, inst: &AInstance) -> Result<&mut Instance, String> {
        if !self.instances.contains_key(&inst.instance) {
            let made = self.make(inst)?;
            self.instances.insert(inst.instance.clone(), made);
        } else if inst.reset {
            // (A state sent along is only for making the instance; `ASetState` replaces a living one's.)
            let me = self.instances.get_mut(&inst.instance).expect("present");
            unsafe {
                me.activate(false);
                me.activate(true);
            }
        }
        let me = self.instances.get_mut(&inst.instance).expect("present");
        for &(id, v) in &inst.params {
            me.set_param(id, v);
        }
        Ok(me)
    }

    pub fn open(&mut self, inst: &AInstance) -> Result<AOut, String> {
        let fresh = !self.instances.contains_key(&inst.instance);
        let me = self.get(inst)?;
        let mut out = me.out();
        if fresh {
            out.values = Some(me.values());
        }
        Ok(out)
    }

    pub fn process(&mut self, p: &AProcess, shm: &mut [u8]) -> Result<AOut, String> {
        let me = self.get(&p.inst)?;
        me.process(p, shm)
    }

    pub fn get_state(&mut self, inst: &AInstance) -> Result<AState, String> {
        let me = self.get(inst)?;
        Ok(me.state())
    }

    pub fn set_state(&mut self, inst: &AInstance, st: &AState) -> Result<AOut, String> {
        let me = self.get(inst)?;
        me.load_state(st);
        let mut out = me.out();
        out.values = Some(me.values());
        Ok(out)
    }

    pub fn editor(&mut self, inst: &AInstance, op: &EditorOp) -> Result<AOut, String> {
        let me = self.get(inst)?;
        match op {
            EditorOp::Open { x, y, offscreen, title } => {
                if me.editor.as_ref().is_some_and(|e| e.state().open) {
                    return Ok(me.out());
                }
                if let Some(e) = me.editor.take() {
                    e.close();
                }
                let v = unsafe { me.controller.createView(ViewType::kEditor) };
                let view = unsafe { ComPtr::from_raw(v) }.ok_or("the plug-in has no editor")?;
                let wanted = Arc::new(Mutex::new(None));
                let frame = ComWrapper::new(Frame { wanted: wanted.clone() });
                let pv = PlugView { view, frame, wanted, attached: false };
                me.editor = Some(Editor::open(Box::new(pv), title, *x, *y, *offscreen)?);
            }
            EditorOp::Close => {
                if let Some(e) = me.editor.take() {
                    e.close();
                }
            }
            EditorOp::Poll => {}
        }
        let mut out = me.out();
        if let Some(e) = &me.editor {
            if let Some(err) = e.error() {
                out.messages.push(err);
            }
        }
        if !out.editor.as_ref().is_some_and(|e| e.open) {
            // Closed (by the user or Milan): the state it left behind travels with this reply.
            if me.editor.is_some() {
                if let Some(e) = me.editor.take() {
                    e.close();
                }
            }
        }
        Ok(out)
    }

    pub fn close(&mut self, instance: &str) {
        if let Some(i) = self.instances.remove(instance) {
            Self::destroy(i);
        }
    }

    pub fn shutdown(&mut self) {
        for (_, i) in self.instances.drain() {
            Self::destroy(i);
        }
    }
}

impl Instance {
    unsafe fn activate(&mut self, on: bool) {
        unsafe {
            if on && !self.processing {
                self.component.setActive(1);
                self.latency = self.processor.getLatencySamples();
                self.tail = self.processor.getTailSamples();
                self.processor.setProcessing(1);
                self.processing = true;
            } else if !on && self.processing {
                self.processor.setProcessing(0);
                self.component.setActive(0);
                self.processing = false;
            }
        }
    }

    /// Every parameter with its names and (program lists) options.
    fn read_params(&self) -> Vec<AParam> {
        let mut out = Vec::new();
        unsafe {
            // Units and their program lists (IUnitInfo), for program change parameters.
            let mut lists: HashMap<i32, Vec<String>> = HashMap::new();
            let mut unit_list: HashMap<i32, i32> = HashMap::new();
            if let Some(u) = self.controller.cast::<IUnitInfo>() {
                for i in 0..u.getProgramListCount() {
                    let mut info: ProgramListInfo = std::mem::zeroed();
                    if ok(u.getProgramListInfo(i, &mut info)) {
                        let names = (0..info.programCount.clamp(0, 1024))
                            .map(|k| {
                                let mut n: String128 = [0; 128];
                                if ok(u.getProgramName(info.id, k, &mut n)) { utf16(&n) } else { format!("Program {}", k + 1) }
                            })
                            .collect();
                        lists.insert(info.id, names);
                    }
                }
                for i in 0..u.getUnitCount() {
                    let mut info: UnitInfo = std::mem::zeroed();
                    if ok(u.getUnitInfo(i, &mut info)) {
                        unit_list.insert(info.id, info.programListId);
                    }
                }
            }
            for i in 0..self.controller.getParameterCount() {
                let mut info: ParameterInfo = std::mem::zeroed();
                if !ok(self.controller.getParameterInfo(i, &mut info)) {
                    continue;
                }
                let flags = info.flags;
                let f = |bit: i32| flags & bit != 0;
                let program = f(ParameterInfo_::ParameterFlags_::kIsProgramChange);
                let mut p = AParam {
                    id: info.id,
                    name: utf16(&info.title),
                    units: utf16(&info.units),
                    steps: info.stepCount.max(0),
                    min: 0.0,
                    max: 1.0,
                    default: info.defaultNormalizedValue.clamp(0.0, 1.0),
                    automatable: f(ParameterInfo_::ParameterFlags_::kCanAutomate),
                    hidden: f(ParameterInfo_::ParameterFlags_::kIsHidden),
                    bypass: f(ParameterInfo_::ParameterFlags_::kIsBypass),
                    readonly: f(ParameterInfo_::ParameterFlags_::kIsReadOnly),
                    options: Vec::new(),
                    program,
                };
                if program {
                    if let Some(names) = unit_list.get(&info.unitId).and_then(|l| lists.get(l)) {
                        p.options = names.clone();
                        p.steps = (names.len() as i32 - 1).max(0);
                    }
                }
                if p.options.is_empty() && (1..=64).contains(&p.steps) && (f(ParameterInfo_::ParameterFlags_::kIsList) || p.steps <= 16) {
                    for k in 0..=p.steps {
                        let mut s: String128 = [0; 128];
                        let v = k as f64 / p.steps as f64;
                        let text = if ok(self.controller.getParamStringByValue(info.id, v, &mut s)) { utf16(&s) } else { String::new() };
                        p.options.push(if text.is_empty() { format!("{}", k) } else { text });
                    }
                }
                out.push(p);
            }
        }
        out
    }

    fn set_param(&mut self, id: u32, v: f64) {
        if self.sent.get(&id).is_some_and(|x| *x == v) {
            return;
        }
        let v = v.clamp(0.0, 1.0);
        unsafe { self.controller.setParamNormalized(id, v) };
        self.sent.insert(id, v);
        // The processor hears it with the next block.
        self.pending.push((id, v));
    }

    /// Every parameter's value as the controller holds it.
    fn values(&mut self) -> Vec<(u32, f64)> {
        let v: Vec<(u32, f64)> = self.params.iter().map(|p| (p.id, unsafe { self.controller.getParamNormalized(p.id) })).collect();
        for &(id, x) in &v {
            self.sent.insert(id, x);
        }
        v
    }

    /// Values set since the last block reach the processor (a `process` of no samples, as the SDK allows for parameter
    /// flushing), so its state holds them.
    fn flush(&mut self) {
        if self.pending.is_empty() || !self.processing {
            return;
        }
        let changes = ComWrapper::new(Changes::default());
        for (id, v) in self.pending.drain(..) {
            changes.add(id, v);
        }
        let out_changes = ComWrapper::new(Changes::default());
        let events = self.events.as_com_ref::<IEventList>().expect("events").as_ptr();
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: 0,
            numInputs: 0,
            numOutputs: 0,
            inputs: std::ptr::null_mut(),
            outputs: std::ptr::null_mut(),
            inputParameterChanges: changes.as_com_ref::<IParameterChanges>().expect("changes").as_ptr(),
            outputParameterChanges: out_changes.as_com_ref::<IParameterChanges>().expect("changes").as_ptr(),
            inputEvents: events,
            outputEvents: events,
            processContext: std::ptr::null_mut(),
        };
        unsafe { self.processor.process(&mut data) };
    }

    fn state(&mut self) -> AState {
        self.flush();
        let comp = Stream::new(Vec::new());
        let ctrl = Stream::new(Vec::new());
        unsafe {
            self.component.getState(comp.as_com_ref::<IBStream>().expect("stream").as_ptr());
            self.controller.getState(ctrl.as_com_ref::<IBStream>().expect("stream").as_ptr());
        }
        AState { component: b64(&comp.bytes()), controller: b64(&ctrl.bytes()) }
    }

    fn load_state(&mut self, st: &AState) {
        let comp = unb64(&st.component);
        if !comp.is_empty() {
            let s = Stream::new(comp.clone());
            unsafe { self.component.setState(s.as_com_ref::<IBStream>().expect("stream").as_ptr()) };
            if !self.single {
                let s = Stream::new(comp);
                unsafe { self.controller.setComponentState(s.as_com_ref::<IBStream>().expect("stream").as_ptr()) };
            }
        }
        let ctrl = unb64(&st.controller);
        if !ctrl.is_empty() {
            let s = Stream::new(ctrl);
            unsafe { self.controller.setState(s.as_com_ref::<IBStream>().expect("stream").as_ptr()) };
        }
        // Milan's values are stale now: the next ones it sends are applied whatever they are.
        self.sent.clear();
        self.pending.clear();
    }

    /// What this instance reports: latency, edits made in its editor, gestures, the editor's state.
    fn out(&mut self) -> AOut {
        let (edits, gestures, restart) = {
            let mut h = lock(&self.handler);
            (std::mem::take(&mut h.edits), std::mem::take(&mut h.gestures), std::mem::take(&mut h.restart))
        };
        let mut out = AOut { latency: self.latency, tail: self.tail, edits, gestures, ..Default::default() };
        if restart & (RestartFlags_::kLatencyChanged | RestartFlags_::kIoChanged) != 0 {
            unsafe {
                self.activate(false);
                self.activate(true);
            }
            out.latency = self.latency;
            out.tail = self.tail;
        }
        if restart & RestartFlags_::kParamValuesChanged != 0 {
            out.values = Some(self.values());
        }
        for &(id, v) in &out.edits {
            self.sent.insert(id, v);
            self.pending.push((id, v));
        }
        out.editor = self.editor.as_ref().map(|e| e.state());
        out
    }

    fn process(&mut self, p: &AProcess, shm: &mut [u8]) -> Result<AOut, String> {
        let n = p.frames as usize;
        if shm.len() < n * 16 {
            return Err("the audio memory is too small for the block".into());
        }
        let input: &[f32] = bytemuck::cast_slice(&shm[..n * 8]);
        for b in &mut self.bufs {
            b.clear();
            b.resize(n, 0.0);
        }
        for (k, fr) in input.chunks_exact(2).enumerate() {
            (self.bufs[0][k], self.bufs[1][k]) = (fr[0], fr[1]);
        }
        let changes = ComWrapper::new(Changes::default());
        for (id, v) in self.pending.drain(..) {
            changes.add(id, v);
        }
        let out_changes = ComWrapper::new(Changes::default());
        let mut ctx: ProcessContext = unsafe { std::mem::zeroed() };
        ctx.sampleRate = p.inst.rate;
        ctx.projectTimeSamples = (p.time * p.inst.rate).round() as i64;
        ctx.continousTimeSamples = ctx.projectTimeSamples;
        ctx.tempo = 120.0;
        (ctx.timeSigNumerator, ctx.timeSigDenominator) = (4, 4);
        ctx.projectTimeMusic = p.time * 2.0;
        ctx.state = (ProcessContext_::StatesAndFlags_::kTempoValid | ProcessContext_::StatesAndFlags_::kTimeSigValid | ProcessContext_::StatesAndFlags_::kProjectTimeMusicValid | ProcessContext_::StatesAndFlags_::kContTimeValid) as u32;
        if p.playing {
            ctx.state |= ProcessContext_::StatesAndFlags_::kPlaying as u32;
        }
        let mut in_ch = [self.bufs[0].as_mut_ptr(), self.bufs[1].as_mut_ptr()];
        let mut out_ch = [self.bufs[2].as_mut_ptr(), self.bufs[3].as_mut_ptr()];
        let mut inputs = AudioBusBuffers { numChannels: 2, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: in_ch.as_mut_ptr() } };
        let mut outputs = AudioBusBuffers { numChannels: 2, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: out_ch.as_mut_ptr() } };
        let events = self.events.as_com_ref::<IEventList>().expect("events").as_ptr();
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: n as i32,
            numInputs: 1,
            numOutputs: 1,
            inputs: &mut inputs,
            outputs: &mut outputs,
            inputParameterChanges: changes.as_com_ref::<IParameterChanges>().expect("changes").as_ptr(),
            outputParameterChanges: out_changes.as_com_ref::<IParameterChanges>().expect("changes").as_ptr(),
            inputEvents: events,
            outputEvents: events,
            processContext: &mut ctx,
        };
        let r = unsafe { self.processor.process(&mut data) };
        if !ok(r) {
            return Err(format!("process returned {r}"));
        }
        let output: &mut [f32] = bytemuck::cast_slice_mut(&mut shm[n * 8..n * 16]);
        for (k, fr) in output.chunks_exact_mut(2).enumerate() {
            (fr[0], fr[1]) = (self.bufs[2][k], self.bufs[3][k]);
        }
        let mut out = self.out();
        // Values the processor changed itself (not meters: read-only ones are left out).
        for (id, v) in out_changes.values() {
            if self.params.iter().any(|q| q.id == id && !q.readonly) && self.sent.get(&id) != Some(&v) {
                unsafe { self.controller.setParamNormalized(id, v) };
                self.sent.insert(id, v);
                out.edits.push((id, v));
            }
        }
        Ok(out)
    }
}
