//! Output through ALSA, the sound interface every Linux desktop carries.
//!
//! The library is opened at run time rather than linked, so a build of the
//! core has no dependency on it and a machine without sound still loads
//! the library and reports that it has no devices.
//!
//! ALSA hands out no callback of its own: a stream writes blocks into the
//! device and blocks until there is room. Playback therefore runs on a
//! thread of its own, which calls the renderer exactly as the callback of
//! a callback-driven host would. Everything that thread touches after the
//! stream opens is already allocated.

use core::ffi::{c_char, c_int, c_uint, c_void};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::ffi::CStr;
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::audio::{
    AudioError, Backend, BlockTiming, Capturer, DeviceId, DeviceInfo, Direction, InputBackend,
    Name, Rates, Renderer, SUPPORTED_RATES, Stream, StreamConfig,
};
use crate::mixer::MAX_FRAMES;

/// Largest number of devices reported.
pub const MAX_DEVICES: usize = 32;

// Values from the ALSA headers. They are part of the library's interface
// and are fixed by it.
const SND_PCM_STREAM_PLAYBACK: c_int = 0;
const SND_PCM_STREAM_CAPTURE: c_int = 1;
const SND_PCM_FORMAT_FLOAT_LE: c_int = 14;
const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

// Values from dlfcn.h.
const RTLD_NOW: c_int = 2;
const RTLD_LOCAL: c_int = 0;

/// First-in first-out real-time scheduling, from the scheduler header.
const SCHED_FIFO: c_int = 1;

/// How far above the lowest real-time priority the playback thread asks
/// to run. Low enough to leave the kernel's own threads above it.
const PRIORITY_STEP: c_int = 10;

#[repr(C)]
struct SchedParam {
    priority: c_int,
}

unsafe extern "C" {
    fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn pthread_self() -> usize;
    fn pthread_setschedparam(thread: usize, policy: c_int, param: *const SchedParam) -> c_int;
    fn sched_get_priority_min(policy: c_int) -> c_int;
    fn sched_get_priority_max(policy: c_int) -> c_int;
}

/// Asks for real-time scheduling on the calling thread.
///
/// A machine that does not allow it keeps the ordinary policy, which
/// still plays; it is more likely to be interrupted under load. There is
/// nothing to undo: the thread exits with the stream.
fn request_realtime() -> bool {
    // SAFETY: Both calls take a policy and return a priority or -1.
    let (lowest, highest) = unsafe {
        (
            sched_get_priority_min(SCHED_FIFO),
            sched_get_priority_max(SCHED_FIFO),
        )
    };
    if lowest < 0 || highest < lowest {
        return false;
    }
    let param = SchedParam {
        priority: (lowest + PRIORITY_STEP).min(highest),
    };
    // SAFETY: The thread is the calling one and the parameter outlives
    // the call.
    unsafe { pthread_setschedparam(pthread_self(), SCHED_FIFO, &raw const param) == 0 }
}

