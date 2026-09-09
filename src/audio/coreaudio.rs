//! Output through the platform's audio system on macOS.
//!
//! This drives an audio unit of the default output kind. The unit calls
//! back from a thread the system owns and schedules ahead of everything
//! else on the machine; that callback is the audio thread, and everything
//! the real-time rules forbid is forbidden inside it.
//!
//! The renderer is handed over when the stream opens and lives in a box
//! the callback reaches through a raw pointer. The box is kept alive by
//! the stream, and the unit is stopped and taken apart before the box is
//! dropped, so the callback can never see freed storage.

#![cfg(target_os = "macos")]

use super::{
    AudioError, Backend, BlockTiming, Capturer, DeviceId, DeviceInfo, Direction, InputBackend,
    Name, Rates, Renderer, Stream, StreamConfig,
};
use crate::mixer::MAX_FRAMES;
use core::ffi::{c_char, c_void};
use core::ptr;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

// Types and constants from the platform's audio headers. They are
// reproduced here rather than bound from a crate so the build needs no
// dependency; each one is annotated with what the header calls it.

#[allow(non_camel_case_types)]
type OSStatus = i32;
#[allow(non_camel_case_types)]
type AudioObjectID = u32;
#[allow(non_camel_case_types)]
type AudioUnitElement = u32;
#[allow(non_camel_case_types)]
type AudioUnitScope = u32;
#[allow(non_camel_case_types)]
type AudioUnitPropertyID = u32;
#[allow(non_camel_case_types)]
type AudioFormatID = u32;
#[allow(non_camel_case_types)]
type AudioFormatFlags = u32;
#[allow(non_camel_case_types)]
type AudioUnit = *mut c_void;
#[allow(non_camel_case_types)]
type AudioComponent = *mut c_void;

const NO_ERROR: OSStatus = 0;

/// `AudioStreamBasicDescription`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct StreamDescription {
    sample_rate: f64,
    format_id: AudioFormatID,
    format_flags: AudioFormatFlags,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

/// `AudioComponentDescription`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ComponentDescription {
    component_type: u32,
    component_subtype: u32,
    manufacturer: u32,
    flags: u32,
    flags_mask: u32,
}

/// `AudioBuffer`.
#[repr(C)]
struct Buffer {
    channels: u32,
    data_byte_size: u32,
    data: *mut c_void,
}

/// The head of an `AudioBufferList`, used only to find where its buffers
/// begin. The real list carries as many buffers as the count says.
#[repr(C)]
struct BufferListHeader {
    count: u32,
    first: Buffer,
}

/// `AudioBufferList` with room for the channels a stereo unit provides.
#[repr(C)]
struct BufferList {
    count: u32,
    buffers: [Buffer; 2],
}

/// `AudioTimeStamp`.
#[repr(C)]
struct TimeStamp {
    sample_time: f64,
    host_time: u64,
    rate_scalar: f64,
    word_clock_time: u64,
    smpte_time: [u8; 16],
    flags: u32,
    reserved: u32,
}

/// `AURenderCallbackStruct`.
#[repr(C)]
struct RenderCallbackStruct {
    proc_: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *mut u32,
            *const TimeStamp,
            u32,
            u32,
            *mut BufferList,
        ) -> OSStatus,
    >,
    ref_con: *mut c_void,
}

/// `AudioObjectPropertyAddress`.
#[repr(C)]
#[derive(Clone, Copy)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

/// Builds the four-character code the platform headers use as constants.
const fn four_cc(code: &[u8; 4]) -> u32 {
    ((code[0] as u32) << 24) | ((code[1] as u32) << 16) | ((code[2] as u32) << 8) | (code[3] as u32)
}

const COMPONENT_TYPE_OUTPUT: u32 = four_cc(b"auou");
// The hardware abstraction unit, rather than the default output unit,
// because only this one lets a particular device be chosen.
const COMPONENT_SUBTYPE_HAL_OUTPUT: u32 = four_cc(b"ahal");
const MANUFACTURER_APPLE: u32 = four_cc(b"appl");

const FORMAT_LINEAR_PCM: AudioFormatID = four_cc(b"lpcm");
const FLAG_IS_FLOAT: AudioFormatFlags = 1 << 0;
const FLAG_IS_PACKED: AudioFormatFlags = 1 << 3;
const FLAG_IS_NON_INTERLEAVED: AudioFormatFlags = 1 << 5;

const SCOPE_GLOBAL: AudioUnitScope = 0;
const SCOPE_INPUT: AudioUnitScope = 1;
const SCOPE_OUTPUT: AudioUnitScope = 2;
const ELEMENT_OUTPUT: AudioUnitElement = 0;

const PROPERTY_STREAM_FORMAT: AudioUnitPropertyID = 8;
const PROPERTY_SET_RENDER_CALLBACK: AudioUnitPropertyID = 23;
const PROPERTY_SET_INPUT_CALLBACK: AudioUnitPropertyID = 2005;
const PROPERTY_MAXIMUM_FRAMES: AudioUnitPropertyID = 14;
const PROPERTY_CURRENT_DEVICE: AudioUnitPropertyID = 2000;
const PROPERTY_ENABLE_IO: AudioUnitPropertyID = 2003;
const ELEMENT_INPUT: AudioUnitElement = 1;

const OBJECT_SYSTEM: AudioObjectID = 1;
const SELECTOR_DEFAULT_OUTPUT_DEVICE: u32 = four_cc(b"dOut");
const SELECTOR_DEFAULT_INPUT_DEVICE: u32 = four_cc(b"dIn ");
const SELECTOR_DEVICES: u32 = four_cc(b"dev#");
const SELECTOR_DEVICE_NAME: u32 = four_cc(b"lnam");
const SELECTOR_STREAM_CONFIGURATION: u32 = four_cc(b"slay");
const SELECTOR_NOMINAL_SAMPLE_RATE: u32 = four_cc(b"nsrt");
const SELECTOR_BUFFER_FRAME_SIZE: u32 = four_cc(b"fsiz");
const SCOPE_OBJECT_GLOBAL: u32 = four_cc(b"glob");
const SCOPE_OBJECT_OUTPUT: u32 = four_cc(b"outp");
const SCOPE_OBJECT_INPUT: u32 = four_cc(b"inpt");
const ELEMENT_MAIN: u32 = 0;

