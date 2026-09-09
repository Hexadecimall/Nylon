//! Output through WASAPI, the audio interface Windows has carried since
//! Vista.
//!
//! The interfaces are declared here rather than taken from a crate, the
//! way the other platform backends are, so the core keeps no third-party
//! dependency. Only what an output stream needs is declared.
//!
//! A shared-mode client is opened and asked to signal an event when it
//! wants another block. A thread of its own waits on that event, renders,
//! and copies the block into the buffer the client hands out, which is
//! the same contract a callback host gives the renderer.

use core::ffi::{c_int, c_void};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::audio::{
    AudioError, Backend, BlockTiming, Capturer, DeviceId, DeviceInfo, Direction, InputBackend,
    Name, Rates, Renderer, SUPPORTED_RATES, Stream, StreamConfig,
};
use crate::mixer::MAX_FRAMES;

/// Largest number of devices reported.
pub const MAX_DEVICES: usize = 32;

type Hresult = i32;
const S_OK: Hresult = 0;
const S_FALSE: Hresult = 1;
/// Returned when a thread has already entered a different apartment. The
/// interfaces are still usable, so it is not treated as a failure.
const RPC_E_CHANGED_MODE: Hresult = -2_147_417_850;

/// A globally unique identifier, as the interface tables spell it.
#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

static CLSID_MM_DEVICE_ENUMERATOR: Guid = Guid {
    data1: 0xBCDE_0395,
    data2: 0xE52F,
    data3: 0x467C,
    data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
};
static IID_IMM_DEVICE_ENUMERATOR: Guid = Guid {
    data1: 0xA956_64D2,
    data2: 0x9614,
    data3: 0x4F35,
    data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
};
static IID_IAUDIO_CLIENT: Guid = Guid {
    data1: 0x1CB9_AD4C,
    data2: 0xDBFA,
    data3: 0x4C32,
    data4: [0xB1, 0x78, 0xC2, 0xF5, 0x68, 0xA7, 0x03, 0xB2],
};
static IID_IAUDIO_CAPTURE_CLIENT: Guid = Guid {
    data1: 0xC8AD_BD64,
    data2: 0xE71E,
    data3: 0x48A0,
    data4: [0xA4, 0xDE, 0x18, 0x5C, 0x39, 0x5C, 0xD3, 0x17],
};
static IID_IAUDIO_RENDER_CLIENT: Guid = Guid {
    data1: 0xF294_ACFC,
    data2: 0x3146,
    data3: 0x4483,
    data4: [0xA7, 0xBF, 0xAD, 0xDC, 0xA7, 0xC2, 0x60, 0xE2],
};
// An identifier has to have an address to be passed by reference, so
// these are values rather than constants.
/// The property that carries a device's name as a person reads it.
static PKEY_DEVICE_FRIENDLY_NAME: PropertyKey = PropertyKey {
    fmtid: Guid {
        data1: 0xA45C_254E,
        data2: 0xDF1C,
        data3: 0x4EFD,
        data4: [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0],
    },
    pid: 14,
};

#[repr(C)]
struct PropertyKey {
    fmtid: Guid,
    pid: u32,
}

// Values from the platform headers.
const CLSCTX_ALL: u32 = 23;
const COINIT_MULTITHREADED: u32 = 0;
const EDATAFLOW_RENDER: u32 = 0;
const EDATAFLOW_CAPTURE: u32 = 1;
const EROLE_CONSOLE: u32 = 0;
const DEVICE_STATE_ACTIVE: u32 = 1;
const STGM_READ: u32 = 0;
const AUDCLNT_SHAREMODE_SHARED: u32 = 0;
const AUDCLNT_STREAMFLAGS_EVENTCALLBACK: u32 = 0x0004_0000;
const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;
const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;
const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x0000_0002;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const VT_LPWSTR: u16 = 31;
const WAIT_OBJECT_0: u32 = 0;
/// Returned once the device has been unplugged or taken over. Nothing
/// further can be done with the client.
const AUDCLNT_E_DEVICE_INVALIDATED: Hresult = -2_004_287_484;
const EVENT_TIMEOUT_MS: u32 = 2_000;
/// A hundred-nanosecond tick, the unit the client takes its buffer
/// duration in.
const TICKS_PER_SECOND: i64 = 10_000_000;

#[repr(C)]
#[derive(Clone, Copy)]
struct WaveFormatEx {
    format_tag: u16,
    channels: u16,
    samples_per_second: u32,
    bytes_per_second: u32,
    block_align: u16,
    bits_per_sample: u16,
    extra_size: u16,
}

#[repr(C)]
struct PropVariant {
    kind: u16,
    reserved: [u16; 3],
    // The union. Only the string pointer is read, and only when the kind
    // says the value is one.
    value: *mut u16,
    padding: u64,
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> Hresult;
    fn CoUninitialize();
    fn CoCreateInstance(
        class: *const Guid,
        outer: *mut c_void,
        context: u32,
        interface: *const Guid,
        out: *mut *mut c_void,
    ) -> Hresult;
    fn CoTaskMemFree(block: *mut c_void);
    fn PropVariantClear(value: *mut PropVariant) -> Hresult;
}

#[link(name = "avrt")]
unsafe extern "system" {
    fn AvSetMmThreadCharacteristicsW(task: *const u16, index: *mut u32) -> *mut c_void;
    fn AvRevertMmThreadCharacteristics(handle: *mut c_void) -> c_int;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateEventW(
        attributes: *mut c_void,
        manual_reset: c_int,
        initial_state: c_int,
        name: *const u16,
    ) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn SetEvent(handle: *mut c_void) -> c_int;
    fn CloseHandle(handle: *mut c_void) -> c_int;
}

