//! CLAP instance lifecycle and allocation-free block processing.

use super::probe::clap_binary;
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::{
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE, clap_event_header, clap_event_param_value,
    clap_input_events, clap_output_events,
};
use clap_sys::ext::audio_ports::{
    CLAP_EXT_AUDIO_PORTS, clap_audio_port_info, clap_plugin_audio_ports,
};
use clap_sys::ext::latency::{CLAP_EXT_LATENCY, clap_host_latency, clap_plugin_latency};
use clap_sys::ext::params::{
    CLAP_EXT_PARAMS, clap_host_params, clap_param_clear_flags, clap_param_info,
    clap_param_rescan_flags, clap_plugin_params,
};
use clap_sys::ext::state::{CLAP_EXT_STATE, clap_host_state, clap_plugin_state};
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::process::{CLAP_PROCESS_ERROR, clap_process, clap_process_status};
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::{CLAP_VERSION, clap_version_is_compatible};
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicU8, Ordering};

const RESTART_REQUESTED: u8 = 1;
const PROCESS_REQUESTED: u8 = 2;
const CALLBACK_REQUESTED: u8 = 4;
const PARAMETER_RESCAN_REQUESTED: u8 = 8;
const PARAMETER_CLEAR_REQUESTED: u8 = 16;
const PARAMETER_FLUSH_REQUESTED: u8 = 32;
const LATENCY_CHANGED: u8 = 64;
const STATE_DIRTY: u8 = 128;
pub const MAX_PARAMETER_EVENTS: usize = 1_024;
pub const MAX_STATE_BYTES: usize = 256 * 1024 * 1024;
const MAX_PARAMETERS: u32 = 16_384;