// SAFETY: These are the platform's audio entry points, declared with the
// signatures its headers give them. Each call below documents the
// invariants it upholds.
unsafe extern "C" {
    fn AudioUnitRender(
        unit: AudioUnit,
        flags: *mut u32,
        timestamp: *const TimeStamp,
        bus: u32,
        frames: u32,
        buffers: *mut BufferList,
    ) -> OSStatus;
    fn AudioComponentFindNext(
        in_component: AudioComponent,
        description: *const ComponentDescription,
    ) -> AudioComponent;
    fn AudioComponentInstanceNew(component: AudioComponent, unit: *mut AudioUnit) -> OSStatus;
    fn AudioComponentInstanceDispose(unit: AudioUnit) -> OSStatus;
    fn AudioUnitInitialize(unit: AudioUnit) -> OSStatus;
    fn AudioUnitUninitialize(unit: AudioUnit) -> OSStatus;
    fn AudioUnitSetProperty(
        unit: AudioUnit,
        property: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        data: *const c_void,
        size: u32,
    ) -> OSStatus;
    fn AudioUnitGetProperty(
        unit: AudioUnit,
        property: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        data: *mut c_void,
        size: *mut u32,
    ) -> OSStatus;
    fn AudioOutputUnitStart(unit: AudioUnit) -> OSStatus;
    fn AudioOutputUnitStop(unit: AudioUnit) -> OSStatus;
    fn AudioObjectGetPropertyData(
        object: AudioObjectID,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> OSStatus;
    fn AudioObjectGetPropertyDataSize(
        object: AudioObjectID,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> OSStatus;
    fn AudioObjectSetPropertyData(
        object: AudioObjectID,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> OSStatus;
    fn CFStringGetCString(
        string: *const c_void,
        buffer: *mut c_char,
        size: isize,
        encoding: u32,
    ) -> bool;
    fn CFRelease(item: *const c_void);
}

const ENCODING_UTF8: u32 = 0x0800_0100;

/// Shared between the stream and its callback.
///
/// The renderer is reached through a pointer the callback holds. The
/// counters are atomics because the callback writes them while the
/// control thread reads them.
struct Shared {
    renderer: *mut dyn Renderer,
    frames: AtomicU64,
    dropouts: AtomicU64,
    // Frames the unit asked for last, so an oversized request can be
    // reported rather than silently truncated.
    largest_request: AtomicUsize,
}

// SAFETY: Only the audio callback touches the renderer, and the stream
// keeps it alive and stops the unit before dropping it. The counters are
// atomic.
unsafe impl Send for Shared {}
// SAFETY: As for `Send`; the shared state is reachable from both threads
// but only the atomics are read on the control thread.
unsafe impl Sync for Shared {}

/// The platform's audio system as a backend.
#[derive(Clone, Copy, Debug, Default)]
pub struct CoreAudioBackend;

impl CoreAudioBackend {
    /// A backend over the system's audio devices.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn default_input_id() -> Result<AudioObjectID, AudioError> {
        Self::default_device_id(SELECTOR_DEFAULT_INPUT_DEVICE)
    }

    fn default_output_id() -> Result<AudioObjectID, AudioError> {
        Self::default_device_id(SELECTOR_DEFAULT_OUTPUT_DEVICE)
    }

    /// The system's current default device for one direction.
    fn default_device_id(selector: u32) -> Result<AudioObjectID, AudioError> {
        let address = PropertyAddress {
            selector,
            scope: SCOPE_OBJECT_GLOBAL,
            element: ELEMENT_MAIN,
        };
        let mut device: AudioObjectID = 0;
        let mut size = size_of::<AudioObjectID>() as u32;
        // SAFETY: The address and destination are valid for the sizes given.
        let status = unsafe {
            AudioObjectGetPropertyData(
                OBJECT_SYSTEM,
                &address,
                0,
                ptr::null(),
                &mut size,
                (&raw mut device).cast(),
            )
        };
        if status != NO_ERROR || device == 0 {
            return Err(AudioError::DeviceMissing);
        }
        Ok(device)
    }

    /// Output channels the device carries, or zero when it has none.
    fn output_channels(device: AudioObjectID) -> u32 {
        Self::channels(device, SCOPE_OBJECT_OUTPUT)
    }

    /// Channels a device carries in one direction. Zero means it carries
    /// none, which is how an output-only device is told from an input.
    fn channels(device: AudioObjectID, scope: u32) -> u32 {
        let address = PropertyAddress {
            selector: SELECTOR_STREAM_CONFIGURATION,
            scope,
            element: ELEMENT_MAIN,
        };
        let mut size: u32 = 0;
        // SAFETY: Asking for the size writes only to `size`.
        let status =
            unsafe { AudioObjectGetPropertyDataSize(device, &address, 0, ptr::null(), &mut size) };
        if status != NO_ERROR || size == 0 {
            return 0;
        }
        // The configuration is an `AudioBufferList` whose length varies, so
        // it is read into a byte buffer sized by the query above.
        let mut bytes = vec![0_u8; size as usize];
        // SAFETY: The buffer is at least `size` bytes, which is what the
        // system reported it needs.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                ptr::null(),
                &mut size,
                bytes.as_mut_ptr().cast(),
            )
        };
        if status != NO_ERROR || (size as usize) < size_of::<u32>() {
            return 0;
        }
        // SAFETY: The buffer holds a buffer list: a count followed by that
        // many `AudioBuffer` values.
        let count = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<u32>()) };
        let mut channels = 0;
        // The buffers do not begin straight after the count: the list is a
        // C structure, so the first one starts at its own alignment. Taking
        // the count's size instead reads each buffer four bytes early and
        // sees nothing.
        let first = core::mem::offset_of!(BufferListHeader, first);
        for index in 0..count as usize {
            let offset = first + index * size_of::<Buffer>();
            if offset + size_of::<Buffer>() > bytes.len() {
                break;
            }
            // SAFETY: The offset stays inside the buffer, checked above.
            let buffer =
                unsafe { ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<Buffer>()) };
            channels += buffer.channels;
        }
        channels
    }

    /// The device's current sample rate, or zero when it cannot be read.
    fn nominal_rate(device: AudioObjectID) -> u32 {
        let address = PropertyAddress {
            selector: SELECTOR_NOMINAL_SAMPLE_RATE,
            scope: SCOPE_OBJECT_GLOBAL,
            element: ELEMENT_MAIN,
        };
        let mut rate: f64 = 0.0;
        let mut size = size_of::<f64>() as u32;
        // SAFETY: The destination is a single `f64`, matching `size`.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                ptr::null(),
                &mut size,
                (&raw mut rate).cast(),
            )
        };
        if status != NO_ERROR || rate <= 0.0 {
            0
        } else {
            rate as u32
        }
    }

    /// The device's name, empty when it cannot be read.
    fn device_name(device: AudioObjectID) -> Name {
        let address = PropertyAddress {
            selector: SELECTOR_DEVICE_NAME,
            scope: SCOPE_OBJECT_GLOBAL,
            element: ELEMENT_MAIN,
        };
        let mut string: *const c_void = ptr::null();
        let mut size = size_of::<*const c_void>() as u32;
        // SAFETY: The destination holds one pointer, matching `size`.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                ptr::null(),
                &mut size,
                (&raw mut string).cast(),
            )
        };
        if status != NO_ERROR || string.is_null() {
            return Name::new();
        }
        let mut bytes = [0_i8; 256];
        // SAFETY: The buffer is 256 bytes and the length passed matches it.
        let copied = unsafe {
            CFStringGetCString(
                string,
                bytes.as_mut_ptr().cast::<c_char>(),
                bytes.len() as isize,
                ENCODING_UTF8,
            )
        };
        // SAFETY: The string was returned with a reference this call owns.
        unsafe { CFRelease(string) };
        if !copied {
            return Name::new();
        }
        let length = bytes.iter().position(|byte| *byte == 0).unwrap_or(0);
        // SAFETY: The bytes up to the terminator came from a UTF-8 request.
        let text = unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                bytes.as_ptr().cast::<u8>(),
                length,
            ))
        };
        Name::truncated(text)
    }

    /// Asks the device for a buffer size and reports what it granted.
    ///
    /// A device is free to refuse or round the request, so the caller uses
    /// the returned figure rather than the one it asked for.
    fn negotiate_buffer_size(device: AudioObjectID, wanted: u32) -> u32 {
        let address = PropertyAddress {
            selector: SELECTOR_BUFFER_FRAME_SIZE,
            scope: SCOPE_OBJECT_GLOBAL,
            element: ELEMENT_MAIN,
        };
        // SAFETY: The property takes one `u32`, which is what is passed.
        unsafe {
            AudioObjectSetPropertyData(
                device,
                &address,
                0,
                ptr::null(),
                size_of::<u32>() as u32,
                (&raw const wanted).cast(),
            );
        }
        let mut granted: u32 = 0;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: The destination is one `u32`, matching `size`.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                ptr::null(),
                &mut size,
                (&raw mut granted).cast(),
            )
        };
        if status != NO_ERROR || granted == 0 {
            wanted
        } else {
            granted
        }
    }

    fn all_device_ids(out: &mut Vec<AudioObjectID>) -> Result<(), AudioError> {
        let address = PropertyAddress {
            selector: SELECTOR_DEVICES,
            scope: SCOPE_OBJECT_GLOBAL,
            element: ELEMENT_MAIN,
        };
        let mut size: u32 = 0;
        // SAFETY: Asking for the size writes only to `size`.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(OBJECT_SYSTEM, &address, 0, ptr::null(), &mut size)
        };
        if status != NO_ERROR {
            return Err(AudioError::Host("device list unavailable"));
        }
        let count = size as usize / size_of::<AudioObjectID>();
        out.clear();
        out.resize(count, 0);
        if count == 0 {
            return Ok(());
        }
        // SAFETY: The destination holds `count` identifiers, which is the
        // size the system reported.
        let status = unsafe {
            AudioObjectGetPropertyData(
                OBJECT_SYSTEM,
                &address,
                0,
                ptr::null(),
                &mut size,
                out.as_mut_ptr().cast(),
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Host("device list unavailable"));
        }
        Ok(())
    }
}