/// The three entries every interface starts with.
#[repr(C)]
struct Unknown {
    query: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct DeviceEnumeratorVtable {
    base: Unknown,
    enum_endpoints: unsafe extern "system" fn(*mut c_void, u32, u32, *mut *mut c_void) -> Hresult,
    default_endpoint: unsafe extern "system" fn(*mut c_void, u32, u32, *mut *mut c_void) -> Hresult,
    // The rest of the table is not called.
}

#[repr(C)]
struct DeviceCollectionVtable {
    base: Unknown,
    count: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    item: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult,
}

#[repr(C)]
struct DeviceVtable {
    base: Unknown,
    activate: unsafe extern "system" fn(
        *mut c_void,
        *const Guid,
        u32,
        *mut c_void,
        *mut *mut c_void,
    ) -> Hresult,
    open_property_store: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult,
    id: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> Hresult,
    state: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
}

#[repr(C)]
struct PropertyStoreVtable {
    base: Unknown,
    count: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    key_at: unsafe extern "system" fn(*mut c_void, u32, *mut PropertyKey) -> Hresult,
    value: unsafe extern "system" fn(*mut c_void, *const PropertyKey, *mut PropVariant) -> Hresult,
}

#[repr(C)]
struct AudioClientVtable {
    base: Unknown,
    initialize: unsafe extern "system" fn(
        *mut c_void,
        u32,
        u32,
        i64,
        i64,
        *const WaveFormatEx,
        *const Guid,
    ) -> Hresult,
    buffer_size: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    latency: unsafe extern "system" fn(*mut c_void, *mut i64) -> Hresult,
    padding: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    is_format_supported: unsafe extern "system" fn(
        *mut c_void,
        u32,
        *const WaveFormatEx,
        *mut *mut WaveFormatEx,
    ) -> Hresult,
    mix_format: unsafe extern "system" fn(*mut c_void, *mut *mut WaveFormatEx) -> Hresult,
    device_period: unsafe extern "system" fn(*mut c_void, *mut i64, *mut i64) -> Hresult,
    start: unsafe extern "system" fn(*mut c_void) -> Hresult,
    stop: unsafe extern "system" fn(*mut c_void) -> Hresult,
    reset: unsafe extern "system" fn(*mut c_void) -> Hresult,
    set_event_handle: unsafe extern "system" fn(*mut c_void, *mut c_void) -> Hresult,
    service: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
}

#[repr(C)]
struct CaptureClientVtable {
    base: Unknown,
    get_buffer: unsafe extern "system" fn(
        *mut c_void,
        *mut *mut u8,
        *mut u32,
        *mut u32,
        *mut u64,
        *mut u64,
    ) -> Hresult,
    release_buffer: unsafe extern "system" fn(*mut c_void, u32) -> Hresult,
    next_packet_size: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
}

#[repr(C)]
struct RenderClientVtable {
    base: Unknown,
    get_buffer: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u8) -> Hresult,
    release_buffer: unsafe extern "system" fn(*mut c_void, u32, u32) -> Hresult,
}

/// An owned interface pointer, released when dropped.
struct Interface<V> {
    pointer: *mut c_void,
    kind: core::marker::PhantomData<V>,
}

impl<V> Interface<V> {
    /// Takes ownership of a pointer the platform returned.
    ///
    /// # Safety
    ///
    /// The caller states the pointer is an interface of this type with a
    /// reference held for this value.
    unsafe fn from_raw(pointer: *mut c_void) -> Option<Self> {
        if pointer.is_null() {
            return None;
        }
        Some(Self {
            pointer,
            kind: core::marker::PhantomData,
        })
    }

    /// The entry table.
    fn table(&self) -> &V {
        // SAFETY: An interface pointer points at its own table pointer,
        // and the table outlives the interface.
        unsafe { &**self.pointer.cast::<*const V>() }
    }
}

impl<V> Drop for Interface<V> {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            // SAFETY: Every table starts with the three common entries, so
            // release is at the same place whatever the interface is.
            let base = unsafe { &**self.pointer.cast::<*const Unknown>() };
            // SAFETY: The reference this value holds is given up once.
            unsafe { (base.release)(self.pointer) };
            self.pointer = core::ptr::null_mut();
        }
    }
}

// SAFETY: The interfaces are created in the multithreaded apartment, which
// is what makes them usable from whichever thread holds them.
unsafe impl<V> Send for Interface<V> {}

/// Enters the multithreaded apartment for as long as it is held.
struct Apartment {
    joined: bool,
}

impl Apartment {
    /// Joins the apartment. A thread already in a different one keeps it
    /// and is not left, since it was not entered here.
    fn enter() -> Self {
        // SAFETY: The call takes no arguments beyond the flags.
        let status = unsafe { CoInitializeEx(core::ptr::null_mut(), COINIT_MULTITHREADED) };
        debug_assert!(
            status != RPC_E_CHANGED_MODE || cfg!(test),
            "the thread was already in another apartment"
        );
        Self {
            joined: status == S_OK || status == S_FALSE,
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.joined {
            // SAFETY: Balances the call that joined.
            unsafe { CoUninitialize() };
        }
    }
}

/// Reads a null-terminated wide string.
///
/// # Safety
///
/// The caller states the pointer is a null-terminated string.
unsafe fn wide_string(pointer: *const u16) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let mut length = 0;
    // SAFETY: The caller states the string is terminated, so the scan
    // stops inside it.
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
        if length > 512 {
            break;
        }
    }
    // SAFETY: The length was measured above.
    let slice = unsafe { core::slice::from_raw_parts(pointer, length) };
    String::from_utf16_lossy(slice)
}

/// Devices are named by a hash of the platform's own identifier, since it
/// is text and the rest of the engine addresses devices by number.
fn identifier(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if hash == 0 { 1 } else { hash }
}

/// The WASAPI host.
pub struct WasapiBackend {
    _apartment: Apartment,
}

impl WasapiBackend {
    /// Joins the apartment the interfaces live in.
    #[must_use]
    pub fn new() -> Self {
        Self {
            _apartment: Apartment::enter(),
        }
    }