const EMPTY_PARAMETER_EVENT: clap_event_param_value = clap_event_param_value {
    header: clap_event_header {
        size: std::mem::size_of::<clap_event_param_value>() as u32,
        time: 0,
        space_id: CLAP_CORE_EVENT_SPACE_ID,
        type_: CLAP_EVENT_PARAM_VALUE,
        flags: 0,
    },
    param_id: 0,
    cookie: ptr::null_mut(),
    note_id: -1,
    port_index: -1,
    channel: -1,
    key: -1,
    value: 0.0,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostRequests {
    pub restart: bool,
    pub process: bool,
    pub callback: bool,
    pub parameter_rescan: bool,
    pub parameter_clear: bool,
    pub parameter_flush: bool,
    pub latency_changed: bool,
    pub state_dirty: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterEvent {
    pub sample_offset: u32,
    pub identifier: u32,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterInfo {
    pub identifier: u32,
    pub flags: u32,
    pub name: String,
    pub module: String,
    pub minimum: f64,
    pub maximum: f64,
    pub default_value: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    MissingBinary,
    InvalidPath,
    Load(String),
    MissingEntry,
    IncompatibleEntry,
    EntryInitialization,
    MissingFactory,
    InvalidFactory,
    PluginCreation,
    InvalidPlugin,
    PluginInitialization,
    InvalidConfiguration,
    UnsupportedPorts,
    UnsupportedParameters,
    InvalidParameters,
    UnsupportedState,
    StateIo,
    StateTooLarge,
    Activation,
    StartProcessing,
    NotProcessing,
    InvalidBlock,
    Processing,
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBinary => output.write_str("Plugin binary is missing"),
            Self::InvalidPath => output.write_str("Plugin path or identifier is invalid"),
            Self::Load(message) => write!(output, "Dynamic library load failed: {message}"),
            Self::MissingEntry => output.write_str("CLAP entry symbol is missing"),
            Self::IncompatibleEntry => output.write_str("CLAP entry version is incompatible"),
            Self::EntryInitialization => output.write_str("CLAP entry initialization failed"),
            Self::MissingFactory => output.write_str("CLAP plugin factory is missing"),
            Self::InvalidFactory => output.write_str("CLAP plugin factory is incomplete"),
            Self::PluginCreation => output.write_str("CLAP plugin creation failed"),
            Self::InvalidPlugin => output.write_str("CLAP plugin interface is incomplete"),
            Self::PluginInitialization => output.write_str("CLAP plugin initialization failed"),
            Self::InvalidConfiguration => {
                output.write_str("CLAP processing configuration is invalid")
            }
            Self::UnsupportedPorts => output.write_str("CLAP plugin needs unsupported audio ports"),
            Self::UnsupportedParameters => {
                output.write_str("CLAP plugin does not expose parameters")
            }
            Self::InvalidParameters => output.write_str("CLAP plugin parameters are invalid"),
            Self::UnsupportedState => output.write_str("CLAP plugin does not expose state"),
            Self::StateIo => output.write_str("CLAP plugin state transfer failed"),
            Self::StateTooLarge => output.write_str("CLAP plugin state exceeds the size limit"),
            Self::Activation => output.write_str("CLAP plugin activation failed"),
            Self::StartProcessing => output.write_str("CLAP plugin did not start processing"),
            Self::NotProcessing => output.write_str("CLAP plugin is not processing"),
            Self::InvalidBlock => output.write_str("CLAP audio block is invalid"),
            Self::Processing => output.write_str("CLAP plugin processing failed"),
        }
    }
}

struct EntryLibrary {
    _library: Library,
    deinitialize: unsafe extern "C" fn(),
}

impl Drop for EntryLibrary {
    fn drop(&mut self) {
        // SAFETY: Each successful entry initialization is paired here.
        unsafe { (self.deinitialize)() };
    }
}

pub struct Instance {
    plugin: *const clap_plugin,
    _entry: EntryLibrary,
    _host: Box<clap_host>,
    requests: Box<AtomicU8>,
    active: bool,
    processing: bool,
    min_frames: u32,
    max_frames: u32,
    input_ports: u32,
    steady_time: i64,
    parameter_events: Box<[clap_event_param_value; MAX_PARAMETER_EVENTS]>,
}

impl Instance {
    pub fn open(package: &Path, identifier: &str) -> Result<Self, Error> {
        let binary = clap_binary(package).map_err(|_| Error::MissingBinary)?;
        let path =
            CString::new(binary.to_string_lossy().as_bytes()).map_err(|_| Error::InvalidPath)?;
        let identifier = CString::new(identifier).map_err(|_| Error::InvalidPath)?;
        let requests = Box::new(AtomicU8::new(0));
        let mut host = Box::new(clap_host {
            clap_version: CLAP_VERSION,
            host_data: ptr::null_mut(),
            name: c"Nylon".as_ptr(),
            vendor: c"Nylon Contributors".as_ptr(),
            url: c"".as_ptr(),
            version: c"0.1.0".as_ptr(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_request_restart),
            request_process: Some(host_request_process),
            request_callback: Some(host_request_callback),
        });
        host.host_data = ptr::from_ref(requests.as_ref()).cast_mut().cast();
        // SAFETY: All plugin-owned pointers stay inside this instance and the
        // dynamic library remains loaded until after plugin destruction.
        unsafe {
            let library = Library::new(&binary).map_err(|error| Error::Load(error.to_string()))?;
            let symbol: Symbol<'_, *const clap_plugin_entry> = library
                .get(b"clap_entry\0")
                .map_err(|_| Error::MissingEntry)?;
            let entry = symbol.as_ref().ok_or(Error::MissingEntry)?;
            if !clap_version_is_compatible(entry.clap_version) {
                return Err(Error::IncompatibleEntry);
            }
            let initialize = entry.init.ok_or(Error::InvalidFactory)?;
            let deinitialize = entry.deinit.ok_or(Error::InvalidFactory)?;
            if !initialize(path.as_ptr()) {
                return Err(Error::EntryInitialization);
            }
            let entry_library = EntryLibrary {
                _library: library,
                deinitialize,
            };
            let get_factory = entry.get_factory.ok_or(Error::InvalidFactory)?;
            let factory =
                get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()).cast::<clap_plugin_factory>();
            let factory = factory.as_ref().ok_or(Error::MissingFactory)?;
            let create = factory.create_plugin.ok_or(Error::InvalidFactory)?;
            let plugin = create(factory, host.as_ref(), identifier.as_ptr());
            let Some(raw) = plugin.as_ref() else {
                return Err(Error::PluginCreation);
            };
            let destroy = raw.destroy.ok_or(Error::InvalidPlugin)?;
            if raw.desc.is_null()
                || raw.init.is_none()
                || raw.activate.is_none()
                || raw.deactivate.is_none()
                || raw.start_processing.is_none()
                || raw.stop_processing.is_none()
                || raw.reset.is_none()
                || raw.process.is_none()
                || raw.get_extension.is_none()
                || raw.on_main_thread.is_none()
            {
                destroy(plugin);
                return Err(Error::InvalidPlugin);
            }
            let descriptor_id = (*raw.desc).id;
            if descriptor_id.is_null()
                || CStr::from_ptr(descriptor_id).to_bytes() != identifier.as_bytes()
            {
                destroy(plugin);
                return Err(Error::InvalidPlugin);
            }
            if !(raw.init.unwrap())(plugin) {
                destroy(plugin);
                return Err(Error::PluginInitialization);
            }
            Ok(Self {
                plugin,
                _entry: entry_library,
                _host: host,
                requests,
                active: false,
                processing: false,
                min_frames: 0,
                max_frames: 0,
                input_ports: 0,
                steady_time: 0,
                parameter_events: Box::new([EMPTY_PARAMETER_EVENT; MAX_PARAMETER_EVENTS]),
            })
        }
    }

    pub fn activate(
        &mut self,
        sample_rate: f64,
        min_frames: u32,
        max_frames: u32,
    ) -> Result<(), Error> {
        if self.active
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
            || min_frames == 0
            || max_frames < min_frames
            || max_frames > i32::MAX as u32
        {
            return Err(Error::InvalidConfiguration);
        }
        // SAFETY: Construction validates every lifecycle callback.
        unsafe {
            let plugin = self.plugin.as_ref().ok_or(Error::InvalidPlugin)?;
            if !(plugin.activate.unwrap())(self.plugin, sample_rate, min_frames, max_frames) {
                return Err(Error::Activation);
            }
            self.active = true;
            let input_ports = match stereo_port_counts(self.plugin) {
                Ok(input_ports) => input_ports,
                Err(error) => {
                    (plugin.deactivate.unwrap())(self.plugin);
                    self.active = false;
                    return Err(error);
                }
            };
            if !(plugin.start_processing.unwrap())(self.plugin) {
                (plugin.deactivate.unwrap())(self.plugin);
                self.active = false;
                return Err(Error::StartProcessing);
            }
            self.processing = true;
            self.min_frames = min_frames;
            self.max_frames = max_frames;
            self.input_ports = input_ports;
            self.steady_time = 0;
        }
        Ok(())
    }

    pub fn process_stereo(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        output_left: &mut [f32],
        output_right: &mut [f32],
    ) -> Result<clap_process_status, Error> {
        self.process_stereo_with_events(input, output_left, output_right, &[])
    }

    pub fn process_stereo_with_events(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        output_left: &mut [f32],
        output_right: &mut [f32],
        parameter_events: &[ParameterEvent],
    ) -> Result<clap_process_status, Error> {
        if !self.processing {
            return Err(Error::NotProcessing);
        }
        let frames = output_left.len();
        if output_right.len() != frames
            || frames < self.min_frames as usize
            || frames > self.max_frames as usize
            || usize::from(input.is_some()) != self.input_ports as usize
            || input.is_some_and(|(left, right)| left.len() != frames || right.len() != frames)
            || parameter_events.len() > MAX_PARAMETER_EVENTS
            || parameter_events
                .iter()
                .any(|event| event.sample_offset >= frames as u32 || !event.value.is_finite())
            || parameter_events
                .windows(2)
                .any(|events| events[0].sample_offset > events[1].sample_offset)
        {
            return Err(Error::InvalidBlock);
        }
        for (destination, source) in self.parameter_events.iter_mut().zip(parameter_events) {
            *destination = clap_event_param_value {
                header: clap_event_header {
                    time: source.sample_offset,
                    ..EMPTY_PARAMETER_EVENT.header
                },
                param_id: source.identifier,
                value: source.value,
                ..EMPTY_PARAMETER_EVENT
            };
        }
        let mut output_channels = [output_left.as_mut_ptr(), output_right.as_mut_ptr()];
        let mut output = clap_audio_buffer {
            data32: output_channels.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let mut input_channels = [ptr::null_mut(); 2];
        let input_buffer = input.map(|(left, right)| {
            input_channels = [left.as_ptr().cast_mut(), right.as_ptr().cast_mut()];
            clap_audio_buffer {
                data32: input_channels.as_mut_ptr(),
                data64: ptr::null_mut(),
                channel_count: 2,
                latency: 0,
                constant_mask: 0,
            }
        });
        let event_context = InputEventContext {
            events: self.parameter_events.as_ptr(),
            count: parameter_events.len() as u32,
        };
        let input_events = clap_input_events {
            ctx: ptr::from_ref(&event_context).cast_mut().cast(),
            size: Some(parameter_event_count),
            get: Some(parameter_event_get),
        };
        let output_events = clap_output_events {
            ctx: ptr::null_mut(),
            try_push: Some(discard_output_event),
        };
        let process = clap_process {
            steady_time: self.steady_time,
            frames_count: frames as u32,
            transport: ptr::null(),
            audio_inputs: input_buffer.as_ref().map_or(ptr::null(), ptr::from_ref),
            audio_outputs: ptr::from_mut(&mut output),
            audio_inputs_count: self.input_ports,
            audio_outputs_count: 1,
            in_events: ptr::from_ref(&input_events),
            out_events: ptr::from_ref(&output_events),
        };
        // SAFETY: Buffer pointers remain valid for this call and construction
        // validates the process callback.
        let status =
            unsafe { (self.plugin.as_ref().unwrap().process.unwrap())(self.plugin, &process) };
        if status == CLAP_PROCESS_ERROR {
            output_left.fill(0.0);
            output_right.fill(0.0);
            return Err(Error::Processing);
        }
        self.steady_time = self.steady_time.saturating_add(frames as i64);
        Ok(status)
    }

    pub fn parameters(&self) -> Result<Vec<ParameterInfo>, Error> {
        let Some(extension) = self.parameter_extension_optional()? else {
            return Ok(Vec::new());
        };
        let count = extension.count.ok_or(Error::UnsupportedParameters)?;
        let get_info = extension.get_info.ok_or(Error::UnsupportedParameters)?;
        // SAFETY: The initialized plugin owns the extension for its lifetime.
        let count = unsafe { count(self.plugin) };
        if count > MAX_PARAMETERS {
            return Err(Error::InvalidParameters);
        }
        let mut parameters = Vec::with_capacity(count as usize);
        for index in 0..count {
            let mut raw = std::mem::MaybeUninit::<clap_param_info>::zeroed();
            // SAFETY: The output has storage for one complete parameter record.
            if !unsafe { get_info(self.plugin, index, raw.as_mut_ptr()) } {
                return Err(Error::InvalidParameters);
            }
            // SAFETY: A successful callback initializes the complete record.
            let raw = unsafe { raw.assume_init() };
            if !raw.min_value.is_finite()
                || !raw.max_value.is_finite()
                || !raw.default_value.is_finite()
                || raw.min_value > raw.max_value
                || !(raw.min_value..=raw.max_value).contains(&raw.default_value)
                || parameters
                    .iter()
                    .any(|parameter: &ParameterInfo| parameter.identifier == raw.id)
            {
                return Err(Error::InvalidParameters);
            }
            parameters.push(ParameterInfo {
                identifier: raw.id,
                flags: raw.flags,
                name: fixed_text(&raw.name)?,
                module: fixed_text(&raw.module)?,
                minimum: raw.min_value,
                maximum: raw.max_value,
                default_value: raw.default_value,
            });
        }
        Ok(parameters)
    }

    pub fn parameter_value(&self, identifier: u32) -> Result<f64, Error> {
        let extension = self.parameter_extension()?;
        let get_value = extension.get_value.ok_or(Error::UnsupportedParameters)?;
        let mut value = 0.0;
        // SAFETY: The initialized plugin owns the extension for its lifetime.
        if !unsafe { get_value(self.plugin, identifier, &mut value) } || !value.is_finite() {
            return Err(Error::InvalidParameters);
        }
        Ok(value)
    }

    pub fn latency_frames(&self) -> Result<u32, Error> {
        if !self.active {
            return Err(Error::NotProcessing);
        }
        // SAFETY: The plugin is active and owns the extension pointer.
        unsafe {
            let plugin = self.plugin.as_ref().ok_or(Error::InvalidPlugin)?;
            let extension = (plugin.get_extension.unwrap())(self.plugin, CLAP_EXT_LATENCY.as_ptr())
                .cast::<clap_plugin_latency>();
            let Some(extension) = extension.as_ref() else {
                return Ok(0);
            };
            Ok(extension.get.map_or(0, |get| get(self.plugin)))
        }
    }

    pub fn save_state(&self) -> Result<Vec<u8>, Error> {
        let extension = self.state_extension()?;
        let save = extension.save.ok_or(Error::UnsupportedState)?;
        let mut context = StateWriteContext {
            bytes: Vec::new(),
            failed: false,
        };
        let stream = clap_ostream {
            ctx: ptr::from_mut(&mut context).cast(),
            write: Some(state_write),
        };
        // SAFETY: The stream and context remain live for the complete callback.
        if !unsafe { save(self.plugin, &stream) } {
            return Err(if context.failed {
                Error::StateTooLarge
            } else {
                Error::StateIo
            });
        }
        if context.failed {
            return Err(Error::StateTooLarge);
        }
        Ok(context.bytes)
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > MAX_STATE_BYTES {
            return Err(Error::StateTooLarge);
        }
        let extension = self.state_extension()?;
        let load = extension.load.ok_or(Error::UnsupportedState)?;
        let mut context = StateReadContext {
            bytes: bytes.as_ptr(),
            length: bytes.len(),
            position: 0,
        };
        let stream = clap_istream {
            ctx: ptr::from_mut(&mut context).cast(),
            read: Some(state_read),
        };
        // SAFETY: The stream and context remain live for the complete callback.
        if !unsafe { load(self.plugin, &stream) } {
            return Err(Error::StateIo);
        }
        Ok(())
    }

    fn parameter_extension(&self) -> Result<&clap_plugin_params, Error> {
        self.parameter_extension_optional()?
            .ok_or(Error::UnsupportedParameters)
    }

    fn parameter_extension_optional(&self) -> Result<Option<&clap_plugin_params>, Error> {
        // SAFETY: The plugin is initialized and owns the returned extension pointer.
        unsafe {
            let plugin = self.plugin.as_ref().ok_or(Error::InvalidPlugin)?;
            Ok(
                (plugin.get_extension.unwrap())(self.plugin, CLAP_EXT_PARAMS.as_ptr())
                    .cast::<clap_plugin_params>()
                    .as_ref(),
            )
        }
    }

    fn state_extension(&self) -> Result<&clap_plugin_state, Error> {
        // SAFETY: The plugin is initialized and owns the returned extension pointer.
        unsafe {
            let plugin = self.plugin.as_ref().ok_or(Error::InvalidPlugin)?;
            (plugin.get_extension.unwrap())(self.plugin, CLAP_EXT_STATE.as_ptr())
                .cast::<clap_plugin_state>()
                .as_ref()
                .ok_or(Error::UnsupportedState)
        }
    }

    pub fn reset(&mut self) -> Result<(), Error> {
        if !self.active {
            return Err(Error::NotProcessing);
        }
        // SAFETY: Construction validates reset and active state permits it.
        unsafe { (self.plugin.as_ref().unwrap().reset.unwrap())(self.plugin) };
        self.steady_time = 0;
        Ok(())
    }

    pub fn take_requests(&self) -> HostRequests {
        let requests = self.requests.swap(0, Ordering::AcqRel);
        HostRequests {
            restart: requests & RESTART_REQUESTED != 0,
            process: requests & PROCESS_REQUESTED != 0,
            callback: requests & CALLBACK_REQUESTED != 0,
            parameter_rescan: requests & PARAMETER_RESCAN_REQUESTED != 0,
            parameter_clear: requests & PARAMETER_CLEAR_REQUESTED != 0,
            parameter_flush: requests & PARAMETER_FLUSH_REQUESTED != 0,
            latency_changed: requests & LATENCY_CHANGED != 0,
            state_dirty: requests & STATE_DIRTY != 0,
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: Construction validates lifecycle callbacks. State flags
        // preserve the required stop, deactivate, destroy order.
        unsafe {
            if let Some(plugin) = self.plugin.as_ref() {
                if self.processing {
                    (plugin.stop_processing.unwrap())(self.plugin);
                }
                if self.active {
                    (plugin.deactivate.unwrap())(self.plugin);
                }
                (plugin.destroy.unwrap())(self.plugin);
            }
        }
    }
}

unsafe fn stereo_port_counts(plugin: *const clap_plugin) -> Result<u32, Error> {
    // SAFETY: The instance is initialized and the callback is validated at construction.
    let raw = unsafe { plugin.as_ref() }.ok_or(Error::InvalidPlugin)?;
    // SAFETY: The instance is initialized and owns the returned extension pointer.
    let extension = unsafe { (raw.get_extension.unwrap())(plugin, CLAP_EXT_AUDIO_PORTS.as_ptr()) }
        .cast::<clap_plugin_audio_ports>();
    // SAFETY: A non-null extension pointer follows the CLAP audio-ports layout.
    let ports = unsafe { extension.as_ref() }.ok_or(Error::UnsupportedPorts)?;
    let count = ports.count.ok_or(Error::UnsupportedPorts)?;
    let get = ports.get.ok_or(Error::UnsupportedPorts)?;
    // SAFETY: The audio-ports extension belongs to this initialized plugin.
    let inputs = unsafe { count(plugin, true) };
    // SAFETY: The audio-ports extension belongs to this initialized plugin.
    let outputs = unsafe { count(plugin, false) };
    if inputs > 1 || outputs != 1 {
        return Err(Error::UnsupportedPorts);
    }
    for (is_input, port_count) in [(true, inputs), (false, outputs)] {
        if port_count == 0 {
            continue;
        }
        let mut info = std::mem::MaybeUninit::<clap_audio_port_info>::zeroed();
        // SAFETY: The output points to enough writable storage for one port record.
        if !unsafe { get(plugin, 0, is_input, info.as_mut_ptr()) } {
            return Err(Error::UnsupportedPorts);
        }
        // SAFETY: A successful get initializes the complete C structure.
        if unsafe { info.assume_init() }.channel_count != 2 {
            return Err(Error::UnsupportedPorts);
        }
    }
    Ok(inputs)
}

unsafe extern "C" fn host_get_extension(
    _host: *const clap_host,
    extension_id: *const c_char,
) -> *const c_void {
    if extension_id.is_null() {
        return ptr::null();
    }
    // SAFETY: CLAP extension identifiers are terminated strings.
    let identifier = unsafe { CStr::from_ptr(extension_id) };
    if identifier == CLAP_EXT_PARAMS {
        return ptr::from_ref(&HOST_PARAMETERS).cast();
    }
    if identifier == CLAP_EXT_LATENCY {
        return ptr::from_ref(&HOST_LATENCY).cast();
    }
    if identifier == CLAP_EXT_STATE {
        return ptr::from_ref(&HOST_STATE).cast();
    }
    ptr::null()
}

static HOST_PARAMETERS: clap_host_params = clap_host_params {
    rescan: Some(host_parameter_rescan),
    clear: Some(host_parameter_clear),
    request_flush: Some(host_parameter_flush),
};

static HOST_LATENCY: clap_host_latency = clap_host_latency {
    changed: Some(host_latency_changed),
};

static HOST_STATE: clap_host_state = clap_host_state {
    mark_dirty: Some(host_state_dirty),
};

unsafe extern "C" fn host_parameter_rescan(
    host: *const clap_host,
    _flags: clap_param_rescan_flags,
) {
    // SAFETY: CLAP passes back the host pointer supplied at creation.
    unsafe { request(host, PARAMETER_RESCAN_REQUESTED) };
}

unsafe extern "C" fn host_parameter_clear(
    host: *const clap_host,
    _identifier: u32,
    _flags: clap_param_clear_flags,
) {
    // SAFETY: CLAP passes back the host pointer supplied at creation.
    unsafe { request(host, PARAMETER_CLEAR_REQUESTED) };
}

unsafe extern "C" fn host_parameter_flush(host: *const clap_host) {
    // SAFETY: CLAP passes back the host pointer supplied at creation.
    unsafe { request(host, PARAMETER_FLUSH_REQUESTED) };
}

unsafe extern "C" fn host_latency_changed(host: *const clap_host) {
    // SAFETY: CLAP passes back the host pointer supplied at creation.
    unsafe { request(host, LATENCY_CHANGED) };
}

unsafe extern "C" fn host_state_dirty(host: *const clap_host) {
    // SAFETY: CLAP passes back the host pointer supplied at creation.
    unsafe { request(host, STATE_DIRTY) };
}

unsafe fn request(host: *const clap_host, bit: u8) {
    // SAFETY: The host and request bits live until after plugin destruction.
    let Some(host) = (unsafe { host.as_ref() }) else {
        return;
    };
    // SAFETY: host_data points to the boxed atomic owned by Instance.
    if let Some(requests) = unsafe { host.host_data.cast::<AtomicU8>().as_ref() } {
        requests.fetch_or(bit, Ordering::Release);
    }
}

unsafe extern "C" fn host_request_restart(host: *const clap_host) {
    // SAFETY: CLAP passes back the host pointer supplied at creation.
    unsafe { request(host, RESTART_REQUESTED) };
}

unsafe extern "C" fn host_request_process(host: *const clap_host) {
    // SAFETY: CLAP passes back the host pointer supplied at creation.
    unsafe { request(host, PROCESS_REQUESTED) };
}

unsafe extern "C" fn host_request_callback(host: *const clap_host) {
    // SAFETY: CLAP passes back the host pointer supplied at creation.
    unsafe { request(host, CALLBACK_REQUESTED) };
}

struct InputEventContext {
    events: *const clap_event_param_value,
    count: u32,
}

struct StateWriteContext {
    bytes: Vec<u8>,
    failed: bool,
}

unsafe extern "C" fn state_write(
    stream: *const clap_ostream,
    buffer: *const c_void,
    size: u64,
) -> i64 {
    // SAFETY: The state callback receives the stream created by save_state().
    let Some(context) = (unsafe {
        stream
            .as_ref()
            .and_then(|stream| stream.ctx.cast::<StateWriteContext>().as_mut())
    }) else {
        return -1;
    };
    let Ok(size) = usize::try_from(size) else {
        context.failed = true;
        return -1;
    };
    if (buffer.is_null() && size != 0) || context.bytes.len().saturating_add(size) > MAX_STATE_BYTES
    {
        context.failed = true;
        return -1;
    }
    if size != 0 {
        // SAFETY: The plugin supplies a readable region containing size bytes.
        let bytes = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), size) };
        context.bytes.extend_from_slice(bytes);
    }
    size as i64
}