/// The callback the audio unit calls once per block.
///
/// Everything here runs on the audio thread.
///
/// # Safety
///
/// The system passes the reference it was given at
/// `PROPERTY_SET_RENDER_CALLBACK` time, which is the stream's shared
/// state, kept alive until after the unit is stopped.
unsafe extern "C" fn render_callback(
    ref_con: *mut c_void,
    _flags: *mut u32,
    _timestamp: *const TimeStamp,
    _bus: u32,
    frames: u32,
    buffers: *mut BufferList,
) -> OSStatus {
    if ref_con.is_null() || buffers.is_null() {
        return NO_ERROR;
    }
    // SAFETY: The pointer is the shared state handed to the unit, which
    // outlives the callback.
    let shared = unsafe { &*ref_con.cast::<Shared>() };
    // SAFETY: The system provides a buffer list with at least one buffer.
    let list = unsafe { &mut *buffers };

    let frames = frames as usize;
    shared.largest_request.fetch_max(frames, Ordering::Relaxed);
    if frames > MAX_FRAMES {
        // Larger than anything the engine will render. Produce silence
        // rather than a partly filled block, and count it as a gap.
        shared.dropouts.fetch_add(1, Ordering::Relaxed);
        for index in 0..list.count.min(2) as usize {
            let buffer = &list.buffers[index];
            if !buffer.data.is_null() {
                // SAFETY: The system reports the size it allocated.
                unsafe {
                    ptr::write_bytes(buffer.data.cast::<u8>(), 0, buffer.data_byte_size as usize);
                }
            }
        }
        return NO_ERROR;
    }

    // The unit is configured for two non-interleaved channels, so the
    // engine renders into a stack block and the channels are split out.
    let mut block = [[0.0_f32; 2]; MAX_FRAMES];
    let timing = BlockTiming {
        frame: shared.frames.load(Ordering::Relaxed),
        dropouts: shared.dropouts.load(Ordering::Relaxed),
    };
    // SAFETY: The renderer is owned by the stream, which stops the unit
    // before dropping it, so no other reference exists during this call.
    let renderer = unsafe { &mut *shared.renderer };
    renderer.render(&mut block[..frames], timing);

    let channels = list.count.min(2) as usize;
    for (channel, buffer) in list.buffers.iter().take(channels).enumerate() {
        if buffer.data.is_null() {
            continue;
        }
        let wanted = frames * size_of::<f32>();
        if (buffer.data_byte_size as usize) < wanted {
            continue;
        }
        let destination = buffer.data.cast::<f32>();
        for (index, frame) in block[..frames].iter().enumerate() {
            // SAFETY: The buffer holds at least `frames` samples, checked
            // just above.
            unsafe { destination.add(index).write(frame[channel]) };
        }
    }
    shared.frames.fetch_add(frames as u64, Ordering::Relaxed);
    NO_ERROR
}

impl Backend for CoreAudioBackend {
    type Stream = CoreAudioStream;