    /// Creates the device enumerator.
    fn enumerator() -> Result<Interface<DeviceEnumeratorVtable>, AudioError> {
        let mut raw: *mut c_void = core::ptr::null_mut();
        // SAFETY: The identifiers name the enumerator and its interface,
        // and the pointer is written only on success.
        let status = unsafe {
            CoCreateInstance(
                &raw const CLSID_MM_DEVICE_ENUMERATOR,
                core::ptr::null_mut(),
                CLSCTX_ALL,
                &raw const IID_IMM_DEVICE_ENUMERATOR,
                &raw mut raw,
            )
        };
        if status != S_OK {
            return Err(AudioError::Host("the audio service is not available"));
        }
        // SAFETY: The call gave this value the reference it returned.
        unsafe { Interface::from_raw(raw) }
            .ok_or(AudioError::Host("the audio service is not available"))
    }

    /// Calls `visit` with the identifier and name of every active output.
    fn for_each_device(visit: impl FnMut(&str, &str, bool)) -> Result<(), AudioError> {
        Self::for_each_endpoint(EDATAFLOW_RENDER, visit)
    }

    /// Calls `visit` with the identifier and name of every active input.
    fn for_each_input(visit: impl FnMut(&str, &str, bool)) -> Result<(), AudioError> {
        Self::for_each_endpoint(EDATAFLOW_CAPTURE, visit)
    }

    /// Calls `visit` for every active endpoint carrying audio in one
    /// direction.
    fn for_each_endpoint(
        flow: u32,
        mut visit: impl FnMut(&str, &str, bool),
    ) -> Result<(), AudioError> {
        let enumerator = Self::enumerator()?;

        // The default endpoint, so the list can mark it.
        let mut default_id = String::new();
        let mut default_raw: *mut c_void = core::ptr::null_mut();
        // SAFETY: The enumerator is live; the pointer is written on
        // success only.
        let status = unsafe {
            (enumerator.table().default_endpoint)(
                enumerator.pointer,
                flow,
                EROLE_CONSOLE,
                &raw mut default_raw,
            )
        };
        if status == S_OK {
            // SAFETY: The call gave this value a reference.
            if let Some(device) = unsafe { Interface::<DeviceVtable>::from_raw(default_raw) } {
                default_id = device_identifier(&device);
            }
        }

        let mut collection_raw: *mut c_void = core::ptr::null_mut();
        // SAFETY: As above.
        let status = unsafe {
            (enumerator.table().enum_endpoints)(
                enumerator.pointer,
                flow,
                DEVICE_STATE_ACTIVE,
                &raw mut collection_raw,
            )
        };
        if status != S_OK {
            return Ok(());
        }
        // SAFETY: The call gave this value a reference.
        let Some(collection) =
            (unsafe { Interface::<DeviceCollectionVtable>::from_raw(collection_raw) })
        else {
            return Ok(());
        };

        let mut count: u32 = 0;
        // SAFETY: The collection is live and the count is written.
        if unsafe { (collection.table().count)(collection.pointer, &raw mut count) } != S_OK {
            return Ok(());
        }
        for index in 0..count {
            let mut device_raw: *mut c_void = core::ptr::null_mut();
            // SAFETY: The index is inside the collection.
            let status = unsafe {
                (collection.table().item)(collection.pointer, index, &raw mut device_raw)
            };
            if status != S_OK {
                continue;
            }
            // SAFETY: The call gave this value a reference.
            let Some(device) = (unsafe { Interface::<DeviceVtable>::from_raw(device_raw) }) else {
                continue;
            };
            let id = device_identifier(&device);
            if id.is_empty() {
                continue;
            }
            let name = device_name(&device).unwrap_or_else(|| id.clone());
            visit(&id, &name, id == default_id);
        }
        Ok(())
    }

    /// Finds an active endpoint by identifier, in one direction.
    fn find_device(flow: u32, wanted: DeviceId) -> Result<Interface<DeviceVtable>, AudioError> {
        let enumerator = Self::enumerator()?;
        let mut collection_raw: *mut c_void = core::ptr::null_mut();
        // SAFETY: The enumerator is live and the pointer is written on
        // success.
        let status = unsafe {
            (enumerator.table().enum_endpoints)(
                enumerator.pointer,
                flow,
                DEVICE_STATE_ACTIVE,
                &raw mut collection_raw,
            )
        };
        if status != S_OK {
            return Err(AudioError::DeviceMissing);
        }
        // SAFETY: The call gave this value a reference.
        let collection = unsafe { Interface::<DeviceCollectionVtable>::from_raw(collection_raw) }
            .ok_or(AudioError::DeviceMissing)?;
        let mut count: u32 = 0;
        // SAFETY: The collection is live.
        if unsafe { (collection.table().count)(collection.pointer, &raw mut count) } != S_OK {
            return Err(AudioError::DeviceMissing);
        }
        for index in 0..count {
            let mut device_raw: *mut c_void = core::ptr::null_mut();
            // SAFETY: The index is inside the collection.
            let status = unsafe {
                (collection.table().item)(collection.pointer, index, &raw mut device_raw)
            };
            if status != S_OK {
                continue;
            }
            // SAFETY: The call gave this value a reference.
            let Some(device) = (unsafe { Interface::<DeviceVtable>::from_raw(device_raw) }) else {
                continue;
            };
            if identifier(&device_identifier(&device)) == wanted.0 {
                return Ok(device);
            }
        }
        Err(AudioError::DeviceMissing)
    }

    /// Activates a device's audio client.
    fn activate_client(
        device: &Interface<DeviceVtable>,
    ) -> Result<Interface<AudioClientVtable>, AudioError> {
        let mut client_raw: *mut c_void = core::ptr::null_mut();
        // SAFETY: The device is live and the interface is the one asked
        // for.
        let status = unsafe {
            (device.table().activate)(
                device.pointer,
                &raw const IID_IAUDIO_CLIENT,
                CLSCTX_ALL,
                core::ptr::null_mut(),
                &raw mut client_raw,
            )
        };
        if status != S_OK {
            return Err(AudioError::Host("the device refused to open"));
        }
        // SAFETY: The call gave this value a reference.
        unsafe { Interface::from_raw(client_raw) }
            .ok_or(AudioError::Host("the device refused to open"))
    }
}