struct StateReadContext {
    bytes: *const u8,
    length: usize,
    position: usize,
}

unsafe extern "C" fn state_read(
    stream: *const clap_istream,
    buffer: *mut c_void,
    size: u64,
) -> i64 {
    // SAFETY: The state callback receives the stream created by load_state().
    let Some(context) = (unsafe {
        stream
            .as_ref()
            .and_then(|stream| stream.ctx.cast::<StateReadContext>().as_mut())
    }) else {
        return -1;
    };
    let Ok(requested) = usize::try_from(size) else {
        return -1;
    };
    if buffer.is_null() && requested != 0 {
        return -1;
    }
    let count = requested.min(context.length.saturating_sub(context.position));
    if count != 0 {
        // SAFETY: The plugin supplies writable storage for the requested byte count.
        unsafe {
            ptr::copy_nonoverlapping(
                context.bytes.add(context.position),
                buffer.cast::<u8>(),
                count,
            )
        };
        context.position += count;
    }
    count as i64
}

unsafe extern "C" fn parameter_event_count(list: *const clap_input_events) -> u32 {
    // SAFETY: The list context points to the stack record held through process().
    unsafe {
        list.as_ref()
            .and_then(|list| list.ctx.cast::<InputEventContext>().as_ref())
            .map_or(0, |context| context.count)
    }
}