    fn name(&self) -> &'static str {
        "CoreAudio"
    }

    fn devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
        let mut ids = Vec::new();
        Self::all_device_ids(&mut ids)?;
        let default = Self::default_output_id().unwrap_or(0);
        let mut written = 0;
        for id in ids {
            if written == out.len() {
                break;
            }
            let channels = Self::output_channels(id);
            if channels == 0 {
                continue;
            }
            let rate = Self::nominal_rate(id);
            let mut rates = Rates::new();
            if rate != 0 {
                let _ = rates.push(rate);
            }
            for candidate in super::SUPPORTED_RATES {
                if candidate != rate {
                    let _ = rates.push(candidate);
                }
            }
            out[written] = DeviceInfo {
                id: DeviceId(u64::from(id)),
                name: Self::device_name(id),
                direction: Direction::Output,
                channels: channels.min(u32::from(u16::MAX)) as u16,
                rates,
                is_default: id == default,
            };
            written += 1;
        }
        Ok(written)
    }

    fn default_output(&self) -> Result<DeviceId, AudioError> {
        Self::default_output_id().map(|id| DeviceId(u64::from(id)))
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

        // A device of zero means whatever the system currently defaults to.
        let device = if config.device.0 == 0 {
            Self::default_output_id()?
        } else {
            u32::try_from(config.device.0).map_err(|_| AudioError::DeviceMissing)?
        };

        let description = ComponentDescription {
            component_type: COMPONENT_TYPE_OUTPUT,
            component_subtype: COMPONENT_SUBTYPE_HAL_OUTPUT,
            manufacturer: MANUFACTURER_APPLE,
            flags: 0,
            flags_mask: 0,
        };
        // SAFETY: The description is a valid value of the expected type.
        let component = unsafe { AudioComponentFindNext(ptr::null_mut(), &description) };
        if component.is_null() {
            return Err(AudioError::Host("no output component"));
        }
        let mut unit: AudioUnit = ptr::null_mut();
        // SAFETY: `unit` receives the new instance.
        let status = unsafe { AudioComponentInstanceNew(component, &mut unit) };
        if status != NO_ERROR || unit.is_null() {
            return Err(AudioError::Host("the output unit could not be created"));
        }

        // From here on the unit must be disposed of on every failure.
        let mut stream = CoreAudioStream {
            unit,
            initialized: false,
            running: false,
            config,
            shared: Box::new(Shared {
                renderer: ptr::null_mut::<PlaceholderRenderer>(),
                frames: AtomicU64::new(0),
                dropouts: AtomicU64::new(0),
                largest_request: AtomicUsize::new(0),
            }),
            renderer: None,
        };

        let mut prepared = renderer;
        prepared.prepare(config);
        let boxed: Box<dyn Renderer> = Box::new(prepared);
        let mut boxed = boxed;
        stream.shared.renderer = &raw mut *boxed;
        stream.renderer = Some(boxed);

        // The hardware unit carries both directions; only output is wanted.
        let enable: u32 = 1;
        // SAFETY: The property takes one `u32`.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_ENABLE_IO,
                SCOPE_OUTPUT,
                ELEMENT_OUTPUT,
                (&raw const enable).cast(),
                size_of::<u32>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Host("output could not be enabled"));
        }
        let disable: u32 = 0;
        // SAFETY: As above; failing here is not fatal, since input is off
        // by default on an output unit.
        unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_ENABLE_IO,
                SCOPE_INPUT,
                ELEMENT_INPUT,
                (&raw const disable).cast(),
                size_of::<u32>() as u32,
            );
        }

        // SAFETY: The property takes one device identifier.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_CURRENT_DEVICE,
                SCOPE_GLOBAL,
                ELEMENT_OUTPUT,
                (&raw const device).cast(),
                size_of::<AudioObjectID>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::DeviceMissing);
        }

        // The device decides its own buffer size; asking is a request.
        let granted = Self::negotiate_buffer_size(device, config.block_frames as u32);
        stream.config.block_frames = (granted as usize).clamp(super::MIN_BLOCK, MAX_FRAMES);

        let format = StreamDescription {
            sample_rate: f64::from(config.sample_rate),
            format_id: FORMAT_LINEAR_PCM,
            format_flags: FLAG_IS_FLOAT | FLAG_IS_PACKED | FLAG_IS_NON_INTERLEAVED,
            bytes_per_packet: size_of::<f32>() as u32,
            frames_per_packet: 1,
            bytes_per_frame: size_of::<f32>() as u32,
            channels_per_frame: u32::from(config.channels),
            bits_per_channel: 32,
            reserved: 0,
        };
        // SAFETY: The property takes a stream description of this size.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_STREAM_FORMAT,
                SCOPE_INPUT,
                ELEMENT_OUTPUT,
                (&raw const format).cast(),
                size_of::<StreamDescription>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Unsupported("stream format"));
        }

        // The largest block the renderer can fill. Setting this to the
        // requested size would break rendering whenever the device asked
        // for more, so it is set to the most the engine can ever handle.
        let maximum = MAX_FRAMES as u32;
        // SAFETY: The property takes one `u32`.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_MAXIMUM_FRAMES,
                SCOPE_GLOBAL,
                ELEMENT_OUTPUT,
                (&raw const maximum).cast(),
                size_of::<u32>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Unsupported("block size"));
        }

        let callback = RenderCallbackStruct {
            proc_: Some(render_callback),
            ref_con: (&raw mut *stream.shared).cast(),
        };
        // SAFETY: The property takes a callback structure of this size, and
        // the reference it carries outlives the unit.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_SET_RENDER_CALLBACK,
                SCOPE_INPUT,
                ELEMENT_OUTPUT,
                (&raw const callback).cast(),
                size_of::<RenderCallbackStruct>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Host("the render callback was refused"));
        }

        // SAFETY: The unit is configured and not yet running.
        let status = unsafe { AudioUnitInitialize(unit) };
        if status != NO_ERROR {
            return Err(AudioError::Host("the output unit could not start"));
        }
        stream.initialized = true;

        // The device may have granted a different rate than was asked for.
        let mut granted = StreamDescription::default();
        let mut size = size_of::<StreamDescription>() as u32;
        // SAFETY: The destination matches the property's type and size.
        let status = unsafe {
            AudioUnitGetProperty(
                unit,
                PROPERTY_STREAM_FORMAT,
                SCOPE_INPUT,
                ELEMENT_OUTPUT,
                (&raw mut granted).cast(),
                &mut size,
            )
        };
        if status == NO_ERROR && granted.sample_rate > 0.0 {
            stream.config.sample_rate = granted.sample_rate as u32;
        }
        Ok(stream)
    }
}

/// Planar input storage with the alignment required by `AudioUnitRender`.
#[repr(align(16))]
struct CaptureStorage([[f32; MAX_FRAMES]; 2]);

/// Shared state the input callback reads.
struct CaptureShared {
    // The unit the callback pulls from, which the stream keeps alive.
    unit: AudioUnit,
    capturer: *mut dyn Capturer,
    frames: AtomicU64,
    dropouts: AtomicU64,
    // The buffer the unit is asked to fill. It is allocated when the
    // stream opens, so the callback only writes into it.
    channels: usize,
    storage: CaptureStorage,
    block: [[f32; 2]; MAX_FRAMES],
}

// SAFETY: The state is owned by one stream, which stops the unit before
// dropping it, so the callback never runs beside the control thread.
unsafe impl Send for CaptureShared {}

