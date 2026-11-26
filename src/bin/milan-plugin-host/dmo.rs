//! R35 research spike: can a DirectX Media Object audio effect (the format VEGAS's `Audio_x64` plug-ins use) run
//! outside VEGAS? Enumerates the registered DMO audio effects (`DMOEnum`, read-only), makes the first one with
//! `CoCreateInstance`, asks it for 32-bit float stereo 48 kHz in and out and pushes one block through
//! `IMediaObject::ProcessInput` / `ProcessOutput`. Everything is recorded as text for `docs/compat/dx-audio.md`;
//! nothing is kept, registered or shown. Milan Pro ships no DX path (see the report).
//!
//! `IEnumDMO`, `IMediaObject` and `IMediaBuffer` are written out here from `mediaobj.h` (windows-sys 0.59 has the
//! functions and constants of `dmo.h` but not these COM interfaces).
#![allow(non_snake_case, clippy::missing_safety_doc)]
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{BOOL, E_NOINTERFACE, S_OK};
use windows_sys::Win32::Media::DxMediaObjects::*;
use windows_sys::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows_sys::core::{GUID, HRESULT, PWSTR};

type Unknown = *mut c_void;

#[repr(C)]
struct IUnknownVtbl {
    QueryInterface: unsafe extern "system" fn(Unknown, *const GUID, *mut *mut c_void) -> HRESULT,
    AddRef: unsafe extern "system" fn(Unknown) -> u32,
    Release: unsafe extern "system" fn(Unknown) -> u32,
}

#[repr(C)]
struct IEnumDMOVtbl {
    base: IUnknownVtbl,
    Next: unsafe extern "system" fn(Unknown, u32, *mut GUID, *mut PWSTR, *mut u32) -> HRESULT,
    Skip: unsafe extern "system" fn(Unknown, u32) -> HRESULT,
    Reset: unsafe extern "system" fn(Unknown) -> HRESULT,
    Clone: unsafe extern "system" fn(Unknown, *mut Unknown) -> HRESULT,
}

#[repr(C)]
struct IMediaObjectVtbl {
    base: IUnknownVtbl,
    GetStreamCount: unsafe extern "system" fn(Unknown, *mut u32, *mut u32) -> HRESULT,
    GetInputStreamInfo: unsafe extern "system" fn(Unknown, u32, *mut u32) -> HRESULT,
    GetOutputStreamInfo: unsafe extern "system" fn(Unknown, u32, *mut u32) -> HRESULT,
    GetInputType: unsafe extern "system" fn(Unknown, u32, u32, *mut DMO_MEDIA_TYPE) -> HRESULT,
    GetOutputType: unsafe extern "system" fn(Unknown, u32, u32, *mut DMO_MEDIA_TYPE) -> HRESULT,
    SetInputType: unsafe extern "system" fn(Unknown, u32, *const DMO_MEDIA_TYPE, u32) -> HRESULT,
    SetOutputType: unsafe extern "system" fn(Unknown, u32, *const DMO_MEDIA_TYPE, u32) -> HRESULT,
    GetInputCurrentType: unsafe extern "system" fn(Unknown, u32, *mut DMO_MEDIA_TYPE) -> HRESULT,
    GetOutputCurrentType: unsafe extern "system" fn(Unknown, u32, *mut DMO_MEDIA_TYPE) -> HRESULT,
    GetInputSizeInfo: unsafe extern "system" fn(Unknown, u32, *mut u32, *mut u32, *mut u32) -> HRESULT,
    GetOutputSizeInfo: unsafe extern "system" fn(Unknown, u32, *mut u32, *mut u32) -> HRESULT,
    GetInputMaxLatency: unsafe extern "system" fn(Unknown, u32, *mut i64) -> HRESULT,
    SetInputMaxLatency: unsafe extern "system" fn(Unknown, u32, i64) -> HRESULT,
    Flush: unsafe extern "system" fn(Unknown) -> HRESULT,
    Discontinuity: unsafe extern "system" fn(Unknown, u32) -> HRESULT,
    AllocateStreamingResources: unsafe extern "system" fn(Unknown) -> HRESULT,
    FreeStreamingResources: unsafe extern "system" fn(Unknown) -> HRESULT,
    GetInputStatus: unsafe extern "system" fn(Unknown, u32, *mut u32) -> HRESULT,
    ProcessInput: unsafe extern "system" fn(Unknown, u32, Unknown, u32, i64, i64) -> HRESULT,
    ProcessOutput: unsafe extern "system" fn(Unknown, u32, u32, *mut DMO_OUTPUT_DATA_BUFFER, *mut u32) -> HRESULT,
    Lock: unsafe extern "system" fn(Unknown, i32) -> HRESULT,
}

#[repr(C)]
struct IMediaBufferVtbl {
    base: IUnknownVtbl,
    SetLength: unsafe extern "system" fn(Unknown, u32) -> HRESULT,
    GetMaxLength: unsafe extern "system" fn(Unknown, *mut u32) -> HRESULT,
    GetBufferAndLength: unsafe extern "system" fn(Unknown, *mut *mut u8, *mut u32) -> HRESULT,
}