/// The library's entry points, resolved once.
///
/// Only what an output stream needs is resolved. A missing symbol makes
/// the whole backend unavailable rather than failing later in a call.
struct Library {
    open: unsafe extern "C" fn(*mut *mut c_void, *const c_char, c_int, c_int) -> c_int,
    close: unsafe extern "C" fn(*mut c_void) -> c_int,
    set_params:
        unsafe extern "C" fn(*mut c_void, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
    get_params: unsafe extern "C" fn(*mut c_void, *mut u64, *mut u64) -> c_int,
    prepare: unsafe extern "C" fn(*mut c_void) -> c_int,
    drop_stream: unsafe extern "C" fn(*mut c_void) -> c_int,
    writei: unsafe extern "C" fn(*mut c_void, *const c_void, u64) -> i64,
    readi: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> i64,
    recover: unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int,
    name_hint: unsafe extern "C" fn(c_int, *const c_char, *mut *mut *mut c_void) -> c_int,
    get_hint: unsafe extern "C" fn(*const c_void, *const c_char) -> *mut c_char,
    free_hint: unsafe extern "C" fn(*mut *mut c_void) -> c_int,
    free_string: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: The entries are plain function pointers into a library that is
// never unloaded, and the library is thread safe for distinct handles.
unsafe impl Send for Library {}
// SAFETY: As for `Send`.
unsafe impl Sync for Library {}

/// Resolves one symbol, or returns `None` when the library lacks it.
///
/// # Safety
///
/// The caller states that `handle` came from `dlopen` and that `T` is the
/// type of the named symbol.
unsafe fn symbol<T>(handle: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: The handle is live and the name is a C string.
    let address = unsafe { dlsym(handle, name.as_ptr()) };
    if address.is_null() {
        return None;
    }
    // SAFETY: The caller states the symbol has this type, and a function
    // pointer is the size of a data pointer on every platform this runs
    // on.
    Some(unsafe { core::mem::transmute_copy::<*mut c_void, T>(&address) })
}

impl Library {
    /// Opens the sound library. `None` when it is not installed or does
    /// not carry what an output stream needs.
    fn load() -> Option<Self> {
        // The versioned name is the one a distribution ships; the plain
        // name only exists with the development package installed.
        for name in [c"libasound.so.2", c"libasound.so"] {
            // SAFETY: The name is a C string and the flags are the ones
            // the interface defines.
            let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
            if handle.is_null() {
                continue;
            }
            // SAFETY: Each symbol is declared with the type the library's
            // own header gives it.
            let library = unsafe {
                Some(Self {
                    open: symbol(handle, c"snd_pcm_open")?,
                    close: symbol(handle, c"snd_pcm_close")?,
                    set_params: symbol(handle, c"snd_pcm_set_params")?,
                    get_params: symbol(handle, c"snd_pcm_get_params")?,
                    prepare: symbol(handle, c"snd_pcm_prepare")?,
                    drop_stream: symbol(handle, c"snd_pcm_drop")?,
                    writei: symbol(handle, c"snd_pcm_writei")?,
                    readi: symbol(handle, c"snd_pcm_readi")?,
                    recover: symbol(handle, c"snd_pcm_recover")?,
                    name_hint: symbol(handle, c"snd_device_name_hint")?,
                    get_hint: symbol(handle, c"snd_device_name_get_hint")?,
                    free_hint: symbol(handle, c"snd_device_name_free_hint")?,
                    free_string: symbol(handle, c"free")
                        .or_else(|| symbol(handle, c"snd_free"))
                        .unwrap_or(no_free),
                })
            };
            if library.is_some() {
                return library;
            }
        }
        None
    }
}

/// Stands in for the library's own release call when it exposes none. The
/// hint strings then leak, which is bounded by the number of devices and
/// happens once per enumeration.
unsafe extern "C" fn no_free(_: *mut c_void) {}

/// Devices are named by a hash of their ALSA name, since the interface
/// identifies them by text and the rest of the engine by number.
fn identifier(name: &str) -> u64 {
    // The same mix the project model uses: constants from FNV.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // Zero is reserved for "no device".
    if hash == 0 { 1 } else { hash }
}

/// The ALSA host.
pub struct AlsaBackend {
    library: Arc<Library>,
}

impl AlsaBackend {
    /// Opens the sound library.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::Host`] when the library is not installed.
    pub fn new() -> Result<Self, AudioError> {
        Library::load()
            .map(|library| Self {
                library: Arc::new(library),
            })
            .ok_or(AudioError::Host("the sound library is not installed"))
    }

    /// The ALSA name of a device, by identifier.
    fn name_of(&self, device: DeviceId, wanted: Direction) -> Option<String> {
        let mut found = None;
        self.for_each_device(wanted, |name, _| {
            if identifier(name) == device.0 {
                found = Some(name.to_string());
            }
        });
        found
    }

    /// Calls `visit` with each device available in one direction.
    fn for_each_device(&self, wanted: Direction, mut visit: impl FnMut(&str, &str)) {
        let mut hints: *mut *mut c_void = core::ptr::null_mut();
        // SAFETY: The call fills `hints` with a null-terminated array on
        // success and leaves it untouched otherwise.
        let status = unsafe { (self.library.name_hint)(-1, c"pcm".as_ptr(), &raw mut hints) };
        if status < 0 || hints.is_null() {
            return;
        }
        let mut cursor = hints;
        loop {
            // SAFETY: The array is null terminated and `cursor` is inside
            // it until the terminator is read.
            let entry = unsafe { *cursor };
            if entry.is_null() {
                break;
            }
            // SAFETY: Each entry is a hint the library owns.
            let name = unsafe { hint_field(&self.library, entry, c"NAME") };
            // SAFETY: As above.
            let description = unsafe { hint_field(&self.library, entry, c"DESC") };
            // SAFETY: As above; direction is absent for devices that do
            // both, which are playback devices too.
            let direction = unsafe { hint_field(&self.library, entry, c"IOID") };
            if let Some(name) = name {
                let supports = supports_direction(wanted, direction.as_deref());
                if supports {
                    let shown = description.unwrap_or_else(|| name.clone());
                    // A description carries a second line with the card's
                    // long name, which is not wanted in a list.
                    let first = shown.lines().next().unwrap_or(&shown).to_string();
                    visit(&name, &first);
                }
            }
            // SAFETY: The array is null terminated, so the pointer stays
            // inside it.
            cursor = unsafe { cursor.add(1) };
        }
        // SAFETY: The array came from the call above and is released once.
        unsafe { (self.library.free_hint)(hints) };
    }

    /// Opens a device in one direction and settles its format.
    ///
    /// Returns the open handle and the configuration the device granted,
    /// which is not always the one that was asked for.
    fn open_device(
        &self,
        config: StreamConfig,
        direction: c_int,
    ) -> Result<(Handle, StreamConfig), AudioError> {
        config.validate()?;
        if config.channels != 2 {
            return Err(AudioError::Unsupported("channel count"));
        }
        let wanted = if direction == SND_PCM_STREAM_CAPTURE {
            Direction::Input
        } else {
            Direction::Output
        };
        let name = self
            .name_of(config.device, wanted)
            .ok_or(AudioError::DeviceMissing)?;
        let c_name = std::ffi::CString::new(name).map_err(|_| AudioError::DeviceMissing)?;

        let mut handle: *mut c_void = core::ptr::null_mut();
        // SAFETY: The name is a C string and the handle is written only on
        // success.
        let status = unsafe {
            (self.library.open)(
                &raw mut handle,
                c_name.as_ptr(),
                direction,
                // Blocking: the thread waits for the device rather than
                // spinning.
                0,
            )
        };
        if status < 0 || handle.is_null() {
            return Err(AudioError::DeviceMissing);
        }
        let device = Handle {
            library: Arc::clone(&self.library),
            pcm: handle,
        };

        // The requested block, expressed as the latency the device should
        // keep. Two blocks of headroom is what a wait-and-transfer loop
        // needs to stay ahead without adding delay of its own.
        let latency =
            (config.block_frames as u64 * 2 * 1_000_000 / u64::from(config.sample_rate)) as c_uint;
        // SAFETY: The handle is open and the values come from a validated
        // configuration.
        let status = unsafe {
            (self.library.set_params)(
                device.pcm,
                SND_PCM_FORMAT_FLOAT_LE,
                SND_PCM_ACCESS_RW_INTERLEAVED,
                c_uint::from(config.channels),
                config.sample_rate,
                1,
                latency,
            )
        };
        if status < 0 {
            return Err(AudioError::Unsupported("device configuration"));
        }

        // What the device settled on, which is what the engine is told.
        let mut buffer_frames: u64 = 0;
        let mut period_frames: u64 = 0;
        // SAFETY: The handle is configured; both outputs are written.
        let status = unsafe {
            (self.library.get_params)(device.pcm, &raw mut buffer_frames, &raw mut period_frames)
        };
        let block = if status == 0 && period_frames > 0 {
            (period_frames as usize).min(MAX_FRAMES)
        } else {
            config.block_frames
        };
        Ok((
            device,
            StreamConfig {
                block_frames: block,
                ..config
            },
        ))
    }
}

fn supports_direction(wanted: Direction, hint: Option<&str>) -> bool {
    hint.is_none_or(|value| {
        matches!(
            (wanted, value),
            (Direction::Input, "Input") | (Direction::Output, "Output")
        )
    })
}

/// Reads one field of a device hint as text.
///
/// # Safety
///
/// The caller states that `hint` is an entry from `snd_device_name_hint`.
unsafe fn hint_field(library: &Library, hint: *mut c_void, field: &CStr) -> Option<String> {
    // SAFETY: The caller states the hint is live.
    let raw = unsafe { (library.get_hint)(hint, field.as_ptr()) };
    if raw.is_null() {
        return None;
    }
    // SAFETY: The library returns a null-terminated string it allocated.
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: The string came from the library and is released once.
    unsafe { (library.free_string)(raw.cast::<c_void>()) };
    Some(text)
}

impl Backend for AlsaBackend {
    type Stream = AlsaStream;

    fn name(&self) -> &'static str {
        "ALSA"
    }

    fn devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
        let mut written = 0;
        self.for_each_device(Direction::Output, |name, description| {
            if written >= out.len() {
                return;
            }
            let mut rates = Rates::new();
            // A device is only asked for rates when it is opened, so the
            // list is the set the engine supports.
            for rate in SUPPORTED_RATES {
                let _ = rates.push(rate);
            }
            out[written] = DeviceInfo {
                id: DeviceId(identifier(name)),
                name: Name::truncated(description),
                direction: Direction::Output,
                channels: 2,
                rates,
                is_default: name == "default",
            };
            written += 1;
        });
        Ok(written)
    }