impl Default for WasapiBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// The platform's own identifier for a device, as text.
fn device_identifier(device: &Interface<DeviceVtable>) -> String {
    let mut raw: *mut u16 = core::ptr::null_mut();
    // SAFETY: The device is live and the pointer is written on success.
    if unsafe { (device.table().id)(device.pointer, &raw mut raw) } != S_OK {
        return String::new();
    }
    // SAFETY: The platform returns a terminated string it allocated.
    let text = unsafe { wide_string(raw) };
    // SAFETY: The string came from the platform allocator.
    unsafe { CoTaskMemFree(raw.cast::<c_void>()) };
    text
}

/// A device's name as a person reads it.
fn device_name(device: &Interface<DeviceVtable>) -> Option<String> {
    let mut store_raw: *mut c_void = core::ptr::null_mut();
    // SAFETY: The device is live; the store is written on success.
    let status = unsafe {
        (device.table().open_property_store)(device.pointer, STGM_READ, &raw mut store_raw)
    };
    if status != S_OK {
        return None;
    }
    // SAFETY: The call gave this value a reference.
    let store = unsafe { Interface::<PropertyStoreVtable>::from_raw(store_raw) }?;

    let mut value = PropVariant {
        kind: 0,
        reserved: [0; 3],
        value: core::ptr::null_mut(),
        padding: 0,
    };
    // SAFETY: The store is live and the property is one it may hold.
    let status = unsafe {
        (store.table().value)(
            store.pointer,
            &raw const PKEY_DEVICE_FRIENDLY_NAME,
            &raw mut value,
        )
    };
    if status != S_OK || value.kind != VT_LPWSTR {
        return None;
    }
    // SAFETY: The kind says the union holds a terminated string.
    let text = unsafe { wide_string(value.value) };
    // SAFETY: The value came from the call above and is cleared once.
    unsafe { PropVariantClear(&raw mut value) };
    Some(text)
}

impl Backend for WasapiBackend {
    type Stream = WasapiStream;

    fn name(&self) -> &'static str {
        "WASAPI"
    }

    fn devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
        let mut written = 0;
        Self::for_each_device(|id, name, is_default| {
            if written >= out.len() {
                return;
            }
            let mut rates = Rates::new();
            // A shared-mode client converts for the engine, so every rate
            // the engine supports can be asked for.
            for rate in SUPPORTED_RATES {
                let _ = rates.push(rate);
            }
            out[written] = DeviceInfo {
                id: DeviceId(identifier(id)),
                name: Name::truncated(name),
                direction: Direction::Output,
                channels: 2,
                rates,
                is_default,
            };
            written += 1;
        })?;
        Ok(written)
    }

    fn default_output(&self) -> Result<DeviceId, AudioError> {
        let mut found = None;
        Self::for_each_device(|id, _, is_default| {
            if is_default || found.is_none() {
                if found.is_some() && !is_default {
                    return;
                }
                found = Some(DeviceId(identifier(id)));
            }
        })?;
        found.ok_or(AudioError::DeviceMissing)
    }

    fn open_output<R: Renderer + 'static>(
        &self,
        config: StreamConfig,
        renderer: R,
    ) -> Result<Self::Stream, AudioError> {
        config.validate()?;
        if config.channels != 2 {
            return Err(AudioError::Unsupported("channel count"));
        }

        // The device is found by identifier, then activated for playback.
        let device = Self::find_device(EDATAFLOW_RENDER, config.device)?;
        let client = Self::activate_client(&device)?;

        // Stereo floating point at the rate that was asked for. A shared
        // client converts to whatever the device is mixing at.
        let format = WaveFormatEx {
            format_tag: WAVE_FORMAT_IEEE_FLOAT,
            channels: 2,
            samples_per_second: config.sample_rate,
            bytes_per_second: config.sample_rate * 8,
            block_align: 8,
            bits_per_sample: 32,
            extra_size: 0,
        };
        let duration = i64::from(config.block_frames as u32) * 4 * TICKS_PER_SECOND
            / i64::from(config.sample_rate);
        // SAFETY: The client is live and the format outlives the call.
        let status = unsafe {
            (client.table().initialize)(
                client.pointer,
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                duration,
                0,
                &raw const format,
                core::ptr::null(),
            )
        };
        if status != S_OK {
            return Err(AudioError::Unsupported("device configuration"));
        }

        let mut buffer_frames: u32 = 0;
        // SAFETY: The client is initialized.
        if unsafe { (client.table().buffer_size)(client.pointer, &raw mut buffer_frames) } != S_OK {
            return Err(AudioError::Host("the device would not report its buffer"));
        }

        // SAFETY: The arguments are the defaults for an unnamed event that
        // resets itself.
        let event = unsafe { CreateEventW(core::ptr::null_mut(), 0, 0, core::ptr::null()) };
        if event.is_null() {
            return Err(AudioError::Host("the wake-up event could not be made"));
        }
        let event = Arc::new(EventHandle(event));
        // SAFETY: The client is initialized for event callbacks and the
        // event outlives it.
        if unsafe { (client.table().set_event_handle)(client.pointer, event.0) } != S_OK {
            return Err(AudioError::Host("the device refused the wake-up event"));
        }

        let mut render_raw: *mut c_void = core::ptr::null_mut();
        // SAFETY: The client is initialized and the service is the one
        // asked for.
        let status = unsafe {
            (client.table().service)(
                client.pointer,
                &raw const IID_IAUDIO_RENDER_CLIENT,
                &raw mut render_raw,
            )
        };
        if status != S_OK {
            return Err(AudioError::Host("the device would not hand out a buffer"));
        }
        // SAFETY: The call gave this value a reference.
        let render = unsafe { Interface::<RenderClientVtable>::from_raw(render_raw) }
            .ok_or(AudioError::Host("the device would not hand out a buffer"))?;

        // The client asks for whole buffers, so that is the block the
        // renderer is given.
        let block = (buffer_frames as usize).clamp(1, MAX_FRAMES);
        let running = StreamConfig {
            block_frames: block,
            ..config
        };

        let shared = Arc::new(Shared {
            running: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            dropouts: AtomicU64::new(0),
            realtime: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            wake: Arc::clone(&event),
        });
        let worker = Worker {
            client,
            render,
            event,
            buffer_frames,
            shared: Arc::clone(&shared),
            renderer: Box::new(renderer),
            block: vec![[0.0; 2]; block],
        };
        let thread = std::thread::Builder::new()
            .name("nylon-wasapi".to_string())
            .stack_size(512 * 1024)
            .spawn(move || worker.run())
            .map_err(|_| AudioError::Host("the playback thread could not be started"))?;

        Ok(WasapiStream {
            shared,
            config: running,
            thread: Some(thread),
        })
    }
}

