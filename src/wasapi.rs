//! R9: WASAPI exclusive-mode output (Preferences › Audio Device › Windows Audio (Exclusive Mode); Milan Pro's own, off
//! by default), which cpal 0.15 does not offer. The device runs at its smallest period, event driven, on a
//! "milan-wasapi" thread that renders through the same callback as the shared path. It stands in for ASIO, whose SDK
//! licence forbids shipping it.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;
use windows::Win32::Devices::Properties::DEVPKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, S_OK, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::{KernelStreaming, Multimedia};
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, STGM_READ};
use windows::Win32::System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::core::{PCWSTR, w};

/// The running exclusive stream; dropping it stops the thread (within one 200 ms wait) and releases the device.
pub struct Exclusive {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Exclusive {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The device sample formats Milan Pro writes, best first.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fmt {
    F32,
    I32,
    I24,
    I16,
}

impl Fmt {
    fn bytes(self) -> usize {
        match self {
            Fmt::F32 | Fmt::I32 => 4,
            Fmt::I24 => 3,
            Fmt::I16 => 2,
        }
    }
}

/// A WAVEFORMATEXTENSIBLE for `rate`, `ch` channels of `fmt` (front left and right for stereo).
fn wave(rate: u32, ch: u16, fmt: Fmt) -> WAVEFORMATEXTENSIBLE {
    let bits = 8 * fmt.bytes() as u16;
    let block = ch * fmt.bytes() as u16;
    let sub = if fmt == Fmt::F32 { Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT } else { KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM };
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: KernelStreaming::WAVE_FORMAT_EXTENSIBLE as u16,
            nChannels: ch,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * block as u32,
            nBlockAlign: block,
            wBitsPerSample: bits,
            cbSize: (std::mem::size_of::<WAVEFORMATEXTENSIBLE>() - std::mem::size_of::<WAVEFORMATEX>()) as u16,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: bits },
        dwChannelMask: if ch == 2 { 3 } else { KernelStreaming::KSAUDIO_SPEAKER_DIRECTOUT },
        SubFormat: sub,
    }
}

/// `src` (f32) as `fmt` samples into `dst` (its bytes).
fn write(fmt: Fmt, src: &[f32], dst: &mut [u8]) {
    let c = |s: f32| s.clamp(-1.0, 1.0);
    match fmt {
        Fmt::F32 => dst.chunks_exact_mut(4).zip(src).for_each(|(d, s)| d.copy_from_slice(&s.to_le_bytes())),
        Fmt::I32 => dst.chunks_exact_mut(4).zip(src).for_each(|(d, s)| d.copy_from_slice(&((c(*s) as f64 * 2_147_483_647.0) as i32).to_le_bytes())),
        Fmt::I24 => dst.chunks_exact_mut(3).zip(src).for_each(|(d, s)| d.copy_from_slice(&((c(*s) * 8_388_607.0) as i32).to_le_bytes()[..3])),
        Fmt::I16 => dst.chunks_exact_mut(2).zip(src).for_each(|(d, s)| d.copy_from_slice(&((c(*s) * 32_767.0) as i16).to_le_bytes())),
    }
}

/// The endpoint's friendly name (as cpal names devices).
unsafe fn friendly(dev: &IMMDevice) -> Option<String> {
    unsafe {
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let mut v = store.GetValue(&DEVPKEY_Device_FriendlyName as *const _ as *const _).ok()?;
        let raw = &v.as_raw().Anonymous.Anonymous;
        let name = (raw.vt == VT_LPWSTR.0).then(|| {
            let p = *(&raw.Anonymous as *const _ as *const *const u16);
            let n = (0..).take_while(|&k| *p.add(k) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
        });
        let _ = PropVariantClear(&mut v);
        name
    }
}

/// An initialised exclusive client, ready to start.
struct Client {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: HANDLE,
    frames: u32,
    rate: u32,
    ch: usize,
    fmt: Fmt,
    /// The device's own latency, beyond the buffer.
    latency: Duration,
}

/// Find the playback endpoint named `name` (None or not found: the default), and open it exclusively at 48 kHz (or
/// its mix rate), stereo (or its mix channels), in the best format it takes, at `buffer` frames (0: its smallest
/// period).
unsafe fn setup(name: Option<&str>, buffer: u32) -> Result<Client, String> {
    let e = |x: windows::core::Error| format!("Exclusive mode: {}", x.message());
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e)?;
        let mut dev = None;
        if let Some(n) = name {
            let all = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).map_err(e)?;
            for k in 0..all.GetCount().map_err(e)? {
                let d = all.Item(k).map_err(e)?;
                if friendly(&d).as_deref() == Some(n) {
                    dev = Some(d);
                    break;
                }
            }
        }
        let dev = match dev {
            Some(d) => d,
            None => en.GetDefaultAudioEndpoint(eRender, eConsole).map_err(|_| "No audio device".to_string())?,
        };
        let dname = friendly(&dev).unwrap_or_default();
        let mut client: IAudioClient = dev.Activate(CLSCTX_ALL, None).map_err(e)?;
        let mix = client.GetMixFormat().map_err(e)?;
        let (rate0, ch0) = ((*mix).nSamplesPerSec, (*mix).nChannels);
        CoTaskMemFree(Some(mix as *const _));
        let mut chosen = None;
        'found: for rate in [48_000, rate0] {
            for ch in [2, ch0] {
                for fmt in [Fmt::F32, Fmt::I32, Fmt::I24, Fmt::I16] {
                    let wf = wave(rate, ch, fmt);
                    if client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &wf.Format, None) == S_OK {
                        chosen = Some((rate, ch, fmt, wf));
                        break 'found;
                    }
                }
            }
        }
        let (rate, ch, fmt, wf) = chosen.ok_or_else(|| format!("Exclusive mode: {dname} takes none of Milan Pro's sample formats"))?;
        let (mut def, mut min) = (0i64, 0i64);
        client.GetDevicePeriod(Some(&mut def), Some(&mut min)).map_err(e)?;
        // 100 ns units.
        let mut period = if buffer > 0 { (buffer as i64 * 10_000_000 / rate as i64).max(min) } else { min };
        let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
        if let Err(x) = client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, period, period, &wf.Format, None) {
            if x.code() != AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                return Err(if x.code() == AUDCLNT_E_DEVICE_IN_USE { format!("Exclusive mode: {dname} is in use by another program") } else { e(x) });
            }
            // The period the device can align to, on a fresh client.
            let frames = client.GetBufferSize().map_err(e)?;
            period = (10_000_000.0 * frames as f64 / rate as f64 + 0.5) as i64;
            client = dev.Activate(CLSCTX_ALL, None).map_err(e)?;
            client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, period, period, &wf.Format, None).map_err(e)?;
        }
        let event = CreateEventW(None, false, false, PCWSTR::null()).map_err(e)?;
        let ready = (|| {
            client.SetEventHandle(event)?;
            let frames = client.GetBufferSize()?;
            let render: IAudioRenderClient = client.GetService()?;
            let latency = Duration::from_nanos(client.GetStreamLatency().unwrap_or(0).max(0) as u64 * 100);
            Ok::<_, windows::core::Error>((frames, render, latency))
        })();
        match ready {
            Ok((frames, render, latency)) => Ok(Client { client, render, event, frames, rate, ch: ch as usize, fmt, latency }),
            Err(x) => {
                let _ = CloseHandle(event);
                Err(e(x))
            }
        }
    }
}