    fn default_output(&self) -> Result<DeviceId, AudioError> {
        let mut found = None;
        self.for_each_device(Direction::Output, |name, _| {
            if found.is_none() || name == "default" {
                if found.is_some() && name != "default" {
                    return;
                }
                found = Some(DeviceId(identifier(name)));
            }
        });
        found.ok_or(AudioError::DeviceMissing)
    }

    fn open_output<R: Renderer + 'static>(
        &self,
        config: StreamConfig,
        renderer: R,
    ) -> Result<Self::Stream, AudioError> {
        let (device, running) = self.open_device(config, SND_PCM_STREAM_PLAYBACK)?;
        let device = Arc::new(device);

        let shared = Arc::new(Shared {
            running: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            dropouts: AtomicU64::new(0),
            realtime: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            device: Arc::clone(&device),
        });
        let worker = Worker {
            device,
            shared: Arc::clone(&shared),
            renderer: Box::new(renderer),
            block: vec![[0.0; 2]; running.block_frames],
        };
        let thread = std::thread::Builder::new()
            .name("nylon-alsa".to_string())
            .stack_size(512 * 1024)
            .spawn(move || worker.run())
            .map_err(|_| AudioError::Host("the playback thread could not be started"))?;

        Ok(AlsaStream {
            shared,
            config: running,
            thread: Some(thread),
        })
    }
}

