//! Native control interface. Handles belong to one control thread and must
//! not be accessed concurrently or used after release.

use crate::project::{Command, Project, TrackId, TrackKind};
use std::ffi::{CStr, c_char};

#[unsafe(no_mangle)]
pub extern "C" fn nylon_project_new() -> *mut Project {
    Box::into_raw(Box::new(Project::new()))
}

/// # Safety
/// A non-null handle must originate from `nylon_project_new`, remain live,
/// and have no outstanding references. Release it exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_free(handle: *mut Project) {
    if !handle.is_null() {
        // SAFETY: Ownership of the original allocation is transferred back.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// # Safety
/// A non-null handle must refer to a live project with no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_tempo(handle: *const Project) -> f64 {
    // SAFETY: Validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }.map_or(0.0, |project| project.snapshot().tempo())
}

/// # Safety
/// A non-null handle must refer to a live project exclusively owned by this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_set_tempo(handle: *mut Project, tempo: f64) -> i32 {
    // SAFETY: Validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |project| {
        i32::from(project.apply(&[Command::SetTempo(tempo)]).is_ok())
    })
}

/// # Safety
/// A non-null handle must refer to a live project exclusively owned by this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_add_track(handle: *mut Project) -> i32 {
    // SAFETY: Validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |project| {
        let name = format!("Audio {}", project.snapshot().tracks().len() + 1);
        i32::from(
            project
                .apply(&[Command::CreateTrack {
                    name,
                    kind: TrackKind::Audio,
                }])
                .is_ok(),
        )
    })
}

/// # Safety
/// A non-null handle must refer to a live project with no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_track_count(handle: *const Project) -> u64 {
    // SAFETY: Validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }.map_or(0, |project| project.snapshot().tracks().len() as u64)
}

/// # Safety
/// A non-null handle must refer to a live project exclusively owned by this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_undo(handle: *mut Project) -> i32 {
    // SAFETY: Validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |project| i32::from(project.undo()))
}

/// # Safety
/// A non-null handle must refer to a live project exclusively owned by this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_redo(handle: *mut Project) -> i32 {
    // SAFETY: Validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |project| i32::from(project.redo()))
}

macro_rules! track_getter {
    ($name:ident, $result:ty, $fallback:expr, $method:ident) => {
        /// # Safety
        /// A non-null handle must be live and have no concurrent mutation.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: *const Project, index: u64) -> $result {
            // SAFETY: Handle validity is required by the native interface.
            let Some(project) = (unsafe { handle.as_ref() }) else {
                return $fallback;
            };
            let Ok(index) = usize::try_from(index) else {
                return $fallback;
            };
            project
                .current
                .tracks
                .get(index)
                .map_or($fallback, |track| track.$method().into())
        }
    };
}

track_getter!(nylon_track_volume_db, f64, f64::NEG_INFINITY, volume_db);
track_getter!(nylon_track_pan, f64, 0.0, pan);
track_getter!(nylon_track_mute, i32, 0, muted);
track_getter!(nylon_track_solo, i32, 0, solo);
track_getter!(nylon_track_arm, i32, 0, armed);
track_getter!(nylon_track_color_index, i32, -1, color_index);

unsafe fn edit_track(
    handle: *mut Project,
    index: u64,
    make: impl FnOnce(TrackId) -> Option<Command>,
) -> i32 {
    // SAFETY: The caller transfers the native interface's exclusive access contract.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let Some(track) = project.current.tracks.get(index) else {
        return 0;
    };
    let Some(command) = make(track.id()) else {
        return 0;
    };
    i32::from(project.apply(&[command]).is_ok())
}

macro_rules! track_setter {
    ($name:ident, $value:ty, $make:expr) => {
        /// # Safety
        /// A non-null handle must be live and exclusively accessible to this call.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: *mut Project, index: u64, value: $value) -> i32 {
            // SAFETY: Handle validity and exclusivity are required by the interface.
            unsafe { edit_track(handle, index, |id| ($make)(id, value)) }
        }
    };
}