/// Called by the platform when the device has audio to hand over.
///
/// # Safety
///
/// The platform states the arguments are those it documents. Nothing here
/// allocates, locks, or blocks.
unsafe extern "C" fn input_callback(
    ref_con: *mut c_void,
    flags: *mut u32,
    timestamp: *const TimeStamp,
    _bus: u32,
    frames: u32,
    _buffers: *mut BufferList,
) -> OSStatus {
    if ref_con.is_null() {
        return NO_ERROR;
    }
    // SAFETY: The pointer is the shared state handed to the unit, which
    // outlives the callback.
    let shared = unsafe { &mut *ref_con.cast::<CaptureShared>() };
    let count = frames as usize;
    if count > MAX_FRAMES {
        // More than the engine handles. Counting it keeps the gap visible
        // rather than silently dropping part of a take.
        shared.dropouts.fetch_add(1, Ordering::Relaxed);
        return NO_ERROR;
    }

    // The unit hands over audio only when asked for it, into a list that
    // points at storage this stream already owns.
    let mut list = BufferList {
        count: shared.channels as u32,
        buffers: [
            Buffer {
                channels: 1,
                data_byte_size: (count * size_of::<f32>()) as u32,
                data: shared.storage.0[0].as_mut_ptr().cast(),
            },
            Buffer {
                channels: 1,
                data_byte_size: (count * size_of::<f32>()) as u32,
                data: shared.storage.0[1].as_mut_ptr().cast(),
            },
        ],
    };
    // SAFETY: The unit is running and the list points at storage large
    // enough for the frames it was asked for.
    let status = unsafe {
        AudioUnitRender(
            shared.unit,
            flags,
            timestamp,
            ELEMENT_INPUT,
            frames,
            &raw mut list,
        )
    };
    if status != NO_ERROR {
        shared.dropouts.fetch_add(1, Ordering::Relaxed);
        return NO_ERROR;
    }

    // The channels arrive apart; the capturer takes stereo frames.
    // A mono device is heard on both sides rather than only the left.
    let right = usize::from(shared.channels > 1);
    for (index, frame) in shared.block[..count].iter_mut().enumerate() {
        *frame = [shared.storage.0[0][index], shared.storage.0[right][index]];
    }
    let timing = BlockTiming {
        frame: shared.frames.load(Ordering::Relaxed),
        dropouts: shared.dropouts.load(Ordering::Relaxed),
    };
    // SAFETY: The capturer is owned by the stream, which stops the unit
    // before dropping it, so no other reference exists during this call.
    let capturer = unsafe { &mut *shared.capturer };
    capturer.capture(&shared.block[..count], timing);
    shared.frames.fetch_add(count as u64, Ordering::Relaxed);
    NO_ERROR
}

/// Stands in for the capturer pointer before the real one is stored.
struct PlaceholderCapturer;

impl Capturer for PlaceholderCapturer {
    fn capture(&mut self, _input: &[[f32; 2]], _timing: BlockTiming) {}
}

/// Stands in for the renderer pointer before the real one is stored.
struct PlaceholderRenderer;

impl Renderer for PlaceholderRenderer {
    fn render(&mut self, output: &mut [[f32; 2]], _timing: BlockTiming) {
        output.fill([0.0, 0.0]);
    }
}

/// An output stream on the platform's audio system.
pub struct CoreAudioStream {
    unit: AudioUnit,
    initialized: bool,
    running: bool,
    config: StreamConfig,
    // Boxed so its address is stable: the unit holds a pointer to it.
    shared: Box<Shared>,
    // Kept alive for as long as the callback can run.
    renderer: Option<Box<dyn Renderer>>,
}

impl CoreAudioStream {
    /// Largest block the device has asked for. A device that asks for more
    /// than the engine renders reports a dropout rather than a partial
    /// block.
    #[must_use]
    pub fn largest_request(&self) -> usize {
        self.shared.largest_request.load(Ordering::Relaxed)
    }
}

impl Stream for CoreAudioStream {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if self.running {
            return Err(AudioError::WrongState);
        }
        // SAFETY: The unit is initialized and not running.
        let status = unsafe { AudioOutputUnitStart(self.unit) };
        if status != NO_ERROR {
            return Err(AudioError::Host("the output unit refused to start"));
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !self.running {
            return Err(AudioError::WrongState);
        }
        // SAFETY: The unit is running. This returns once the callback has
        // finished, so the renderer is free afterwards.
        let status = unsafe { AudioOutputUnitStop(self.unit) };
        if status != NO_ERROR {
            return Err(AudioError::Host("the output unit refused to stop"));
        }
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn frames_rendered(&self) -> u64 {
        self.shared.frames.load(Ordering::Relaxed)
    }

    fn dropouts(&self) -> u64 {
        self.shared.dropouts.load(Ordering::Relaxed)
    }
}

impl Drop for CoreAudioStream {
    fn drop(&mut self) {
        if self.running {
            // SAFETY: The unit is running; stopping waits for the callback.
            unsafe { AudioOutputUnitStop(self.unit) };
            self.running = false;
        }
        if self.initialized {
            // SAFETY: The unit was initialized and is now stopped.
            unsafe { AudioUnitUninitialize(self.unit) };
            self.initialized = false;
        }
        if !self.unit.is_null() {
            // SAFETY: The instance was created here and is not running.
            unsafe { AudioComponentInstanceDispose(self.unit) };
            self.unit = ptr::null_mut();
        }
        // Only now, with the callback certain to have stopped, is the
        // renderer dropped.
        self.renderer = None;
    }
}

impl InputBackend for CoreAudioBackend {
    type Capture = CoreAudioCapture;

    fn input_devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
        let mut ids = Vec::new();
        Self::all_device_ids(&mut ids)?;
        let default = Self::default_input_id().unwrap_or(0);
        let mut written = 0;
        for id in ids {
            if written == out.len() {
                break;
            }
            let channels = Self::channels(id, SCOPE_OBJECT_INPUT);
            if channels == 0 {
                continue;
            }
            let rate = Self::nominal_rate(id);
            let mut rates = Rates::new();
            if rate != 0 {
                let _ = rates.push(rate);
            }
            for candidate in super::SUPPORTED_RATES {
                if candidate != rate {
                    let _ = rates.push(candidate);
                }
            }
            out[written] = DeviceInfo {
                id: DeviceId(u64::from(id)),
                name: Self::device_name(id),
                direction: Direction::Input,
                channels: channels.min(u32::from(u16::MAX)) as u16,
                rates,
                is_default: id == default,
            };
            written += 1;
        }
        Ok(written)
    }

    fn default_input(&self) -> Result<DeviceId, AudioError> {
        Self::default_input_id().map(|id| DeviceId(u64::from(id)))
    }