impl InputBackend for AlsaBackend {
    type Capture = AlsaCapture;

    fn input_devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
        // The host lists a device once; whether it records is only known
        // by opening it, so the same list is offered for capture with the
        // direction it would be used in.
        let mut written = 0;
        self.for_each_device(Direction::Input, |name, description| {
            if written >= out.len() {
                return;
            }
            let mut rates = Rates::new();
            for rate in SUPPORTED_RATES {
                let _ = rates.push(rate);
            }
            out[written] = DeviceInfo {
                id: DeviceId(identifier(name)),
                name: Name::truncated(description),
                direction: Direction::Input,
                channels: 2,
                rates,
                is_default: name == "default",
            };
            written += 1;
        });
        Ok(written)
    }

    fn default_input(&self) -> Result<DeviceId, AudioError> {
        let mut found = None;
        self.for_each_device(Direction::Input, |name, _| {
            if found.is_none() || name == "default" {
                if found.is_some() && name != "default" {
                    return;
                }
                found = Some(DeviceId(identifier(name)));
            }
        });
        found.ok_or(AudioError::DeviceMissing)
    }

    fn open_input<C: Capturer + 'static>(
        &self,
        config: StreamConfig,
        capturer: C,
    ) -> Result<Self::Capture, AudioError> {
        let (device, running) = self.open_device(config, SND_PCM_STREAM_CAPTURE)?;
        let device = Arc::new(device);

        let shared = Arc::new(Shared {
            running: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            dropouts: AtomicU64::new(0),
            realtime: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            device: Arc::clone(&device),
        });
        let mut prepared = capturer;
        prepared.prepare(running);
        let worker = CaptureWorker {
            device,
            shared: Arc::clone(&shared),
            capturer: Box::new(prepared),
            block: vec![[0.0; 2]; running.block_frames],
        };
        let thread = std::thread::Builder::new()
            .name("nylon-alsa-in".to_string())
            .stack_size(512 * 1024)
            .spawn(move || worker.run())
            .map_err(|_| AudioError::Host("the capture thread could not be started"))?;

        Ok(AlsaCapture {
            shared,
            config: running,
            thread: Some(thread),
        })
    }
}

/// The capture thread's own state.
struct CaptureWorker {
    device: Arc<Handle>,
    shared: Arc<Shared>,
    capturer: Box<dyn Capturer>,
    block: Vec<[f32; 2]>,
}