impl InputBackend for WasapiBackend {
    type Capture = WasapiCapture;

    fn input_devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
        let mut written = 0;
        Self::for_each_input(|id, name, is_default| {
            if written >= out.len() {
                return;
            }
            let mut rates = Rates::new();
            for rate in SUPPORTED_RATES {
                let _ = rates.push(rate);
            }
            out[written] = DeviceInfo {
                id: DeviceId(identifier(id)),
                name: Name::truncated(name),
                direction: Direction::Input,
                channels: 2,
                rates,
                is_default,
            };
            written += 1;
        })?;
        Ok(written)
    }

    fn default_input(&self) -> Result<DeviceId, AudioError> {
        let mut found = None;
        Self::for_each_input(|id, _, is_default| {
            if is_default || found.is_none() {
                if found.is_some() && !is_default {
                    return;
                }
                found = Some(DeviceId(identifier(id)));
            }
        })?;
        found.ok_or(AudioError::DeviceMissing)
    }

    fn open_input<C: Capturer + 'static>(
        &self,
        config: StreamConfig,
        capturer: C,
    ) -> Result<Self::Capture, AudioError> {
        config.validate()?;
        if config.channels != 2 {
            return Err(AudioError::Unsupported("channel count"));
        }

        let device = Self::find_device(EDATAFLOW_CAPTURE, config.device)?;
        let client = Self::activate_client(&device)?;

        let format = WaveFormatEx {
            format_tag: WAVE_FORMAT_IEEE_FLOAT,
            channels: 2,
            samples_per_second: config.sample_rate,
            bytes_per_second: config.sample_rate * 8,
            block_align: 8,
            bits_per_sample: 32,
            extra_size: 0,
        };
        let duration =
            config.block_frames as i64 * 4 * TICKS_PER_SECOND / i64::from(config.sample_rate);
        // SAFETY: The client is live and the format outlives the call.
        let status = unsafe {
            (client.table().initialize)(
                client.pointer,
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                duration,
                0,
                &raw const format,
                core::ptr::null(),
            )
        };
        if status != S_OK {
            return Err(AudioError::Unsupported("device configuration"));
        }

        let mut buffer_frames: u32 = 0;
        // SAFETY: The client is initialized.
        if unsafe { (client.table().buffer_size)(client.pointer, &raw mut buffer_frames) } != S_OK {
            return Err(AudioError::Host("the device would not report its buffer"));
        }

        // SAFETY: The arguments are the defaults for an unnamed event that
        // resets itself.
        let event = unsafe { CreateEventW(core::ptr::null_mut(), 0, 0, core::ptr::null()) };
        if event.is_null() {
            return Err(AudioError::Host("the wake-up event could not be made"));
        }
        let event = Arc::new(EventHandle(event));
        // SAFETY: The client is initialized for event callbacks and the
        // event outlives it.
        if unsafe { (client.table().set_event_handle)(client.pointer, event.0) } != S_OK {
            return Err(AudioError::Host("the device refused the wake-up event"));
        }

        let mut capture_raw: *mut c_void = core::ptr::null_mut();
        // SAFETY: The client is initialized and the service is the one
        // asked for.
        let status = unsafe {
            (client.table().service)(
                client.pointer,
                &raw const IID_IAUDIO_CAPTURE_CLIENT,
                &raw mut capture_raw,
            )
        };
        if status != S_OK {
            return Err(AudioError::Host("the device would not hand out a buffer"));
        }
        // SAFETY: The call gave this value a reference.
        let capture = unsafe { Interface::<CaptureClientVtable>::from_raw(capture_raw) }
            .ok_or(AudioError::Host("the device would not hand out a buffer"))?;

        let block = (buffer_frames as usize).clamp(1, MAX_FRAMES);
        let running = StreamConfig {
            block_frames: block,
            ..config
        };

        let shared = Arc::new(Shared {
            running: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            dropouts: AtomicU64::new(0),
            realtime: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            wake: Arc::clone(&event),
        });
        let mut prepared = capturer;
        prepared.prepare(running);
        let worker = CaptureWorker {
            client,
            capture,
            event,
            shared: Arc::clone(&shared),
            capturer: Box::new(prepared),
            block: vec![[0.0; 2]; MAX_FRAMES],
        };
        let thread = std::thread::Builder::new()
            .name("nylon-wasapi-in".to_string())
            .stack_size(512 * 1024)
            .spawn(move || worker.run())
            .map_err(|_| AudioError::Host("the capture thread could not be started"))?;

        Ok(WasapiCapture {
            shared,
            config: running,
            thread: Some(thread),
        })
    }
}

/// The capture thread's own state.
struct CaptureWorker {
    client: Interface<AudioClientVtable>,
    capture: Interface<CaptureClientVtable>,
    event: Arc<EventHandle>,
    shared: Arc<Shared>,
    capturer: Box<dyn Capturer>,
    block: Vec<[f32; 2]>,
}