unsafe extern "C" fn parameter_event_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    // SAFETY: The bounded index selects initialized storage held through process().
    let Some(context) = (unsafe {
        list.as_ref()
            .and_then(|list| list.ctx.cast::<InputEventContext>().as_ref())
    }) else {
        return ptr::null();
    };
    if index >= context.count {
        return ptr::null();
    }
    // SAFETY: count never exceeds the preallocated event array length.
    unsafe { ptr::from_ref(&(*context.events.add(index as usize)).header) }
}

unsafe extern "C" fn discard_output_event(
    _list: *const clap_output_events,
    _event: *const clap_event_header,
) -> bool {
    false
}

fn fixed_text(text: &[c_char]) -> Result<String, Error> {
    let Some(end) = text.iter().position(|byte| *byte == 0) else {
        return Err(Error::InvalidParameters);
    };
    let bytes = text[..end]
        .iter()
        .map(|byte| *byte as u8)
        .collect::<Vec<_>>();
    String::from_utf8(bytes).map_err(|_| Error::InvalidParameters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_plugins_fail_before_loading() {
        let result = Instance::open(Path::new("target/absent.clap"), "app.nylon.absent");
        assert!(matches!(result, Err(Error::MissingBinary)));
    }

    #[test]
    fn request_flags_start_clear() {
        let requests = HostRequests::default();
        assert!(
            !requests.restart
                && !requests.process
                && !requests.callback
                && !requests.parameter_rescan
                && !requests.parameter_clear
                && !requests.parameter_flush
                && !requests.latency_changed
                && !requests.state_dirty
        );
    }

    #[test]
    fn fixed_parameter_text_requires_utf8_and_a_terminator() {
        assert_eq!(fixed_text(&[b'G' as c_char, 0]).unwrap(), "G");
        assert_eq!(fixed_text(&[b'G' as c_char]), Err(Error::InvalidParameters));
        assert_eq!(
            fixed_text(&[-1_i8 as c_char, 0]),
            Err(Error::InvalidParameters)
        );
    }

    #[test]
    fn state_streams_transfer_exact_bytes_and_stop_at_eof() {
        let mut write_context = StateWriteContext {
            bytes: Vec::new(),
            failed: false,
        };
        let output = clap_ostream {
            ctx: ptr::from_mut(&mut write_context).cast(),
            write: Some(state_write),
        };
        let source = [3_u8, 1, 4, 1];
        // SAFETY: Both pointers cover the complete call and source byte range.
        let written = unsafe { state_write(&output, source.as_ptr().cast(), source.len() as u64) };
        assert_eq!(written, 4);
        assert_eq!(write_context.bytes, source);

        let mut read_context = StateReadContext {
            bytes: source.as_ptr(),
            length: source.len(),
            position: 0,
        };
        let input = clap_istream {
            ctx: ptr::from_mut(&mut read_context).cast(),
            read: Some(state_read),
        };
        let mut destination = [0_u8; 6];
        // SAFETY: Both pointers cover the complete call and destination range.
        let read = unsafe { state_read(&input, destination.as_mut_ptr().cast(), 6) };
        assert_eq!(read, 4);
        // SAFETY: The same stream remains live and has reached EOF.
        let eof = unsafe { state_read(&input, destination.as_mut_ptr().cast(), 1) };
        assert_eq!(eof, 0);
        assert_eq!(&destination[..4], &source);
    }
}