impl CaptureWorker {
    /// Takes blocks from the device for as long as the stream is open.
    ///
    /// Nothing here allocates: the block is filled before the loop starts
    /// and reused for every read.
    fn run(mut self) {
        self.shared
            .realtime
            .store(request_realtime(), Ordering::Relaxed);
        let frames = self.block.len();
        let mut position = 0_u64;
        let mut was_running = false;

        'stream: while !self.shared.finished.load(Ordering::Acquire) {
            if !self.shared.running.load(Ordering::Acquire) {
                was_running = false;
                std::thread::sleep(core::time::Duration::from_millis(2));
                continue;
            }
            if !was_running {
                // SAFETY: The handle is open and idle.
                unsafe { (self.device.library.prepare)(self.device.pcm) };
                was_running = true;
            }

            let mut taken = 0;
            while taken < frames {
                // SAFETY: The slice holds `frames` interleaved pairs and
                // the read starts inside it.
                let count = unsafe {
                    (self.device.library.readi)(
                        self.device.pcm,
                        self.block[taken..].as_mut_ptr().cast::<c_void>(),
                        (frames - taken) as u64,
                    )
                };
                if count > 0 {
                    taken += count as usize;
                    continue;
                }
                if count == 0 {
                    self.shared.lost.store(true, Ordering::Release);
                    self.shared.finished.store(true, Ordering::Release);
                    break 'stream;
                }
                if self.shared.finished.load(Ordering::Acquire) {
                    break 'stream;
                }
                if !self.shared.running.load(Ordering::Acquire) {
                    continue 'stream;
                }
                // The device overran or was suspended, so material was
                // lost. Recovering keeps the take going and counts it.
                self.shared.dropouts.fetch_add(1, Ordering::Relaxed);
                // SAFETY: The handle is open; silent recovery is asked for
                // so the library does not print.
                let recovered =
                    unsafe { (self.device.library.recover)(self.device.pcm, count as c_int, 1) };
                if recovered < 0 {
                    self.shared.lost.store(true, Ordering::Release);
                    self.shared.finished.store(true, Ordering::Release);
                    break;
                }
            }
            if taken == 0 {
                continue;
            }
            let timing = BlockTiming {
                frame: position,
                dropouts: self.shared.dropouts.load(Ordering::Relaxed),
            };
            self.capturer.capture(&self.block[..taken], timing);
            position += taken as u64;
            self.shared
                .frames
                .fetch_add(taken as u64, Ordering::Relaxed);
        }
    }
}

/// A running ALSA input.
pub struct AlsaCapture {
    shared: Arc<Shared>,
    config: StreamConfig,
    thread: Option<JoinHandle<()>>,
}

impl AlsaCapture {
    /// Whether the capture thread runs under real-time scheduling.
    #[must_use]
    pub fn is_realtime(&self) -> bool {
        self.shared.realtime.load(Ordering::Relaxed)
    }
}

impl Stream for AlsaCapture {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if self.shared.running.load(Ordering::Acquire) {
            return Err(AudioError::WrongState);
        }
        self.shared.running.store(true, Ordering::Release);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !self.shared.running.load(Ordering::Acquire) {
            return Err(AudioError::WrongState);
        }
        self.shared.running.store(false, Ordering::Release);
        self.shared.interrupt();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Acquire)
    }

    fn frames_rendered(&self) -> u64 {
        self.shared.frames.load(Ordering::Relaxed)
    }

    fn dropouts(&self) -> u64 {
        self.shared.dropouts.load(Ordering::Relaxed)
    }

    fn is_lost(&self) -> bool {
        self.shared.lost.load(Ordering::Acquire)
    }
}

impl Drop for AlsaCapture {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        self.shared.finished.store(true, Ordering::Release);
        self.shared.interrupt();
        if let Some(thread) = self.thread.take() {
            // The capturer lives on that thread, so it has to finish
            // before this returns.
            let _ = thread.join();
        }
    }
}

/// An open device, closed when dropped.
struct Handle {
    library: Arc<Library>,
    pcm: *mut c_void,
}

// SAFETY: A handle is moved to the playback thread and used only there.
unsafe impl Send for Handle {}
// SAFETY: ALSA serializes calls on a handle in its default thread-safe
// build. The control thread only interrupts a blocking transfer.
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.pcm.is_null() {
            // SAFETY: The handle was opened by this type and is closed
            // once.
            unsafe { (self.library.drop_stream)(self.pcm) };
            // SAFETY: As above.
            unsafe { (self.library.close)(self.pcm) };
            self.pcm = core::ptr::null_mut();
        }
    }
}