impl CaptureWorker {
    /// Takes packets from the device for as long as the stream is open.
    ///
    /// Nothing here allocates: the block is filled before the loop starts
    /// and reused for every packet.
    fn run(mut self) {
        let _apartment = Apartment::enter();
        let class = AudioClass::join();
        self.shared
            .realtime
            .store(class.joined(), Ordering::Relaxed);
        let mut position = 0_u64;
        let mut started = false;

        while !self.shared.finished.load(Ordering::Acquire) {
            if !self.shared.running.load(Ordering::Acquire) {
                if started {
                    // SAFETY: The client is running.
                    unsafe { (self.client.table().stop)(self.client.pointer) };
                    started = false;
                }
                std::thread::sleep(core::time::Duration::from_millis(2));
                continue;
            }
            if !started {
                // SAFETY: The client is initialized and idle.
                unsafe { (self.client.table().reset)(self.client.pointer) };
                // SAFETY: As above.
                let status = unsafe { (self.client.table().start)(self.client.pointer) };
                if status != S_OK {
                    if status == AUDCLNT_E_DEVICE_INVALIDATED {
                        self.shared.lost.store(true, Ordering::Release);
                    }
                    self.shared.finished.store(true, Ordering::Release);
                    break;
                }
                started = true;
            }

            // SAFETY: The handle is live for as long as this value is.
            let waited = unsafe { WaitForSingleObject(self.event.0, EVENT_TIMEOUT_MS) };
            if waited != WAIT_OBJECT_0 {
                self.shared.dropouts.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if self.shared.finished.load(Ordering::Acquire)
                || !self.shared.running.load(Ordering::Acquire)
            {
                continue;
            }

            // The device hands over whole packets, and there may be more
            // than one waiting.
            loop {
                let mut waiting: u32 = 0;
                // SAFETY: The client is running.
                let status = unsafe {
                    (self.capture.table().next_packet_size)(self.capture.pointer, &raw mut waiting)
                };
                if status != S_OK {
                    if status == AUDCLNT_E_DEVICE_INVALIDATED {
                        self.shared.lost.store(true, Ordering::Release);
                        self.shared.finished.store(true, Ordering::Release);
                    }
                    break;
                }
                if waiting == 0 {
                    break;
                }

                let mut buffer: *mut u8 = core::ptr::null_mut();
                let mut frames: u32 = 0;
                let mut flags: u32 = 0;
                // SAFETY: Every output is written before it is read, and
                // the positions are not wanted.
                let status = unsafe {
                    (self.capture.table().get_buffer)(
                        self.capture.pointer,
                        &raw mut buffer,
                        &raw mut frames,
                        &raw mut flags,
                        core::ptr::null_mut(),
                        core::ptr::null_mut(),
                    )
                };
                if status != S_OK {
                    if status == AUDCLNT_E_DEVICE_INVALIDATED {
                        self.shared.lost.store(true, Ordering::Release);
                        self.shared.finished.store(true, Ordering::Release);
                    }
                    self.shared.dropouts.fetch_add(1, Ordering::Relaxed);
                    break;
                }

                if flags & AUDCLNT_BUFFERFLAGS_SILENT != 0 || buffer.is_null() {
                    // The device says the packet is silence and may not
                    // have filled the memory at all.
                    let mut remaining = frames as usize;
                    while remaining != 0 {
                        let count = remaining.min(self.block.len());
                        self.block[..count].fill([0.0, 0.0]);
                        let timing = BlockTiming {
                            frame: position,
                            dropouts: self.shared.dropouts.load(Ordering::Relaxed),
                        };
                        self.capturer.capture(&self.block[..count], timing);
                        position += count as u64;
                        self.shared
                            .frames
                            .fetch_add(count as u64, Ordering::Relaxed);
                        remaining -= count;
                    }
                } else {
                    // SAFETY: The device reports how many frames it wrote,
                    // in the stereo float format it was opened with.
                    let input = unsafe {
                        core::slice::from_raw_parts(buffer.cast::<[f32; 2]>(), frames as usize)
                    };
                    for chunk in input.chunks(MAX_FRAMES) {
                        let timing = BlockTiming {
                            frame: position,
                            dropouts: self.shared.dropouts.load(Ordering::Relaxed),
                        };
                        self.capturer.capture(chunk, timing);
                        position += chunk.len() as u64;
                        self.shared
                            .frames
                            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    }
                }

                // SAFETY: The whole packet is released, which is what the
                // interface requires whatever was taken from it.
                unsafe { (self.capture.table().release_buffer)(self.capture.pointer, frames) };
            }
        }

        if started {
            // SAFETY: The client is running.
            unsafe { (self.client.table().stop)(self.client.pointer) };
        }
    }
}

/// A running WASAPI input.
pub struct WasapiCapture {
    shared: Arc<Shared>,
    config: StreamConfig,
    thread: Option<JoinHandle<()>>,
}

impl WasapiCapture {
    /// Whether the capture thread runs in the scheduler's audio class.
    #[must_use]
    pub fn is_realtime(&self) -> bool {
        self.shared.realtime.load(Ordering::Relaxed)
    }
}

impl Stream for WasapiCapture {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if self.shared.running.load(Ordering::Acquire) {
            return Err(AudioError::WrongState);
        }
        self.shared.running.store(true, Ordering::Release);
        self.shared.wake();
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !self.shared.running.load(Ordering::Acquire) {
            return Err(AudioError::WrongState);
        }
        self.shared.running.store(false, Ordering::Release);
        self.shared.wake();
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

impl Drop for WasapiCapture {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        self.shared.finished.store(true, Ordering::Release);
        self.shared.wake();
        if let Some(thread) = self.thread.take() {
            // The capturer and the interfaces live on that thread, so it
            // has to finish before this returns.
            let _ = thread.join();
        }
    }
}

/// Membership of the scheduler's audio class, given up when dropped.
///
/// A machine that refuses it still plays; the thread is then scheduled
/// like any other and is more likely to be interrupted under load.
struct AudioClass(*mut c_void);

impl AudioClass {
    /// Joins the class for the calling thread.
    fn join() -> Self {
        // The scheduler names its classes; this is the one meant for
        // audio that must not be interrupted.
        let task: [u16; 10] = [
            b'P' as u16,
            b'r' as u16,
            b'o' as u16,
            b' ' as u16,
            b'A' as u16,
            b'u' as u16,
            b'd' as u16,
            b'i' as u16,
            b'o' as u16,
            0,
        ];
        let mut index: u32 = 0;
        // SAFETY: The name is terminated and the index is written.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &raw mut index) };
        Self(handle)
    }