    fn open_input<C: Capturer + 'static>(
        &self,
        config: StreamConfig,
        capturer: C,
    ) -> Result<Self::Capture, AudioError> {
        config.validate()?;
        if config.channels == 0 || config.channels > 2 {
            return Err(AudioError::Unsupported("channel count"));
        }

        // A device of zero means whatever the system currently defaults to.
        let device = if config.device.0 == 0 {
            Self::default_input_id()?
        } else {
            u32::try_from(config.device.0).map_err(|_| AudioError::DeviceMissing)?
        };
        let available = Self::channels(device, SCOPE_OBJECT_INPUT);
        if available == 0 {
            return Err(AudioError::DeviceMissing);
        }

        let description = ComponentDescription {
            component_type: COMPONENT_TYPE_OUTPUT,
            component_subtype: COMPONENT_SUBTYPE_HAL_OUTPUT,
            manufacturer: MANUFACTURER_APPLE,
            flags: 0,
            flags_mask: 0,
        };
        // SAFETY: The description is a valid value of the expected type.
        let component = unsafe { AudioComponentFindNext(ptr::null_mut(), &description) };
        if component.is_null() {
            return Err(AudioError::Host("no input component"));
        }
        let mut unit: AudioUnit = ptr::null_mut();
        // SAFETY: `unit` receives the new instance.
        let status = unsafe { AudioComponentInstanceNew(component, &mut unit) };
        if status != NO_ERROR || unit.is_null() {
            return Err(AudioError::Host("the input unit could not be created"));
        }

        // From here on the unit must be disposed of on every failure.
        let taken = usize::from(config.channels).min(available as usize).max(1);
        let mut stream = CoreAudioCapture {
            unit,
            initialized: false,
            running: false,
            config: StreamConfig {
                device: DeviceId(u64::from(device)),
                channels: taken as u16,
                ..config
            },
            shared: Box::new(CaptureShared {
                unit,
                capturer: ptr::null_mut::<PlaceholderCapturer>(),
                frames: AtomicU64::new(0),
                dropouts: AtomicU64::new(0),
                channels: taken,
                storage: CaptureStorage([[0.0; MAX_FRAMES]; 2]),
                block: [[0.0; 2]; MAX_FRAMES],
            }),
            capturer: None,
        };

        // The hardware unit carries both directions; only input is wanted.
        let enable: u32 = 1;
        // SAFETY: The property takes one `u32`.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_ENABLE_IO,
                SCOPE_INPUT,
                ELEMENT_INPUT,
                (&raw const enable).cast(),
                size_of::<u32>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Host("input could not be enabled"));
        }
        let disable: u32 = 0;
        // SAFETY: As above.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_ENABLE_IO,
                SCOPE_OUTPUT,
                ELEMENT_OUTPUT,
                (&raw const disable).cast(),
                size_of::<u32>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Host("output could not be disabled"));
        }

        // SAFETY: The property takes one device identifier.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_CURRENT_DEVICE,
                SCOPE_GLOBAL,
                ELEMENT_OUTPUT,
                (&raw const device).cast(),
                size_of::<AudioObjectID>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::DeviceMissing);
        }

        let granted = Self::negotiate_buffer_size(device, config.block_frames as u32);
        stream.config.block_frames = (granted as usize).clamp(super::MIN_BLOCK, MAX_FRAMES);

        // The format the unit hands audio over in, on the output side of
        // its input element.
        let format = StreamDescription {
            sample_rate: f64::from(config.sample_rate),
            format_id: FORMAT_LINEAR_PCM,
            format_flags: FLAG_IS_FLOAT | FLAG_IS_PACKED | FLAG_IS_NON_INTERLEAVED,
            bytes_per_packet: size_of::<f32>() as u32,
            frames_per_packet: 1,
            bytes_per_frame: size_of::<f32>() as u32,
            channels_per_frame: taken as u32,
            bits_per_channel: 32,
            reserved: 0,
        };
        // SAFETY: The property takes a stream description of this size.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_STREAM_FORMAT,
                SCOPE_OUTPUT,
                ELEMENT_INPUT,
                (&raw const format).cast(),
                size_of::<StreamDescription>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Unsupported("stream format"));
        }

        // The most the callback will ever be handed, which is what the
        // storage in the shared state is sized for.
        let maximum = MAX_FRAMES as u32;
        // SAFETY: The property takes one `u32`.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_MAXIMUM_FRAMES,
                SCOPE_GLOBAL,
                ELEMENT_OUTPUT,
                (&raw const maximum).cast(),
                size_of::<u32>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Unsupported("block size"));
        }

        let callback = RenderCallbackStruct {
            proc_: Some(input_callback),
            ref_con: (&raw mut *stream.shared).cast(),
        };
        // SAFETY: The property takes a callback structure of this size, and
        // the reference it carries outlives the unit.
        let status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROPERTY_SET_INPUT_CALLBACK,
                SCOPE_GLOBAL,
                ELEMENT_OUTPUT,
                (&raw const callback).cast(),
                size_of::<RenderCallbackStruct>() as u32,
            )
        };
        if status != NO_ERROR {
            return Err(AudioError::Host("the input callback was refused"));
        }

        // SAFETY: The unit is configured and not yet running.
        let status = unsafe { AudioUnitInitialize(unit) };
        if status != NO_ERROR {
            return Err(AudioError::Host("the input unit could not start"));
        }
        stream.initialized = true;

        // The device may have granted a different rate than was asked for.
        let mut settled = StreamDescription::default();
        let mut size = size_of::<StreamDescription>() as u32;
        // SAFETY: The destination matches the property's type and size.
        let status = unsafe {
            AudioUnitGetProperty(
                unit,
                PROPERTY_STREAM_FORMAT,
                SCOPE_OUTPUT,
                ELEMENT_INPUT,
                (&raw mut settled).cast(),
                &mut size,
            )
        };
        if status == NO_ERROR && settled.sample_rate > 0.0 {
            stream.config.sample_rate = settled.sample_rate as u32;
        }
        let mut prepared = capturer;
        prepared.prepare(stream.config);
        let boxed: Box<dyn Capturer> = Box::new(prepared);
        let mut boxed = boxed;
        stream.shared.capturer = &raw mut *boxed;
        stream.capturer = Some(boxed);
        Ok(stream)
    }
}

/// An input stream on the platform's audio system.
pub struct CoreAudioCapture {
    unit: AudioUnit,
    initialized: bool,
    running: bool,
    config: StreamConfig,
    // Boxed so its address is stable: the unit holds a pointer to it.
    shared: Box<CaptureShared>,
    // Kept alive for as long as the callback can run.
    capturer: Option<Box<dyn Capturer>>,
}

impl CoreAudioCapture {
    /// Returns the capturer, ending the stream. The unit is stopped first,
    /// so the callback cannot be running when it is handed over.
    #[must_use]
    pub fn into_capturer(mut self) -> Option<Box<dyn Capturer>> {
        if self.running && self.stop().is_err() {
            return None;
        }
        self.capturer.take()
    }
}