/// State the control thread reads while the playback thread runs.
struct Shared {
    running: AtomicBool,
    finished: AtomicBool,
    frames: AtomicU64,
    dropouts: AtomicU64,
    /// Whether the playback thread was granted real-time scheduling.
    realtime: AtomicBool,
    /// Set when the device stops accepting audio for good.
    lost: AtomicBool,
    device: Arc<Handle>,
}

impl Shared {
    /// Stops a blocking transfer so the worker can observe control state.
    fn interrupt(&self) {
        // SAFETY: The stream keeps the handle live until the worker exits.
        // ALSA serializes calls made on one handle when thread safety is
        // enabled, which is the library default.
        unsafe { (self.device.library.drop_stream)(self.device.pcm) };
    }
}

/// The playback thread's own state.
struct Worker {
    device: Arc<Handle>,
    shared: Arc<Shared>,
    renderer: Box<dyn Renderer>,
    block: Vec<[f32; 2]>,
}

impl Worker {
    /// Renders blocks for as long as the stream is open.
    ///
    /// Nothing here allocates: the block buffer is filled before the loop
    /// starts and reused for every write.
    fn run(mut self) {
        self.shared
            .realtime
            .store(request_realtime(), Ordering::Relaxed);
        let frames = self.block.len();
        let mut position = 0_u64;
        let mut was_running = false;
        'stream: while !self.shared.finished.load(Ordering::Acquire) {
            if !self.shared.running.load(Ordering::Acquire) {
                // Stopped: wait without touching the device, so it keeps
                // whatever it has already been given.
                was_running = false;
                std::thread::sleep(core::time::Duration::from_millis(2));
                continue;
            }
            if !was_running {
                // A device that has been left alone needs preparing before
                // it takes audio again.
                // SAFETY: The handle is open and idle.
                unsafe { (self.device.library.prepare)(self.device.pcm) };
                was_running = true;
            }
            let timing = BlockTiming {
                frame: position,
                dropouts: self.shared.dropouts.load(Ordering::Relaxed),
            };
            self.renderer.render(&mut self.block, timing);
            position += frames as u64;

            let mut written = 0;
            while written < frames {
                let remaining = frames - written;
                // SAFETY: The slice holds `frames` interleaved pairs and
                // the write starts inside it.
                let count = unsafe {
                    (self.device.library.writei)(
                        self.device.pcm,
                        self.block[written..].as_ptr().cast::<c_void>(),
                        remaining as u64,
                    )
                };
                if count > 0 {
                    written += count as usize;
                    continue;
                }
                if count == 0 {
                    self.shared.lost.store(true, Ordering::Release);
                    self.shared.finished.store(true, Ordering::Release);
                    break 'stream;
                }
                if self.shared.finished.load(Ordering::Acquire) {
                    break 'stream;
                }
                if !self.shared.running.load(Ordering::Acquire) {
                    continue 'stream;
                }
                // The device ran dry or was suspended. Recovering keeps
                // playback going and counts against the stream.
                self.shared.dropouts.fetch_add(1, Ordering::Relaxed);
                // SAFETY: The handle is open; silent recovery is asked for
                // so the library does not print.
                let recovered =
                    unsafe { (self.device.library.recover)(self.device.pcm, count as c_int, 1) };
                if recovered < 0 {
                    // The device is gone rather than merely behind: it was
                    // unplugged, or the host took it away.
                    self.shared.lost.store(true, Ordering::Release);
                    self.shared.finished.store(true, Ordering::Release);
                    break;
                }
            }
            self.shared
                .frames
                .fetch_add(written as u64, Ordering::Relaxed);
        }
    }
}

/// A running ALSA output.
pub struct AlsaStream {
    shared: Arc<Shared>,
    config: StreamConfig,
    thread: Option<JoinHandle<()>>,
}

impl AlsaStream {
    /// Whether the playback thread runs under real-time scheduling. A
    /// machine that does not allow it still plays.
    #[must_use]
    pub fn is_realtime(&self) -> bool {
        self.shared.realtime.load(Ordering::Relaxed)
    }
}