    /// Whether the class was granted.
    fn joined(&self) -> bool {
        !self.0.is_null()
    }
}

impl Drop for AudioClass {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: The handle came from the call above and is given up
            // once.
            unsafe { AvRevertMmThreadCharacteristics(self.0) };
            self.0 = core::ptr::null_mut();
        }
    }
}

/// An event handle, closed when dropped.
struct EventHandle(*mut c_void);

// SAFETY: A handle is a value the platform accepts from any thread.
unsafe impl Send for EventHandle {}
// SAFETY: Waiting and setting a Windows event are safe from different threads.
unsafe impl Sync for EventHandle {}

impl Drop for EventHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: The handle came from `CreateEventW` and is closed
            // once.
            unsafe { CloseHandle(self.0) };
            self.0 = core::ptr::null_mut();
        }
    }
}

/// State the control thread reads while the playback thread runs.
struct Shared {
    running: AtomicBool,
    finished: AtomicBool,
    frames: AtomicU64,
    dropouts: AtomicU64,
    /// Whether the playback thread was admitted to the audio class.
    realtime: AtomicBool,
    /// Set when the device is taken away.
    lost: AtomicBool,
    /// Wakes the worker after a control-state change.
    wake: Arc<EventHandle>,
}

impl Shared {
    /// Makes a waiting worker observe control changes immediately.
    fn wake(&self) {
        // SAFETY: The handle remains live through the shared reference.
        unsafe { SetEvent(self.wake.0) };
    }
}

/// The playback thread's own state.
struct Worker {
    client: Interface<AudioClientVtable>,
    render: Interface<RenderClientVtable>,
    event: Arc<EventHandle>,
    buffer_frames: u32,
    shared: Arc<Shared>,
    renderer: Box<dyn Renderer>,
    block: Vec<[f32; 2]>,
}

impl Worker {
    /// Fills the device's buffer for as long as the stream is open.
    ///
    /// Nothing here allocates: the block is filled before the loop starts
    /// and reused for every pass.
    fn run(mut self) {
        // The interfaces were created in the multithreaded apartment, so
        // this thread joins it too.
        let _apartment = Apartment::enter();
        let class = AudioClass::join();
        self.shared
            .realtime
            .store(class.joined(), Ordering::Relaxed);
        let mut position = 0_u64;
        let mut started = false;

        while !self.shared.finished.load(Ordering::Acquire) {
            if !self.shared.running.load(Ordering::Acquire) {
                if started {
                    // SAFETY: The client is running.
                    unsafe { (self.client.table().stop)(self.client.pointer) };
                    started = false;
                }
                std::thread::sleep(core::time::Duration::from_millis(2));
                continue;
            }
            if !started {
                // SAFETY: The client is initialized and idle.
                unsafe { (self.client.table().reset)(self.client.pointer) };
                // SAFETY: As above.
                let status = unsafe { (self.client.table().start)(self.client.pointer) };
                if status != S_OK {
                    if status == AUDCLNT_E_DEVICE_INVALIDATED {
                        self.shared.lost.store(true, Ordering::Release);
                    }
                    self.shared.finished.store(true, Ordering::Release);
                    break;
                }
                started = true;
            }

            // SAFETY: The handle is live for as long as this value is.
            let waited = unsafe { WaitForSingleObject(self.event.0, EVENT_TIMEOUT_MS) };
            if waited != WAIT_OBJECT_0 {
                // The device stopped asking for audio. Counting it as a
                // dropout keeps the silence visible.
                self.shared.dropouts.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if self.shared.finished.load(Ordering::Acquire)
                || !self.shared.running.load(Ordering::Acquire)
            {
                continue;
            }

            let mut padding: u32 = 0;
            // SAFETY: The client is running.
            let status =
                unsafe { (self.client.table().padding)(self.client.pointer, &raw mut padding) };
            if status != S_OK {
                if status == AUDCLNT_E_DEVICE_INVALIDATED {
                    self.shared.lost.store(true, Ordering::Release);
                    self.shared.finished.store(true, Ordering::Release);
                    break;
                }
                continue;
            }
            let available = self.buffer_frames.saturating_sub(padding);
            if available == 0 {
                continue;
            }
            let frames = (available as usize).min(self.block.len());

            let mut buffer: *mut u8 = core::ptr::null_mut();
            // SAFETY: The count is what the client said it has room for.
            let status = unsafe {
                (self.render.table().get_buffer)(
                    self.render.pointer,
                    frames as u32,
                    &raw mut buffer,
                )
            };
            if status != S_OK || buffer.is_null() {
                if status == AUDCLNT_E_DEVICE_INVALIDATED {
                    self.shared.lost.store(true, Ordering::Release);
                    self.shared.finished.store(true, Ordering::Release);
                    break;
                }
                self.shared.dropouts.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            let timing = BlockTiming {
                frame: position,
                dropouts: self.shared.dropouts.load(Ordering::Relaxed),
            };
            self.renderer.render(&mut self.block[..frames], timing);
            position += frames as u64;

            // SAFETY: The buffer holds `frames` stereo pairs of floats,
            // which is what the format asked for.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.block.as_ptr().cast::<u8>(),
                    buffer,
                    frames * core::mem::size_of::<[f32; 2]>(),
                );
            }
            // SAFETY: The same count that was taken is given back.
            unsafe { (self.render.table().release_buffer)(self.render.pointer, frames as u32, 0) };
            self.shared
                .frames
                .fetch_add(frames as u64, Ordering::Relaxed);
        }

        if started {
            // SAFETY: The client is running.
            unsafe { (self.client.table().stop)(self.client.pointer) };
        }
    }
}