impl Stream for CoreAudioCapture {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if self.running {
            return Err(AudioError::WrongState);
        }
        // SAFETY: The unit is initialized and not running.
        let status = unsafe { AudioOutputUnitStart(self.unit) };
        if status != NO_ERROR {
            return Err(AudioError::Host("the input unit refused to start"));
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !self.running {
            return Err(AudioError::WrongState);
        }
        // SAFETY: The unit is running. This returns once the callback has
        // finished, so the capturer is free afterwards.
        let status = unsafe { AudioOutputUnitStop(self.unit) };
        if status != NO_ERROR {
            return Err(AudioError::Host("the input unit refused to stop"));
        }
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn frames_rendered(&self) -> u64 {
        self.shared.frames.load(Ordering::Relaxed)
    }

    fn dropouts(&self) -> u64 {
        self.shared.dropouts.load(Ordering::Relaxed)
    }
}

impl Drop for CoreAudioCapture {
    fn drop(&mut self) {
        if self.running {
            // SAFETY: The unit is running.
            unsafe { AudioOutputUnitStop(self.unit) };
            self.running = false;
        }
        if self.initialized {
            // SAFETY: The unit was initialized and is stopped.
            unsafe { AudioUnitUninitialize(self.unit) };
            self.initialized = false;
        }
        if !self.unit.is_null() {
            // SAFETY: The instance was created by this type.
            unsafe { AudioComponentInstanceDispose(self.unit) };
            self.unit = ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A renderer that records how much it was asked for.
    struct Silent {
        blocks: AtomicU64,
    }

    impl Renderer for Silent {
        fn render(&mut self, output: &mut [[f32; 2]], _timing: BlockTiming) {
            self.blocks.fetch_add(1, Ordering::Relaxed);
            output.fill([0.0, 0.0]);
        }
    }

    #[test]
    fn four_character_codes_match_the_headers() {
        assert_eq!(four_cc(b"auou"), 0x6175_6F75);
        assert_eq!(four_cc(b"lpcm"), 0x6C70_636D);
        assert_eq!(four_cc(b"appl"), 0x6170_706C);
    }

    #[test]
    fn the_backend_names_itself() {
        assert_eq!(CoreAudioBackend::new().name(), "CoreAudio");
    }

    #[test]
    fn a_configuration_that_is_not_stereo_is_refused() {
        let backend = CoreAudioBackend::new();
        let config = StreamConfig {
            device: DeviceId(0),
            sample_rate: 48_000,
            block_frames: 256,
            channels: 1,
        };
        let result = backend.open_output(
            config,
            Silent {
                blocks: AtomicU64::new(0),
            },
        );
        assert_eq!(result.err(), Some(AudioError::Unsupported("channel count")));
    }

    #[test]
    fn a_configuration_the_interface_rejects_never_reaches_the_host() {
        let backend = CoreAudioBackend::new();
        let config = StreamConfig {
            device: DeviceId(0),
            sample_rate: 1_234,
            block_frames: 256,
            channels: 2,
        };
        let result = backend.open_output(
            config,
            Silent {
                blocks: AtomicU64::new(0),
            },
        );
        assert_eq!(result.err(), Some(AudioError::Unsupported("sample rate")));
    }

    /// Enumeration talks to the host, so it is only exercised where a host
    /// exists. It must not crash and must describe whatever it finds.
    #[test]
    fn enumerating_devices_describes_what_the_host_offers() {
        let backend = CoreAudioBackend::new();
        let mut devices = [OfflinePlaceholder::info(); 16];
        match backend.devices(&mut devices) {
            Ok(count) => {
                assert!(count <= devices.len());
                for device in &devices[..count] {
                    assert_eq!(device.direction, Direction::Output);
                    assert!(device.channels > 0);
                    assert!(!device.rates.is_empty());
                }
                // A machine that has a default output must list it. This
                // is what catches an enumeration that reads the device
                // list wrongly and quietly returns nothing.
                if let Ok(default) = backend.default_output() {
                    assert!(
                        devices[..count].iter().any(|device| device.id == default),
                        "the default output was not listed among {count} devices"
                    );
                }
            }
            Err(error) => {
                // A machine with no audio system is a valid outcome.
                assert!(matches!(
                    error,
                    AudioError::Host(_) | AudioError::DeviceMissing
                ));
            }
        }
    }

    /// Opens the default output and runs it briefly, rendering silence.
    ///
    /// This needs a working audio device, so it is not part of the usual
    /// run; a machine without one, such as a build runner, has nothing to
    /// open. Run it with `cargo test --lib coreaudio -- --ignored`.
    #[test]
    #[ignore = "needs a real audio device"]
    fn a_real_device_calls_back_and_advances_the_frame_count() {
        let backend = CoreAudioBackend::new();
        let device = backend
            .default_output()
            .expect("this machine has no default output");
        let config = StreamConfig {
            device,
            sample_rate: 48_000,
            block_frames: 512,
            channels: 2,
        };
        let renderer = Silent {
            blocks: AtomicU64::new(0),
        };
        let mut stream = backend
            .open_output(config, renderer)
            .expect("the default output could not be opened");
        assert!(!stream.is_running());
        assert_eq!(stream.frames_rendered(), 0);

        stream.start().expect("the stream did not start");
        assert!(stream.is_running());
        // Long enough for a device at any block size to call back.
        std::thread::sleep(std::time::Duration::from_millis(250));
        stream.stop().expect("the stream did not stop");

        let frames = stream.frames_rendered();
        assert!(frames > 0, "the device never called back");
        // A quarter second at 48 kHz is 12000 frames; allow wide latitude
        // for scheduling, but the count must be plausible.
        assert!(frames < 48_000 * 2, "{frames} frames in a quarter second");
        assert!(stream.largest_request() <= crate::mixer::MAX_FRAMES);
        assert_eq!(
            stream.dropouts(),
            0,
            "the device asked for oversized blocks"
        );
    }

    #[test]
    fn input_devices_are_listed_apart_from_outputs() {
        let backend = CoreAudioBackend::new();
        let mut inputs = [device_placeholder(); 8];
        let count = backend.input_devices(&mut inputs).unwrap_or(0);
        for device in &inputs[..count] {
            assert_eq!(device.direction, Direction::Input);
            assert!(device.channels > 0, "an input with no channels was listed");
            assert!(!device.name.is_empty());
        }
        // A caller with room for one gets one.
        let mut single = [device_placeholder(); 1];
        assert_eq!(
            backend.input_devices(&mut single).unwrap_or(0),
            count.min(1)
        );
        // A machine that has a default input must list it. This is what
        // catches an enumeration that reads the device list wrongly and
        // quietly returns nothing.
        if let Ok(default) = backend.default_input() {
            assert!(
                inputs[..count].iter().any(|device| device.id == default),
                "the default input was not listed among {count} inputs"
            );
        }
    }

    #[test]
    fn opening_an_input_that_is_not_there_is_refused() {
        let backend = CoreAudioBackend::new();
        let config = StreamConfig {
            device: DeviceId(0xffff_fffe),
            sample_rate: 48_000,
            channels: 2,
            block_frames: 512,
        };
        assert!(matches!(
            backend.open_input(config, SilentCapturer),
            Err(AudioError::DeviceMissing)
        ));
    }

    /// A capturer that keeps nothing, for the paths that never reach a
    /// device.
    struct SilentCapturer;

    impl Capturer for SilentCapturer {
        fn capture(&mut self, _input: &[[f32; 2]], _timing: BlockTiming) {}
    }

    struct CountingCapturer {
        frames: std::sync::Arc<AtomicU64>,
    }

    impl Capturer for CountingCapturer {
        fn capture(&mut self, input: &[[f32; 2]], _timing: BlockTiming) {
            self.frames.fetch_add(input.len() as u64, Ordering::Relaxed);
        }
    }

    #[test]
    #[ignore = "needs a real audio input device and permission"]
    fn a_real_input_delivers_frames() {
        let backend = CoreAudioBackend::new();
        let device = backend.default_input().unwrap();
        let frames = std::sync::Arc::new(AtomicU64::new(0));
        let mut stream = backend
            .open_input(
                StreamConfig {
                    device,
                    sample_rate: 48_000,
                    channels: 2,
                    block_frames: 512,
                },
                CountingCapturer {
                    frames: frames.clone(),
                },
            )
            .unwrap();
        stream.start().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        stream.stop().unwrap();
        assert!(frames.load(Ordering::Relaxed) > 0);
        assert!(stream.frames_rendered() > 0);
    }

    /// An empty entry for a buffer the host fills.
    fn device_placeholder() -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(0),
            name: Name::new(),
            direction: Direction::Input,
            channels: 0,
            rates: Rates::new(),
            is_default: false,
        }
    }