/// Our IMediaBuffer: a fixed byte buffer with a length.
#[repr(C)]
struct Buffer {
    vtbl: *const IMediaBufferVtbl,
    refs: u32,
    data: Vec<u8>,
    len: u32,
}

const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const IID_IMEDIAOBJECT: GUID = GUID::from_u128(0xd8ad0f58_5494_4102_97c5_ec798e59bcf4);
const IID_IMEDIABUFFER: GUID = GUID::from_u128(0x59eff8b9_938c_4a26_82f2_95cb84cdc837);
const MEDIATYPE_AUDIO: GUID = GUID::from_u128(0x73647561_0000_0010_8000_00aa00389b71);
const MEDIASUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const FORMAT_WAVEFORMATEX: GUID = GUID::from_u128(0x05589f81_c356_11ce_bf01_00aa0055595a);

fn same(a: &GUID, b: &GUID) -> bool {
    (a.data1, a.data2, a.data3, a.data4) == (b.data1, b.data2, b.data3, b.data4)
}

unsafe extern "system" fn buf_qi(this: Unknown, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe {
        if same(&*iid, &IID_IUNKNOWN) || same(&*iid, &IID_IMEDIABUFFER) {
            *out = this;
            (*(this as *mut Buffer)).refs += 1;
            S_OK
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn buf_addref(this: Unknown) -> u32 {
    unsafe {
        (*(this as *mut Buffer)).refs += 1;
        (*(this as *mut Buffer)).refs
    }
}

unsafe extern "system" fn buf_release(this: Unknown) -> u32 {
    // The buffers live on the probe's stack frame (`Box`ed below); the DMO's references never outlive the call.
    unsafe {
        let b = this as *mut Buffer;
        (*b).refs = (*b).refs.saturating_sub(1);
        (*b).refs
    }
}

unsafe extern "system" fn buf_set_length(this: Unknown, len: u32) -> HRESULT {
    unsafe {
        let b = &mut *(this as *mut Buffer);
        if len as usize > b.data.len() {
            return 0x80070057u32 as HRESULT; // E_INVALIDARG
        }
        b.len = len;
    }
    S_OK
}

unsafe extern "system" fn buf_max(this: Unknown, out: *mut u32) -> HRESULT {
    unsafe { *out = (*(this as *mut Buffer)).data.len() as u32 };
    S_OK
}

unsafe extern "system" fn buf_get(this: Unknown, data: *mut *mut u8, len: *mut u32) -> HRESULT {
    unsafe {
        let b = &mut *(this as *mut Buffer);
        if !data.is_null() {
            *data = b.data.as_mut_ptr();
        }
        if !len.is_null() {
            *len = b.len;
        }
    }
    S_OK
}

static BUFFER_VTBL: IMediaBufferVtbl = IMediaBufferVtbl { base: IUnknownVtbl { QueryInterface: buf_qi, AddRef: buf_addref, Release: buf_release }, SetLength: buf_set_length, GetMaxLength: buf_max, GetBufferAndLength: buf_get };

#[repr(C)]
struct WaveFormatEx {
    tag: u16,
    channels: u16,
    rate: u32,
    bytes_per_sec: u32,
    align: u16,
    bits: u16,
    extra: u16,
}

fn hr(h: HRESULT) -> String {
    format!("0x{:08X}", h as u32)
}

/// The findings, one line each.
pub fn probe() -> Vec<String> {
    let mut log = Vec::new();
    // SAFETY: COM calls on this thread; every interface pointer is released before the function returns.
    unsafe {
        let init = CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32);
        let mut e: Unknown = std::ptr::null_mut();
        let h = DMOEnum(&DMOCATEGORY_AUDIO_EFFECT, DMO_ENUMF_INCLUDE_KEYED as u32, 0, std::ptr::null(), 0, std::ptr::null(), &mut e);
        if h != S_OK || e.is_null() {
            log.push(format!("DMOEnum(DMOCATEGORY_AUDIO_EFFECT) failed: {}", hr(h)));
        } else {
            let vt = &**(e as *mut *const IEnumDMOVtbl);
            let mut found: Vec<(GUID, String)> = Vec::new();
            loop {
                let (mut clsid, mut name, mut n) = (GUID::from_u128(0), std::ptr::null_mut::<u16>(), 0u32);
                if (vt.Next)(e, 1, &mut clsid, &mut name, &mut n) != S_OK || n == 0 {
                    break;
                }
                let mut s = String::new();
                if !name.is_null() {
                    let mut i = 0;
                    while *name.add(i) != 0 {
                        i += 1;
                    }
                    s = String::from_utf16_lossy(std::slice::from_raw_parts(name, i));
                    CoTaskMemFree(name as *const c_void);
                }
                found.push((clsid, s));
            }
            (vt.base.Release)(e);
            log.push(format!("DMO audio effects registered (DMOCATEGORY_AUDIO_EFFECT): {}", found.len()));
            for (g, name) in &found {
                log.push(format!("  {{{:08X}-{:04X}-{:04X}-{}}} {name}", g.data1, g.data2, g.data3, g.data4.iter().map(|b| format!("{b:02X}")).collect::<String>()));
            }
            if let Some((clsid, name)) = found.first() {
                log.extend(try_one(clsid, name));
            }
        }
        if init == S_OK || init == 1 {
            CoUninitialize();
        }
    }
    log
}

/// Make DMO `clsid`, set float stereo 48 kHz in and out, process one block of a -6 dBFS sine.
unsafe fn try_one(clsid: &GUID, name: &str) -> Vec<String> {
    let mut log = vec![format!("Trying \"{name}\" outside its host:")];
    unsafe {
        let mut obj: Unknown = std::ptr::null_mut();
        let h = CoCreateInstance(clsid, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_IMEDIAOBJECT, &mut obj);
        if h != S_OK || obj.is_null() {
            log.push(format!("  CoCreateInstance(IMediaObject) failed: {}", hr(h)));
            return log;
        }
        log.push("  CoCreateInstance(IMediaObject): OK".into());
        let vt = &**(obj as *mut *const IMediaObjectVtbl);
        let (mut ins, mut outs) = (0u32, 0u32);
        (vt.GetStreamCount)(obj, &mut ins, &mut outs);
        log.push(format!("  streams: {ins} in, {outs} out"));
        let wf = WaveFormatEx { tag: 3, channels: 2, rate: 48_000, bytes_per_sec: 48_000 * 8, align: 8, bits: 32, extra: 0 };
        let mt = DMO_MEDIA_TYPE { majortype: MEDIATYPE_AUDIO, subtype: MEDIASUBTYPE_IEEE_FLOAT, bFixedSizeSamples: 1 as BOOL, bTemporalCompression: 0 as BOOL, lSampleSize: 8, formattype: FORMAT_WAVEFORMATEX, pUnk: std::ptr::null_mut(), cbFormat: std::mem::size_of::<WaveFormatEx>() as u32, pbFormat: &wf as *const WaveFormatEx as *mut u8 };
        let hi = (vt.SetInputType)(obj, 0, &mt, 0);
        let ho = (vt.SetOutputType)(obj, 0, &mt, 0);
        log.push(format!("  SetInputType(float stereo 48 kHz): {}; SetOutputType: {}", if hi == S_OK { "OK".to_string() } else { hr(hi) }, if ho == S_OK { "OK".to_string() } else { hr(ho) }));
        if hi == S_OK && ho == S_OK {
            let ha = (vt.AllocateStreamingResources)(obj);
            log.push(format!("  AllocateStreamingResources: {}", if ha == S_OK { "OK".to_string() } else { hr(ha) }));
            let n = 1024usize;
            let mut pcm = vec![0f32; n * 2];
            for (k, fr) in pcm.chunks_exact_mut(2).enumerate() {
                let v = 0.5 * (std::f64::consts::TAU * 1000.0 * k as f64 / 48_000.0).sin() as f32;
                (fr[0], fr[1]) = (v, v);
            }
            let bytes: Vec<u8> = bytemuck::cast_slice(&pcm).to_vec();
            let mut input = Box::new(Buffer { vtbl: &BUFFER_VTBL, refs: 1, len: bytes.len() as u32, data: bytes });
            let mut output = Box::new(Buffer { vtbl: &BUFFER_VTBL, refs: 1, len: 0, data: vec![0u8; n * 8 * 4] });
            let hp = (vt.ProcessInput)(obj, 0, &mut *input as *mut Buffer as Unknown, DMO_INPUT_DATA_BUFFERF_SYNCPOINT as u32, 0, 0);
            let mut odb = DMO_OUTPUT_DATA_BUFFER { pBuffer: &mut *output as *mut Buffer as *mut c_void, dwStatus: 0, rtTimestamp: 0, rtTimelength: 0 };
            let mut status = 0u32;
            let hq = (vt.ProcessOutput)(obj, 0, 1, &mut odb, &mut status);
            let got = output.len as usize / 4;
            let out: &[f32] = bytemuck::cast_slice(&output.data[..got * 4]);
            let rms = |v: &[f32]| (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt();
            log.push(format!("  ProcessInput: {}; ProcessOutput: {} ({} frames out, input RMS {:.3}, output RMS {:.3})", if hp == S_OK { "OK".to_string() } else { hr(hp) }, if hq == S_OK { "OK".to_string() } else { hr(hq) }, got / 2, rms(&pcm), rms(out)));
            log.push(if hp == S_OK && hq == S_OK && got > 0 { "  Result: the DMO processes audio outside its host.".into() } else { "  Result: the DMO did not process audio here.".into() });
            (vt.FreeStreamingResources)(obj);
        }
        (vt.base.Release)(obj);
    }
    log
}
