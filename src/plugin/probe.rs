//! Isolated CLAP descriptor inspection and its bounded output protocol.

use clap_sys::entry::clap_plugin_entry;
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::plugin::clap_plugin_descriptor;
use clap_sys::version::clap_version_is_compatible;
use libloading::{Library, Symbol};
use std::collections::HashSet;
use std::ffi::{CStr, CString, c_char};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const MAX_DESCRIPTORS: usize = 4_096;
pub const MAX_FEATURES: usize = 256;
pub const MAX_TEXT_BYTES: usize = 4_096;
pub const PROTOCOL_HEADER: [u8; 8] = *b"NYCLAP1\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub features: Vec<String>,
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
    TooManyDescriptors,
    InvalidDescriptor,
    DuplicateIdentifier,
    InvalidText,
    TooManyFeatures,
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBinary => output.write_str("Plugin binary is missing"),
            Self::InvalidPath => output.write_str("Plugin path cannot be passed to CLAP"),
            Self::Load(message) => write!(output, "Dynamic library load failed: {message}"),
            Self::MissingEntry => output.write_str("CLAP entry symbol is missing"),
            Self::IncompatibleEntry => output.write_str("CLAP entry version is incompatible"),
            Self::EntryInitialization => output.write_str("CLAP entry initialization failed"),
            Self::MissingFactory => output.write_str("CLAP plugin factory is missing"),
            Self::InvalidFactory => output.write_str("CLAP plugin factory is incomplete"),
            Self::TooManyDescriptors => output.write_str("CLAP descriptor limit exceeded"),
            Self::InvalidDescriptor => output.write_str("CLAP descriptor is incomplete"),
            Self::DuplicateIdentifier => output.write_str("CLAP identifier is duplicated"),
            Self::InvalidText => output.write_str("CLAP metadata is invalid UTF-8 or too long"),
            Self::TooManyFeatures => output.write_str("CLAP feature limit exceeded"),
        }
    }
}

/// Loads one CLAP binary and copies factory descriptors. Call this only from
/// the short-lived probe executable because third-party initialization may exit.
pub fn inspect_clap(package: &Path) -> Result<Vec<Descriptor>, Error> {
    let binary = clap_binary(package)?;
    let path_text = binary.to_string_lossy();
    let path_text = CString::new(path_text.as_bytes()).map_err(|_| Error::InvalidPath)?;
    // SAFETY: Loading and calling a plugin ABI is confined to the probe process.
    unsafe {
        let library = Library::new(&binary).map_err(|error| Error::Load(error.to_string()))?;
        let entry: Symbol<'_, *const clap_plugin_entry> = library
            .get(b"clap_entry\0")
            .map_err(|_| Error::MissingEntry)?;
        let entry = entry.as_ref().ok_or(Error::MissingEntry)?;
        if !clap_version_is_compatible(entry.clap_version) {
            return Err(Error::IncompatibleEntry);
        }
        let initialize = entry.init.ok_or(Error::InvalidFactory)?;
        let deinitialize = entry.deinit.ok_or(Error::InvalidFactory)?;
        if !initialize(path_text.as_ptr()) {
            return Err(Error::EntryInitialization);
        }
        let guard = EntryGuard(deinitialize);
        let get_factory = entry.get_factory.ok_or(Error::InvalidFactory)?;
        let factory = get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()).cast::<clap_plugin_factory>();
        let factory = factory.as_ref().ok_or(Error::MissingFactory)?;
        let get_count = factory.get_plugin_count.ok_or(Error::InvalidFactory)?;
        let get_descriptor = factory.get_plugin_descriptor.ok_or(Error::InvalidFactory)?;
        let count = usize::try_from(get_count(factory)).map_err(|_| Error::TooManyDescriptors)?;
        if count > MAX_DESCRIPTORS {
            return Err(Error::TooManyDescriptors);
        }
        let mut identifiers = HashSet::with_capacity(count);
        let mut descriptors = Vec::with_capacity(count);
        for index in 0..count {
            let raw = get_descriptor(factory, index as u32)
                .as_ref()
                .ok_or(Error::InvalidDescriptor)?;
            if !clap_version_is_compatible(raw.clap_version) {
                return Err(Error::IncompatibleEntry);
            }
            let descriptor = copy_descriptor(raw)?;
            if !identifiers.insert(descriptor.id.clone()) {
                return Err(Error::DuplicateIdentifier);
            }
            descriptors.push(descriptor);
        }
        drop(guard);
        Ok(descriptors)
    }
}

struct EntryGuard(unsafe extern "C" fn());

impl Drop for EntryGuard {
    fn drop(&mut self) {
        // SAFETY: A successful entry initialization must be paired with deinitialization.
        unsafe { (self.0)() };
    }
}

fn clap_binary(package: &Path) -> Result<PathBuf, Error> {
    if package.is_file() {
        return Ok(package.to_owned());
    }
    if !package.is_dir() {
        return Err(Error::MissingBinary);
    }
    let directory = package.join("Contents").join("MacOS");
    let preferred = package
        .file_stem()
        .map(|name| directory.join(name))
        .filter(|path| path.is_file());
    if let Some(preferred) = preferred {
        return Ok(preferred);
    }
    let mut binaries = fs::read_dir(directory)
        .map_err(|_| Error::MissingBinary)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    binaries.sort();
    binaries.into_iter().next().ok_or(Error::MissingBinary)
}