    /// Opening two streams at once must work: one is the main output and
    /// another might be a preview.
    #[test]
    #[ignore = "needs a real audio device"]
    fn two_streams_can_run_at_once() {
        let backend = CoreAudioBackend::new();
        let device = backend.default_output().expect("no default output");
        let config = StreamConfig {
            device,
            sample_rate: 48_000,
            block_frames: 256,
            channels: 2,
        };
        let mut first = backend
            .open_output(
                config,
                Silent {
                    blocks: AtomicU64::new(0),
                },
            )
            .expect("the first stream failed");
        let mut second = backend
            .open_output(
                config,
                Silent {
                    blocks: AtomicU64::new(0),
                },
            )
            .expect("the second stream failed");
        first.start().unwrap();
        second.start().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        first.stop().unwrap();
        second.stop().unwrap();
        assert!(first.frames_rendered() > 0);
        assert!(second.frames_rendered() > 0);
    }

    /// The engine that will actually feed the device must drive it too.
    #[test]
    #[ignore = "needs a real audio device"]
    fn the_playback_engine_drives_a_real_device() {
        use crate::engine::playback::{MixSettings, PlaybackEngine, TrackSettings};

        let backend = CoreAudioBackend::new();
        let device = backend.default_output().expect("no default output");
        let (engine, mut publisher) = PlaybackEngine::new(48_000.0);
        let mut settings = MixSettings::new();
        settings.set_track_count(4);
        for index in 0..4 {
            settings.set_track(index, TrackSettings::default());
        }
        assert!(publisher.publish(&settings));

        let config = StreamConfig {
            device,
            sample_rate: 48_000,
            block_frames: 256,
            channels: 2,
        };
        let mut stream = backend
            .open_output(config, engine)
            .expect("the engine could not open the device");
        stream.start().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        stream.stop().unwrap();

        assert!(stream.frames_rendered() > 0, "the device never called back");
        // The engine reported its state back while the device ran.
        let state = publisher.state();
        assert_eq!(state.track_count, 4);
        assert!(!state.playing, "the transport was never started");
    }

    /// Plays a short chord through the speakers, at a quiet level.
    ///
    /// This is the end-to-end check: notes on a timeline, through the
    /// instrument, the mixer, and a real device. It makes a sound, so it
    /// is only run deliberately.
    #[test]
    #[ignore = "plays audio through the speakers"]
    fn a_chord_plays_through_the_speakers() {
        use crate::engine::playback::{MixSettings, PlaybackEngine, Score, TrackSettings};
        use crate::engine::schedule::ScheduledNote;

        let backend = CoreAudioBackend::new();
        let device = backend.default_output().expect("no default output");
        let (engine, mut publisher) = PlaybackEngine::new(48_000.0);

        let mut settings = MixSettings::new();
        settings.set_track_count(3);
        for index in 0..3 {
            settings.set_track(
                index,
                TrackSettings {
                    // Quiet, so a deliberate test is not startling.
                    volume_db: -18.0,
                    ..TrackSettings::default()
                },
            );
        }
        settings.set_playing(true);
        settings.set_locate_beats(Some(0.0));
        assert!(publisher.publish(&settings));

        let mut score = Score::new();
        // A major triad, each note held for two beats.
        for (index, pitch) in [60_u8, 64, 67].into_iter().enumerate() {
            let track = score.track_mut(index).unwrap();
            track.set_enabled(true);
            track.set_notes(&[ScheduledNote {
                start_beats: 0.0,
                length_beats: 2.0,
                pitch,
                velocity: 90,
            }]);
        }
        assert!(publisher.publish_score(&score));

        let config = StreamConfig {
            device,
            sample_rate: 48_000,
            block_frames: 256,
            channels: 2,
        };
        let mut stream = backend.open_output(config, engine).expect("device refused");
        stream.start().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1_500));
        stream.stop().unwrap();

        assert!(stream.frames_rendered() > 0, "the device never called back");
        let state = publisher.state();
        assert_eq!(state.track_count, 3);
        // The meters saw the chord, which is the proof it was audible
        // rather than merely rendered.
        let heard = state.levels[..3]
            .iter()
            .any(|levels| levels.peak_left > 0.0 || levels.peak_right > 0.0);
        assert!(heard, "no track produced any level");
    }

    /// Supplies a value to fill the enumeration buffer with.
    struct OfflinePlaceholder;
    impl OfflinePlaceholder {
        fn info() -> DeviceInfo {
            DeviceInfo {
                id: DeviceId(0),
                name: Name::new(),
                direction: Direction::Output,
                channels: 0,
                rates: Rates::new(),
                is_default: false,
            }
        }
    }
}
