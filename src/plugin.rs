//! Plugin discovery and quarantine state.

pub mod clap;
pub mod probe;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum entries accepted from one discovery pass.
pub const MAX_PLUGINS: usize = 65_536;
/// Maximum directory depth visited below each root.
pub const MAX_SCAN_DEPTH: usize = 24;
/// Maximum diagnostics retained from one discovery pass.
pub const MAX_SCAN_ISSUES: usize = 1_024;
/// Maximum quarantine reason length in bytes.
pub const MAX_REASON_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Format {
    Vst3,
    AudioUnit,
    Clap,
    Lv2,
}

impl Format {
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::Vst3 => 0,
            Self::AudioUnit => 1,
            Self::Clap => 2,
            Self::Lv2 => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Discovered,
    Quarantined,
}

impl State {
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::Discovered => 0,
            Self::Quarantined => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    path: PathBuf,
    name: String,
    format: Format,
    state: State,
    reason: String,
    descriptors: Vec<probe::Descriptor>,
}

impl Entry {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    #[must_use]
    pub fn quarantine_reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub fn descriptors(&self) -> &[probe::Descriptor] {
        &self.descriptors
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanIssue {
    path: PathBuf,
    message: String,
}

impl ScanIssue {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogError {
    MissingEntry,
    InvalidReason,
    InvalidProbe,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    entries: Vec<Entry>,
    issues: Vec<ScanIssue>,
    capacity_reached: bool,
}

impl Catalog {
    /// Discovers plugin bundles without loading executable code.
    #[must_use]
    pub fn scan(roots: &[PathBuf]) -> Self {
        let mut catalog = Self::default();
        let mut seen = HashSet::new();
        for root in roots {
            catalog.visit(root, 0, &mut seen);
        }
        catalog.entries.sort_by(|left, right| {
            left.path
                .to_string_lossy()
                .cmp(&right.path.to_string_lossy())
                .then(left.format.cmp(&right.format))
        });
        catalog
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    #[must_use]
    pub fn issues(&self) -> &[ScanIssue] {
        &self.issues
    }

    pub fn quarantine(&mut self, index: usize, reason: &str) -> Result<(), CatalogError> {
        if reason.is_empty() || reason.len() > MAX_REASON_BYTES {
            return Err(CatalogError::InvalidReason);
        }
        let entry = self
            .entries
            .get_mut(index)
            .ok_or(CatalogError::MissingEntry)?;
        entry.state = State::Quarantined;
        entry.reason.clear();
        entry.reason.push_str(reason);
        entry.descriptors.clear();
        Ok(())
    }

    pub fn retry(&mut self, index: usize) -> Result<(), CatalogError> {
        let entry = self
            .entries
            .get_mut(index)
            .ok_or(CatalogError::MissingEntry)?;
        entry.state = State::Discovered;
        entry.reason.clear();
        entry.descriptors.clear();
        Ok(())
    }

    pub fn apply_probe(&mut self, index: usize, bytes: &[u8]) -> Result<(), CatalogError> {
        let descriptors = probe::read_protocol(bytes).map_err(|_| CatalogError::InvalidProbe)?;
        let entry = self
            .entries
            .get_mut(index)
            .ok_or(CatalogError::MissingEntry)?;
        if entry.format != Format::Clap {
            return Err(CatalogError::InvalidProbe);
        }
        entry.descriptors = descriptors;
        entry.state = State::Discovered;
        entry.reason.clear();
        Ok(())
    }

    fn visit(&mut self, path: &Path, depth: usize, seen: &mut HashSet<PathBuf>) {
        if self.capacity_reached {
            return;
        }
        if self.entries.len() >= MAX_PLUGINS {
            self.issue(path, "Plugin capacity reached");
            self.capacity_reached = true;
            return;
        }
        if depth > MAX_SCAN_DEPTH {
            self.issue(path, "Scan depth reached");
            return;
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.issue(path, &error.to_string());
                return;
            }
        };
        if metadata.file_type().is_symlink() {
            return;
        }
        if let Some(format) = plugin_format(path) {
            let normalized = normalize(path);
            if seen.insert(normalized.clone()) {
                self.entries.push(Entry {
                    name: plugin_name(path),
                    path: normalized,
                    format,
                    state: State::Discovered,
                    reason: String::new(),
                    descriptors: Vec::new(),
                });
            }
            return;
        }
        if !metadata.is_dir() {
            return;
        }
        let directory = match fs::read_dir(path) {
            Ok(directory) => directory,
            Err(error) => {
                self.issue(path, &error.to_string());
                return;
            }
        };
        let mut children = Vec::new();
        for child in directory {
            match child {
                Ok(child) => children.push(child.path()),
                Err(error) => self.issue(path, &error.to_string()),
            }
        }
        children.sort();
        for child in children {
            self.visit(&child, depth + 1, seen);
        }
    }

    fn issue(&mut self, path: &Path, message: &str) {
        if self.issues.len() >= MAX_SCAN_ISSUES {
            return;
        }
        self.issues.push(ScanIssue {
            path: normalize(path),
            message: message.to_owned(),
        });
    }
}

fn plugin_format(path: &Path) -> Option<Format> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "vst3" => Some(Format::Vst3),
        "component" => Some(Format::AudioUnit),
        "clap" => Some(Format::Clap),
        "lv2" => Some(Format::Lv2),
        _ => None,
    }
}

fn plugin_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Unnamed plugin")
        .to_owned()
}

fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        result.push(component);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(label: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = PathBuf::from("target").join(format!("plugin-{label}-{stamp}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn discovery_is_recursive_typed_sorted_and_deduplicated() {
        let root = directory("scan");
        fs::create_dir_all(root.join("Nested/Beta.vst3")).unwrap();
        fs::create_dir_all(root.join("Alpha.component")).unwrap();
        fs::create_dir_all(root.join("Delta.lv2")).unwrap();
        fs::write(root.join("Gamma.clap"), b"binary").unwrap();
        fs::write(root.join("readme.txt"), b"text").unwrap();
        let catalog = Catalog::scan(&[root.clone(), root.clone()]);
        let entries = catalog.entries();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].name(), "Alpha");
        assert_eq!(entries[0].format(), Format::AudioUnit);
        assert_eq!(entries[1].name(), "Delta");
        assert_eq!(entries[1].format(), Format::Lv2);
        assert_eq!(entries[2].name(), "Gamma");
        assert_eq!(entries[2].format(), Format::Clap);
        assert_eq!(entries[3].name(), "Beta");
        assert_eq!(entries[3].format(), Format::Vst3);
        assert!(catalog.issues().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundles_are_not_traversed_and_symlinks_are_skipped() {
        let root = directory("bundles");
        let bundle = root.join("Outer.vst3");
        fs::create_dir_all(bundle.join("Contents/Inner.clap")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&bundle, root.join("Alias.vst3")).unwrap();
        let catalog = Catalog::scan(std::slice::from_ref(&root));
        assert_eq!(catalog.entries().len(), 1);
        assert_eq!(catalog.entries()[0].name(), "Outer");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_roots_are_reported_without_losing_valid_entries() {
        let root = directory("issues");
        fs::write(root.join("Valid.clap"), b"binary").unwrap();
        let missing = root.join("missing");
        let catalog = Catalog::scan(&[missing.clone(), root.clone()]);
        assert_eq!(catalog.entries().len(), 1);
        assert_eq!(catalog.issues().len(), 1);
        assert_eq!(catalog.issues()[0].path(), missing);
        assert!(!catalog.issues()[0].message().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quarantine_and_retry_validate_their_inputs() {
        let root = directory("quarantine");
        fs::write(root.join("Fault.clap"), b"binary").unwrap();
        let mut catalog = Catalog::scan(std::slice::from_ref(&root));
        assert_eq!(catalog.quarantine(0, "Probe process exited"), Ok(()));
        assert_eq!(catalog.entries()[0].state(), State::Quarantined);
        assert_eq!(
            catalog.entries()[0].quarantine_reason(),
            "Probe process exited"
        );
        assert_eq!(catalog.quarantine(0, ""), Err(CatalogError::InvalidReason));
        assert_eq!(catalog.retry(0), Ok(()));
        assert_eq!(catalog.entries()[0].state(), State::Discovered);
        assert_eq!(catalog.retry(1), Err(CatalogError::MissingEntry));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn probe_metadata_is_applied_atomically_only_to_clap_entries() {
        let root = directory("probe");
        fs::write(root.join("Effect.clap"), b"binary").unwrap();
        fs::create_dir_all(root.join("Other.vst3")).unwrap();
        let mut catalog = Catalog::scan(std::slice::from_ref(&root));
        let clap = catalog
            .entries()
            .iter()
            .position(|entry| entry.format() == Format::Clap)
            .unwrap();
        let other = 1 - clap;
        let descriptors = [probe::Descriptor {
            id: "app.nylon.fixture".into(),
            name: "Fixture".into(),
            vendor: "Nylon Contributors".into(),
            version: "1.0".into(),
            features: vec!["audio-effect".into()],
        }];
        let mut bytes = Vec::new();
        probe::write_protocol(&descriptors, &mut bytes).unwrap();
        assert_eq!(catalog.apply_probe(clap, &bytes), Ok(()));
        assert_eq!(catalog.entries()[clap].descriptors(), descriptors);
        let before = catalog.clone();
        assert_eq!(
            catalog.apply_probe(other, &bytes),
            Err(CatalogError::InvalidProbe)
        );
        assert_eq!(catalog, before);
        assert_eq!(
            catalog.apply_probe(clap, b"invalid"),
            Err(CatalogError::InvalidProbe)
        );
        assert_eq!(catalog, before);
        fs::remove_dir_all(root).unwrap();
    }
}