impl Stream for AlsaStream {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if self.shared.running.load(Ordering::Acquire) {
            return Err(AudioError::WrongState);
        }
        self.shared.running.store(true, Ordering::Release);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !self.shared.running.load(Ordering::Acquire) {
            return Err(AudioError::WrongState);
        }
        self.shared.running.store(false, Ordering::Release);
        self.shared.interrupt();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Acquire)
    }

    fn frames_rendered(&self) -> u64 {
        self.shared.frames.load(Ordering::Relaxed)
    }

    fn dropouts(&self) -> u64 {
        self.shared.dropouts.load(Ordering::Relaxed)
    }

    fn is_lost(&self) -> bool {
        self.shared.lost.load(Ordering::Acquire)
    }
}

impl Drop for AlsaStream {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        self.shared.finished.store(true, Ordering::Release);
        self.shared.interrupt();
        if let Some(thread) = self.thread.take() {
            // The renderer lives on that thread, so it has to be joined
            // before this returns.
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty entry for a buffer the host fills.
    fn blank() -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(0),
            name: Name::new(),
            direction: Direction::Output,
            channels: 0,
            rates: Rates::new(),
            is_default: false,
        }
    }

    /// A renderer that counts the blocks it is asked for.
    struct Counting {
        frames: Arc<AtomicU64>,
    }

    impl Renderer for Counting {
        fn render(&mut self, output: &mut [[f32; 2]], _timing: BlockTiming) {
            for frame in output.iter_mut() {
                *frame = [0.0, 0.0];
            }
            self.frames
                .fetch_add(output.len() as u64, Ordering::Relaxed);
        }
    }

    #[test]
    fn a_name_always_maps_to_the_same_identifier() {
        assert_eq!(identifier("default"), identifier("default"));
        assert_ne!(identifier("default"), identifier("hw:0,0"));
        assert_ne!(identifier(""), 0, "no name may take the reserved value");
    }

    #[test]
    fn direction_hints_keep_input_and_output_devices_apart() {
        assert!(supports_direction(Direction::Input, None));
        assert!(supports_direction(Direction::Output, None));
        assert!(supports_direction(Direction::Input, Some("Input")));
        assert!(!supports_direction(Direction::Input, Some("Output")));
        assert!(supports_direction(Direction::Output, Some("Output")));
        assert!(!supports_direction(Direction::Output, Some("Input")));
    }

    #[test]
    fn the_backend_reports_whether_the_library_is_there() {
        match AlsaBackend::new() {
            Ok(backend) => assert_eq!(backend.name(), "ALSA"),
            Err(error) => assert_eq!(
                error,
                AudioError::Host("the sound library is not installed")
            ),
        }
    }

    #[test]
    fn enumeration_fits_the_buffer_it_is_given() {
        let Ok(backend) = AlsaBackend::new() else {
            return;
        };
        let mut devices = [blank(); MAX_DEVICES];
        let count = backend.devices(&mut devices).expect("enumeration failed");
        assert!(count <= MAX_DEVICES);
        for device in &devices[..count] {
            assert_ne!(device.id.0, 0);
            assert_eq!(device.channels, 2);
            assert!(device.rates.contains(48_000));
        }
        // A caller with room for one device gets one, not a scribble past
        // the end.
        let mut single = [blank(); 1];
        let written = backend.devices(&mut single).expect("enumeration failed");
        assert!(written <= 1);
        assert_eq!(written, count.min(1));
    }

    #[test]
    fn opening_a_device_that_is_not_there_is_refused() {
        let Ok(backend) = AlsaBackend::new() else {
            return;
        };
        let config = StreamConfig {
            device: DeviceId(0xdead_beef),
            sample_rate: 48_000,
            channels: 2,
            block_frames: 512,
        };
        let frames = Arc::new(AtomicU64::new(0));
        let renderer = Counting {
            frames: Arc::clone(&frames),
        };
        assert_eq!(
            backend.open_output(config, renderer).err(),
            Some(AudioError::DeviceMissing)
        );
    }

    /// A capturer that counts what it was handed.
    struct CountingCapture {
        frames: Arc<AtomicU64>,
    }

    impl Capturer for CountingCapture {
        fn capture(&mut self, input: &[[f32; 2]], _timing: BlockTiming) {
            self.frames.fetch_add(input.len() as u64, Ordering::Relaxed);
        }
    }

    #[test]
    fn inputs_are_listed_and_refuse_a_device_that_is_not_there() {
        let Ok(backend) = AlsaBackend::new() else {
            return;
        };
        let mut devices = [blank(); MAX_DEVICES];
        let count = backend
            .input_devices(&mut devices)
            .expect("enumeration failed");
        for device in &devices[..count] {
            assert_eq!(device.direction, Direction::Input);
            assert_ne!(device.id.0, 0);
        }
        assert!(backend.default_input().is_ok());

        let config = StreamConfig {
            device: DeviceId(0xdead_beef),
            sample_rate: 48_000,
            channels: 2,
            block_frames: 512,
        };
        let frames = Arc::new(AtomicU64::new(0));
        assert_eq!(
            backend
                .open_input(
                    config,
                    CountingCapture {
                        frames: Arc::clone(&frames)
                    }
                )
                .err(),
            Some(AudioError::DeviceMissing)
        );
        assert_eq!(frames.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_capture_stream_takes_blocks_from_the_null_device() {
        let Ok(backend) = AlsaBackend::new() else {
            return;
        };
        let mut devices = [blank(); MAX_DEVICES];
        let count = backend.input_devices(&mut devices).unwrap_or(0);
        // The null device accepts a reader and hands back silence, which
        // is the one input a build machine can be relied on to have.
        let Some(target) = devices[..count]
            .iter()
            .find(|device| device.name.as_str().contains("Discard"))
        else {
            return;
        };
        let config = StreamConfig {
            device: target.id,
            sample_rate: 48_000,
            channels: 2,
            block_frames: 256,
        };
        let frames = Arc::new(AtomicU64::new(0));
        let Ok(mut stream) = backend.open_input(
            config,
            CountingCapture {
                frames: Arc::clone(&frames),
            },
        ) else {
            return;
        };
        assert!(!stream.is_running());
        assert!(stream.start().is_ok());
        assert_eq!(stream.start(), Err(AudioError::WrongState));

        let deadline = std::time::Instant::now() + core::time::Duration::from_secs(2);
        while frames.load(Ordering::Relaxed) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(core::time::Duration::from_millis(10));
        }
        assert!(
            frames.load(Ordering::Relaxed) > 0,
            "the stream never reached the capturer"
        );
        assert!(stream.frames_rendered() > 0);
        let _ = stream.is_realtime();
        assert!(stream.stop().is_ok());
    }

    #[test]
    fn a_library_that_is_present_lists_at_least_one_device() {
        let Ok(backend) = AlsaBackend::new() else {
            // Nothing to check on a machine without the library.
            return;
        };
        let mut devices = [blank(); MAX_DEVICES];
        let count = backend.devices(&mut devices).expect("enumeration failed");
        // Every installation carries a configuration with at least the
        // default and null devices in it.
        assert!(
            count > 0,
            "the library is installed but reported no output devices"
        );
        assert!(backend.default_output().is_ok());
    }

    #[test]
    fn a_stream_renders_into_the_null_device() {
        let Ok(backend) = AlsaBackend::new() else {
            return;
        };
        // Every ALSA installation carries a null device, which accepts
        // audio and discards it. It is the one device a build machine can
        // be relied on to have.
        let mut devices = [blank(); MAX_DEVICES];
        let count = backend.devices(&mut devices).unwrap_or(0);
        let Some(target) = devices[..count]
            .iter()
            .find(|device| device.name.as_str().contains("Discard"))
            .or_else(|| devices[..count].first())
        else {
            return;
        };

        let config = StreamConfig {
            device: target.id,
            sample_rate: 48_000,
            channels: 2,
            block_frames: 256,
        };
        let frames = Arc::new(AtomicU64::new(0));
        let renderer = Counting {
            frames: Arc::clone(&frames),
        };
        let Ok(mut stream) = backend.open_output(config, renderer) else {
            // A build machine with no working device is not a failure of
            // this code.
            return;
        };
        assert!(!stream.is_running());
        assert_eq!(stream.frames_rendered(), 0);
        assert!(stream.start().is_ok());
        assert_eq!(stream.start(), Err(AudioError::WrongState));

        // The thread should reach the renderer promptly.
        let deadline = std::time::Instant::now() + core::time::Duration::from_secs(2);
        while frames.load(Ordering::Relaxed) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(core::time::Duration::from_millis(10));
        }
        assert!(
            frames.load(Ordering::Relaxed) > 0,
            "the stream never called the renderer"
        );
        // Real-time scheduling is asked for but not required: a machine
        // that refuses it still renders, which is what was just checked.
        let _ = stream.is_realtime();
        assert!(stream.stop().is_ok());
        assert_eq!(stream.stop(), Err(AudioError::WrongState));
    }
}