track_setter!(nylon_track_set_volume_db, f64, |id, db| Some(
    Command::SetTrackVolume { id, db }
));
track_setter!(nylon_track_set_pan, f64, |id, pan| Some(
    Command::SetTrackPan { id, pan }
));
track_setter!(nylon_track_set_mute, i32, |id, value| match value {
    0 | 1 => Some(Command::SetTrackMute {
        id,
        enabled: value == 1
    }),
    _ => None,
});
track_setter!(nylon_track_set_solo, i32, |id, value| match value {
    0 | 1 => Some(Command::SetTrackSolo {
        id,
        enabled: value == 1
    }),
    _ => None,
});
track_setter!(nylon_track_set_arm, i32, |id, value| match value {
    0 | 1 => Some(Command::SetTrackArm {
        id,
        enabled: value == 1
    }),
    _ => None,
});
track_setter!(nylon_track_set_color_index, i32, |id, value| u8::try_from(
    value
)
.ok()
.map(|index| Command::SetTrackColor { id, index }));

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_delete(handle: *mut Project, index: u64) -> i32 {
    // SAFETY: The caller provides a valid exclusive handle.
    unsafe { edit_track(handle, index, |id| Some(Command::DeleteTrack(id))) }
}

/// # Safety
/// A non-null handle must be live with no concurrent mutation. A non-null buffer
/// must be writable for `capacity` bytes and disjoint from the project storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_name(
    handle: *const Project,
    index: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the interface.
    let name = unsafe { handle.as_ref() }
        .and_then(|p| {
            usize::try_from(index)
                .ok()
                .and_then(|i| p.current.tracks.get(i))
        })
        .map_or("", |track| track.name());
    if !buffer.is_null() && capacity > 0 {
        let count = name
            .len()
            .min(usize::try_from(capacity - 1).unwrap_or(usize::MAX));
        // SAFETY: The caller provides writable capacity and nonoverlapping storage.
        unsafe {
            std::ptr::copy_nonoverlapping(name.as_ptr(), buffer.cast(), count);
            buffer.add(count).write(0);
        }
    }
    name.len() as u64
}

/// # Safety
/// The project must be live and exclusive; name must be a valid NUL-terminated
/// string, or null. UTF-8 is required. Null and invalid text are rejected.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_set_name(
    handle: *mut Project,
    index: u64,
    name: *const c_char,
) -> i32 {
    if name.is_null() {
        return 0;
    }
    // SAFETY: The caller provides a terminated readable string.
    let Ok(name) = (unsafe { CStr::from_ptr(name) }).to_str() else {
        return 0;
    };
    // SAFETY: The caller provides a live exclusive project handle.
    unsafe {
        edit_track(handle, index, |id| {
            Some(Command::RenameTrack {
                id,
                name: name.into(),
            })
        })
    }
}

pub(crate) fn kind_from_int(kind: i32) -> Option<TrackKind> {
    match kind {
        0 => Some(TrackKind::Audio),
        1 => Some(TrackKind::Midi),
        2 => Some(TrackKind::Return),
        3 => Some(TrackKind::Master),
        4 => Some(TrackKind::Group),
        5 => Some(TrackKind::Cue),
        _ => None,
    }
}