/// A running WASAPI output.
pub struct WasapiStream {
    shared: Arc<Shared>,
    config: StreamConfig,
    thread: Option<JoinHandle<()>>,
}

impl WasapiStream {
    /// Whether the playback thread runs in the scheduler's audio class. A
    /// machine that refuses it still plays.
    #[must_use]
    pub fn is_realtime(&self) -> bool {
        self.shared.realtime.load(Ordering::Relaxed)
    }
}

impl Stream for WasapiStream {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if self.shared.running.load(Ordering::Acquire) {
            return Err(AudioError::WrongState);
        }
        self.shared.running.store(true, Ordering::Release);
        self.shared.wake();
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !self.shared.running.load(Ordering::Acquire) {
            return Err(AudioError::WrongState);
        }
        self.shared.running.store(false, Ordering::Release);
        self.shared.wake();
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

impl Drop for WasapiStream {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        self.shared.finished.store(true, Ordering::Release);
        self.shared.wake();
        if let Some(thread) = self.thread.take() {
            // The renderer and the interfaces live on that thread, so it
            // has to finish before this returns.
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

    /// A renderer that counts the frames it is asked for.
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
        assert_eq!(identifier("{0.0.0}"), identifier("{0.0.0}"));
        assert_ne!(identifier("{0.0.0}"), identifier("{0.0.1}"));
        assert_ne!(identifier(""), 0, "no name may take the reserved value");
    }

    #[test]
    fn the_backend_names_the_host_it_speaks_to() {
        assert_eq!(WasapiBackend::new().name(), "WASAPI");
    }

    #[test]
    fn enumeration_fits_the_buffer_it_is_given() {
        let backend = WasapiBackend::new();
        let mut devices = [blank(); MAX_DEVICES];
        // A machine with no sound card reports none rather than failing.
        let count = backend.devices(&mut devices).unwrap_or(0);
        assert!(count <= MAX_DEVICES);
        for device in &devices[..count] {
            assert_ne!(device.id.0, 0);
            assert_eq!(device.channels, 2);
            assert!(device.rates.contains(48_000));
            assert!(!device.name.is_empty());
        }
        let mut single = [blank(); 1];
        let written = backend.devices(&mut single).unwrap_or(0);
        assert_eq!(written, count.min(1));
    }

    #[test]
    fn inputs_are_listed_apart_from_outputs() {
        let backend = WasapiBackend::new();
        let mut inputs = [blank(); MAX_DEVICES];
        let count = backend.input_devices(&mut inputs).unwrap_or(0);
        for device in &inputs[..count] {
            assert_eq!(device.direction, Direction::Input);
            assert_ne!(device.id.0, 0);
            assert!(!device.name.is_empty());
        }
        // A machine that has a default input must list it.
        if let Ok(default) = backend.default_input() {
            assert!(
                inputs[..count].iter().any(|device| device.id == default),
                "the default input was not listed"
            );
        }
    }

    #[test]
    fn opening_an_input_that_is_not_there_is_refused() {
        let backend = WasapiBackend::new();
        let config = StreamConfig {
            device: DeviceId(0xdead_beef),
            sample_rate: 48_000,
            channels: 2,
            block_frames: 512,
        };
        let frames = Arc::new(AtomicU64::new(0));
        assert!(matches!(
            backend.open_input(
                config,
                CountingCapture {
                    frames: Arc::clone(&frames)
                }
            ),
            Err(AudioError::DeviceMissing | AudioError::Host(_))
        ));
        assert_eq!(frames.load(Ordering::Relaxed), 0);
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
    #[ignore = "needs an input device"]
    fn a_stream_records_from_the_default_input() {
        let backend = WasapiBackend::new();
        let device = backend.default_input().expect("no default input");
        let config = StreamConfig {
            device,
            sample_rate: 48_000,
            channels: 2,
            block_frames: 512,
        };
        let frames = Arc::new(AtomicU64::new(0));
        let mut stream = backend
            .open_input(
                config,
                CountingCapture {
                    frames: Arc::clone(&frames),
                },
            )
            .expect("open failed");
        assert!(stream.start().is_ok());
        let deadline = std::time::Instant::now() + core::time::Duration::from_secs(3);
        while frames.load(Ordering::Relaxed) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(core::time::Duration::from_millis(10));
        }
        assert!(
            frames.load(Ordering::Relaxed) > 0,
            "the stream never reached the capturer"
        );
        assert!(stream.stop().is_ok());
    }

    #[test]
    fn opening_a_device_that_is_not_there_is_refused() {
        let backend = WasapiBackend::new();
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
        assert!(matches!(
            backend.open_output(config, renderer),
            Err(AudioError::DeviceMissing | AudioError::Host(_))
        ));
        assert_eq!(frames.load(Ordering::Relaxed), 0);
    }

    #[test]
    #[ignore = "needs an output device"]
    fn a_stream_renders_into_the_default_device() {
        let backend = WasapiBackend::new();
        let device = backend.default_output().expect("no default output");
        let config = StreamConfig {
            device,
            sample_rate: 48_000,
            channels: 2,
            block_frames: 512,
        };
        let frames = Arc::new(AtomicU64::new(0));
        let renderer = Counting {
            frames: Arc::clone(&frames),
        };
        let mut stream = backend.open_output(config, renderer).expect("open failed");
        assert!(!stream.is_running());
        assert!(stream.start().is_ok());
        assert_eq!(stream.start(), Err(AudioError::WrongState));

        let deadline = std::time::Instant::now() + core::time::Duration::from_secs(2);
        while frames.load(Ordering::Relaxed) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(core::time::Duration::from_millis(10));
        }
        assert!(
            frames.load(Ordering::Relaxed) > 0,
            "the stream never called the renderer"
        );
        assert!(stream.frames_rendered() > 0);
        // The audio class is asked for but not required.
        let _ = stream.is_realtime();
        assert!(stream.stop().is_ok());
    }
}