unsafe fn copy_descriptor(raw: &clap_plugin_descriptor) -> Result<Descriptor, Error> {
    // SAFETY: The factory owns descriptor strings until entry deinitialization.
    let id = unsafe { required_text(raw.id) }?;
    // SAFETY: The factory owns descriptor strings until entry deinitialization.
    let name = unsafe { required_text(raw.name) }?;
    // SAFETY: Optional descriptor strings follow the same lifetime contract.
    let vendor = unsafe { optional_text(raw.vendor) }?;
    // SAFETY: Optional descriptor strings follow the same lifetime contract.
    let version = unsafe { optional_text(raw.version) }?;
    let mut features = Vec::new();
    if !raw.features.is_null() {
        for index in 0..=MAX_FEATURES {
            // SAFETY: CLAP defines features as a terminated pointer array.
            let feature = unsafe { *raw.features.add(index) };
            if feature.is_null() {
                return Ok(Descriptor {
                    id,
                    name,
                    vendor,
                    version,
                    features,
                });
            }
            if index == MAX_FEATURES {
                return Err(Error::TooManyFeatures);
            }
            // SAFETY: Each non-null feature is a terminated CLAP string.
            features.push(unsafe { required_text(feature) }?);
        }
    }
    Ok(Descriptor {
        id,
        name,
        vendor,
        version,
        features,
    })
}

unsafe fn required_text(value: *const c_char) -> Result<String, Error> {
    if value.is_null() {
        return Err(Error::InvalidDescriptor);
    }
    // SAFETY: The CLAP ABI requires a terminated string here.
    let value = unsafe { CStr::from_ptr(value) };
    let value = value.to_str().map_err(|_| Error::InvalidText)?;
    if value.is_empty() || value.len() > MAX_TEXT_BYTES {
        return Err(Error::InvalidText);
    }
    Ok(value.to_owned())
}

unsafe fn optional_text(value: *const c_char) -> Result<String, Error> {
    if value.is_null() {
        return Ok(String::new());
    }
    // SAFETY: The CLAP ABI requires a terminated string when non-null.
    let value = unsafe { CStr::from_ptr(value) };
    let value = value.to_str().map_err(|_| Error::InvalidText)?;
    if value.len() > MAX_TEXT_BYTES {
        return Err(Error::InvalidText);
    }
    Ok(value.to_owned())
}

pub fn write_protocol(descriptors: &[Descriptor], mut output: impl Write) -> Result<(), io::Error> {
    output.write_all(&PROTOCOL_HEADER)?;
    write_count(descriptors.len(), &mut output)?;
    for descriptor in descriptors {
        write_text(&descriptor.id, &mut output)?;
        write_text(&descriptor.name, &mut output)?;
        write_text(&descriptor.vendor, &mut output)?;
        write_text(&descriptor.version, &mut output)?;
        write_count(descriptor.features.len(), &mut output)?;
        for feature in &descriptor.features {
            write_text(feature, &mut output)?;
        }
    }
    Ok(())
}

fn write_count(value: usize, output: &mut impl Write) -> Result<(), io::Error> {
    let value = u32::try_from(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Count exceeds protocol"))?;
    output.write_all(&value.to_le_bytes())
}

fn write_text(value: &str, output: &mut impl Write) -> Result<(), io::Error> {
    if value.len() > MAX_TEXT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Text exceeds protocol",
        ));
    }
    write_count(value.len(), output)?;
    output.write_all(value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_has_stable_lengths_and_order() {
        let descriptors = [Descriptor {
            id: "app.nylon.fixture".into(),
            name: "Fixture".into(),
            vendor: "Nylon Contributors".into(),
            version: "1.0".into(),
            features: vec!["audio-effect".into(), "stereo".into()],
        }];
        let mut bytes = Vec::new();
        write_protocol(&descriptors, &mut bytes).unwrap();
        assert_eq!(&bytes[..8], &PROTOCOL_HEADER);
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1);
        let id_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(&bytes[16..16 + id_length], b"app.nylon.fixture");
    }

    #[test]
    fn missing_and_empty_packages_are_rejected() {
        assert_eq!(
            inspect_clap(Path::new("target/missing-plugin.clap")),
            Err(Error::MissingBinary)
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let package = PathBuf::from("target").join(format!("empty-{stamp}.clap"));
        fs::create_dir_all(&package).unwrap();
        assert_eq!(inspect_clap(&package), Err(Error::MissingBinary));
        fs::remove_dir_all(package).unwrap();
    }

    #[test]
    fn oversized_protocol_text_is_rejected() {
        let descriptor = Descriptor {
            id: "x".repeat(MAX_TEXT_BYTES + 1),
            name: "Fixture".into(),
            vendor: String::new(),
            version: String::new(),
            features: Vec::new(),
        };
        assert_eq!(
            write_protocol(&[descriptor], Vec::new())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