pub(crate) fn kind_to_int(kind: TrackKind) -> i32 {
    match kind {
        TrackKind::Audio => 0,
        TrackKind::Midi => 1,
        TrackKind::Return => 2,
        TrackKind::Master => 3,
        TrackKind::Group => 4,
        TrackKind::Cue => 5,
    }
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_kind(handle: *const Project, index: u64) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { handle.as_ref() }
        .and_then(|p| {
            usize::try_from(index)
                .ok()
                .and_then(|i| p.current.tracks.get(i))
        })
        .map_or(-1, |track| kind_to_int(track.kind()))
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_add_track_kind(handle: *mut Project, kind: i32) -> i32 {
    let Some(kind) = kind_from_int(kind) else {
        return 0;
    };
    // SAFETY: Handle validity and exclusivity are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let label = match kind {
        TrackKind::Midi => "MIDI",
        TrackKind::Audio => "Audio",
        TrackKind::Return => "Return",
        TrackKind::Master => "Master",
        TrackKind::Group => "Group",
        TrackKind::Cue => "Cue",
    };
    let name = format!("{label} {}", project.current.tracks.len() + 1);
    i32::from(
        project
            .apply(&[Command::CreateTrack { name, kind }])
            .is_ok(),
    )
}

macro_rules! project_getter {
    ($name:ident, $result:ty, $fallback:expr, $read:expr) => {
        /// # Safety
        /// A non-null handle must be live and have no concurrent mutation.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: *const Project) -> $result {
            // SAFETY: Handle validity is required by the interface.
            unsafe { handle.as_ref() }.map_or($fallback, $read)
        }
    };
}
project_getter!(nylon_project_can_undo, i32, 0, |p| i32::from(p.can_undo()));
project_getter!(nylon_project_can_redo, i32, 0, |p| i32::from(p.can_redo()));
project_getter!(nylon_project_time_signature_numerator, i32, 0, |p| {
    i32::from(p.current.numerator)
});
project_getter!(nylon_project_time_signature_denominator, i32, 0, |p| {
    i32::from(p.current.denominator)
});
project_getter!(nylon_project_sample_rate, u32, 0, |p| p.current.sample_rate);

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_new_in_place(handle: *mut Project) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    *project = Project::new();
    1
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_set_time_signature(
    handle: *mut Project,
    numerator: i32,
    denominator: i32,
) -> i32 {
    let (Ok(numerator), Ok(denominator)) = (u16::try_from(numerator), u16::try_from(denominator))
    else {
        return 0;
    };
    // SAFETY: Handle validity and exclusivity are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |p| {
        i32::from(
            p.apply(&[Command::SetTimeSignature {
                numerator,
                denominator,
            }])
            .is_ok(),
        )
    })
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_set_sample_rate(handle: *mut Project, rate: u32) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |p| {
        i32::from(p.apply(&[Command::SetSampleRate(rate)]).is_ok())
    })
}

/// # Safety
/// The project must be live with no concurrent mutation. A non-null directory
/// must be a readable NUL-terminated UTF-8 string. This performs control-thread I/O.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_save(
    handle: *const Project,
    directory: *const c_char,
) -> i32 {
    if directory.is_null() {
        return 0;
    }
    // SAFETY: The caller supplies a terminated readable directory string.
    let Ok(directory) = (unsafe { CStr::from_ptr(directory) }).to_str() else {
        return 0;
    };
    if directory.is_empty() {
        return 0;
    }
    // SAFETY: The handle remains live and cannot be concurrently mutated.
    unsafe { handle.as_ref() }.map_or(0, |p| {
        i32::from(p.save_bundle(std::path::Path::new(directory)).is_ok())
    })
}

/// # Safety
/// The project must be live and exclusive. A non-null directory must be a readable
/// NUL-terminated UTF-8 string. Failed loads preserve the current project and history.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_open(handle: *mut Project, directory: *const c_char) -> i32 {
    if directory.is_null() {
        return 0;
    }
    // SAFETY: The caller provides a live exclusive handle.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    // SAFETY: The caller supplies a terminated readable directory string.
    let Ok(directory) = (unsafe { CStr::from_ptr(directory) }).to_str() else {
        return 0;
    };
    if directory.is_empty() {
        return 0;
    }
    let Ok(loaded) = Project::load_bundle(std::path::Path::new(directory)) else {
        return 0;
    };
    *project = loaded;
    1
}