/// The render loop: each event, one buffer through `render` (the buffer's first frame is heard after the one playing
/// and the device latency). Ends on `stop` or a device error (reported; the player then reopens the device).
unsafe fn run(c: &Client, stop: &AtomicBool, render: &mut dyn FnMut(&mut [f32], Duration), fail: &dyn Fn(String)) {
    unsafe {
        let mut task = 0u32;
        let mm = AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task).ok();
        let n = c.frames as usize;
        let mut buf = vec![0.0f32; n * c.ch];
        let bytes = n * c.ch * c.fmt.bytes();
        let lat = c.latency + Duration::from_secs_f64(n as f64 / c.rate as f64);
        let mut fill = |silent: bool| -> windows::core::Result<()> {
            let p = c.render.GetBuffer(c.frames)?;
            if silent {
                buf.fill(0.0);
            } else {
                render(&mut buf, lat);
            }
            write(c.fmt, &buf, std::slice::from_raw_parts_mut(p, bytes));
            c.render.ReleaseBuffer(c.frames, 0)
        };
        // An exclusive event stream starts with a buffer queued.
        let started = fill(true).and_then(|_| c.client.Start());
        if let Err(x) = started {
            fail(format!("Exclusive mode: {}", x.message()));
        } else {
            while !stop.load(Relaxed) {
                // A device that stops signalling shows as a stall in the player's watchdog.
                if WaitForSingleObject(c.event, 200) != WAIT_OBJECT_0 {
                    continue;
                }
                if let Err(x) = fill(false) {
                    fail(format!("The audio device stopped ({})", x.message()));
                    break;
                }
            }
            let _ = c.client.Stop();
        }
        let _ = CloseHandle(c.event);
        if let Some(h) = mm {
            let _ = AvRevertMmThreadCharacteristics(h);
        }
    }
}

/// Open device `name` (None: the default) exclusively at `buffer` frames (0: its smallest period). `make` gets the
/// device rate and channel count and returns the render callback (interleaved f32, the time until its first frame is
/// heard); `fail` hears of a device error. The stream (drop it to close) and the rate.
pub fn open<F: FnMut(&mut [f32], Duration) + 'static>(
    name: Option<&str>,
    buffer: u32,
    make: impl FnOnce(u32, usize) -> F + Send + 'static,
    fail: impl Fn(String) + Send + 'static,
) -> Result<(Exclusive, u32), String> {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let (name, quit) = (name.map(str::to_string), stop.clone());
    let thread = std::thread::Builder::new()
        .name("milan-wasapi".into())
        .spawn(move || unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            match setup(name.as_deref(), buffer) {
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
                Ok(c) => {
                    let _ = tx.send(Ok(c.rate));
                    let mut render = make(c.rate, c.ch);
                    run(&c, &quit, &mut render, &fail);
                }
            }
            if com {
                CoUninitialize();
            }
        })
        .map_err(|e| e.to_string())?;
    let mut h = Exclusive { stop, thread: Some(thread) };
    match rx.recv() {
        Ok(Ok(rate)) => Ok((h, rate)),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            h.thread.take();
            Err("Exclusive mode: the device thread stopped".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_sample_conversion() {
        let wf = wave(48_000, 2, Fmt::I24);
        let f = wf.Format;
        assert_eq!((f.nBlockAlign, f.wBitsPerSample, f.nAvgBytesPerSec, f.cbSize), (6, 24, 288_000, 22));
        let mut b = [0u8; 6];
        write(Fmt::I24, &[1.0, -2.0], &mut b);
        assert_eq!(b, [0xff, 0xff, 0x7f, 0x01, 0x00, 0x80]);
        let mut b = [0u8; 4];
        write(Fmt::I16, &[0.5, -1.0], &mut b);
        assert_eq!(b, [0xff, 0x3f, 0x01, 0x80]);
        let mut b = [0u8; 4];
        write(Fmt::F32, &[0.25], &mut b);
        assert_eq!(f32::from_le_bytes(b), 0.25);
    }
}
