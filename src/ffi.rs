//! Native control interface. Handles belong to one control thread and must
//! not be accessed concurrently or used after release.

use crate::project::{ClipId, Command, MidiNote, Project, SceneId, TrackId, TrackKind};
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

fn copy_text(text: &str, buffer: *mut c_char, capacity: u64) -> u64 {
    if !buffer.is_null() && capacity > 0 {
        let count = text
            .len()
            .min(usize::try_from(capacity - 1).unwrap_or(usize::MAX));
        // SAFETY: Native callers provide writable, nonoverlapping output storage.
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), buffer.cast(), count);
            buffer.add(count).write(0);
        }
    }
    text.len() as u64
}

unsafe fn input_text(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: Native callers provide a readable terminated string.
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

fn slot_ids(project: &Project, track: u64, scene: u64) -> Option<(TrackId, SceneId, ClipId)> {
    let track = usize::try_from(track).ok()?;
    let scene = usize::try_from(scene).ok()?;
    let track_ref = project.current.tracks.get(track)?;
    let scene_ref = project.current.scenes.get(scene)?;
    Some((
        track_ref.id,
        scene_ref.id,
        track_ref.session_slots.get(scene)?.as_ref().copied()?,
    ))
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_count(handle: *const Project) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }.map_or(0, |p| p.current.scenes.len() as u64)
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_create(handle: *mut Project, name: *const c_char) -> i32 {
    // SAFETY: The native interface requires readable text and exclusive project access.
    let Some(name) = (unsafe { input_text(name) }) else {
        return 0;
    };
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    unsafe { handle.as_mut() }.map_or(0, |p| {
        i32::from(p.apply(&[Command::CreateScene { name }]).is_ok())
    })
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_delete(handle: *mut Project, scene: u64) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some(id) = usize::try_from(scene)
        .ok()
        .and_then(|i| project.current.scenes.get(i))
        .map(|s| s.id)
    else {
        return 0;
    };
    i32::from(project.apply(&[Command::DeleteScene(id)]).is_ok())
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_name(
    handle: *const Project,
    scene: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    let text = unsafe { handle.as_ref() }
        .and_then(|p| {
            usize::try_from(scene)
                .ok()
                .and_then(|i| p.current.scenes.get(i))
        })
        .map_or("", |s| s.name.as_str());
    copy_text(text, buffer, capacity)
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_set_name(
    handle: *mut Project,
    scene: u64,
    name: *const c_char,
) -> i32 {
    // SAFETY: The native interface requires readable text and exclusive project access.
    let Some(name) = (unsafe { input_text(name) }) else {
        return 0;
    };
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some(id) = usize::try_from(scene)
        .ok()
        .and_then(|i| project.current.scenes.get(i))
        .map(|s| s.id)
    else {
        return 0;
    };
    i32::from(project.apply(&[Command::RenameScene { id, name }]).is_ok())
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_slot_state(
    handle: *const Project,
    track: u64,
    scene: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }.map_or(0, |p| i32::from(slot_ids(p, track, scene).is_some()))
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_create_midi(
    handle: *mut Project,
    track: u64,
    scene: u64,
    length_beats: f64,
) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let (Some(track_id), Some(scene_id)) = (
        usize::try_from(track)
            .ok()
            .and_then(|i| project.current.tracks.get(i))
            .map(|t| t.id),
        usize::try_from(scene)
            .ok()
            .and_then(|i| project.current.scenes.get(i))
            .map(|s| s.id),
    ) else {
        return 0;
    };
    let name = format!("MIDI Clip {}", project.current.clips.len() + 1);
    i32::from(
        project
            .apply(&[Command::CreateMidiClip {
                track: track_id,
                scene: scene_id,
                name,
                length_beats,
            }])
            .is_ok(),
    )
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_delete(handle: *mut Project, track: u64, scene: u64) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some((track, scene, _)) = slot_ids(project, track, scene) else {
        return 0;
    };
    i32::from(
        project
            .apply(&[Command::DeleteClip { track, scene }])
            .is_ok(),
    )
}

unsafe fn edit_slot_clip(
    handle: *mut Project,
    track: u64,
    scene: u64,
    make: impl FnOnce(ClipId) -> Option<Command>,
) -> i32 {
    // SAFETY: The caller transfers the native interface's exclusive access contract.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some((_, _, clip)) = slot_ids(project, track, scene) else {
        return 0;
    };
    let Some(command) = make(clip) else { return 0 };
    i32::from(project.apply(&[command]).is_ok())
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_name(
    handle: *const Project,
    track: u64,
    scene: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    let text = unsafe { handle.as_ref() }
        .and_then(|p| {
            let (_, _, id) = slot_ids(p, track, scene)?;
            p.current
                .clips
                .iter()
                .find(|clip| clip.id == id)
                .map(|clip| clip.name.as_str())
        })
        .unwrap_or("");
    copy_text(text, buffer, capacity)
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_name(
    handle: *mut Project,
    track: u64,
    scene: u64,
    name: *const c_char,
) -> i32 {
    // SAFETY: The native interface requires readable text and exclusive project access.
    let Some(name) = (unsafe { input_text(name) }) else {
        return 0;
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetClipName { id, name })
        })
    }
}

macro_rules! clip_getter {
    ($name:ident, $result:ty, $fallback:expr, $read:expr) => {
        /// # Safety
        /// The handle must be live and have no concurrent mutation.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: *const Project, track: u64, scene: u64) -> $result {
            // SAFETY: Handle validity is required by the native interface.
            unsafe { handle.as_ref() }
                .and_then(|p| {
                    let (_, _, id) = slot_ids(p, track, scene)?;
                    p.current.clips.iter().find(|clip| clip.id == id).map($read)
                })
                .unwrap_or($fallback)
        }
    };
}

clip_getter!(nylon_clip_color_index, i32, -1, |clip| i32::from(
    clip.color_index
));
clip_getter!(nylon_clip_loop_start, f64, 0.0, |clip| clip
    .loop_start_beats);
clip_getter!(nylon_clip_loop_length, f64, 0.0, |clip| clip
    .loop_length_beats);
clip_getter!(nylon_clip_note_count, u64, 0, |clip| clip.notes.len()
    as u64);

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_color_index(
    handle: *mut Project,
    track: u64,
    scene: u64,
    index: i32,
) -> i32 {
    let Ok(index) = u8::try_from(index) else {
        return 0;
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetClipColor { id, index })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_loop(
    handle: *mut Project,
    track: u64,
    scene: u64,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetClipLoop {
                id,
                start_beats,
                length_beats,
            })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_note_at(
    handle: *const Project,
    track: u64,
    scene: u64,
    index: u64,
    pitch: *mut u8,
    velocity: *mut u8,
    start_beats: *mut f64,
    length_beats: *mut f64,
) -> i32 {
    if pitch.is_null() || velocity.is_null() || start_beats.is_null() || length_beats.is_null() {
        return 0;
    }
    // SAFETY: Handle validity is required by the native interface.
    let Some(note) = (unsafe { handle.as_ref() }).and_then(|p| {
        let (_, _, id) = slot_ids(p, track, scene)?;
        let clip = p.current.clips.iter().find(|clip| clip.id == id)?;
        clip.notes.get(usize::try_from(index).ok()?).copied()
    }) else {
        return 0;
    };
    // SAFETY: Native callers provide four writable output values.
    unsafe {
        pitch.write(note.pitch);
        velocity.write(note.velocity);
        start_beats.write(note.start_beats);
        length_beats.write(note.length_beats);
    }
    1
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_note_add(
    handle: *mut Project,
    track: u64,
    scene: u64,
    pitch: u8,
    velocity: u8,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    let note = MidiNote {
        pitch,
        velocity,
        start_beats,
        length_beats,
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::AddNote { id, note })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_note_remove(
    handle: *mut Project,
    track: u64,
    scene: u64,
    index: u64,
) -> i32 {
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::RemoveNote { id, index })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_note_move(
    handle: *mut Project,
    track: u64,
    scene: u64,
    index: u64,
    pitch: u8,
    velocity: u8,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let note = MidiNote {
        pitch,
        velocity,
        start_beats,
        length_beats,
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::MoveNote { id, index, note })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_count(handle: *const Project, track: u64) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }
        .and_then(|p| {
            usize::try_from(track)
                .ok()
                .and_then(|i| p.current.tracks.get(i))
        })
        .map_or(0, |t| t.arrangement.len() as u64)
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_add_from_slot(
    handle: *mut Project,
    track: u64,
    scene: u64,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some((track, _, clip)) = slot_ids(project, track, scene) else {
        return 0;
    };
    i32::from(
        project
            .apply(&[Command::PlaceClip {
                track,
                clip,
                start_beats,
                length_beats,
            }])
            .is_ok(),
    )
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_range(
    handle: *const Project,
    track: u64,
    index: u64,
    start_beats: *mut f64,
    length_beats: *mut f64,
) -> i32 {
    if start_beats.is_null() || length_beats.is_null() {
        return 0;
    }
    // SAFETY: Handle validity is required by the native interface.
    let Some(placement) = (unsafe { handle.as_ref() }).and_then(|p| {
        p.current
            .tracks
            .get(usize::try_from(track).ok()?)?
            .arrangement
            .get(usize::try_from(index).ok()?)
            .copied()
    }) else {
        return 0;
    };
    // SAFETY: Native callers provide two writable output values.
    unsafe {
        start_beats.write(placement.start_beats);
        length_beats.write(placement.length_beats);
    }
    1
}

unsafe fn edit_placement(
    handle: *mut Project,
    track: u64,
    index: u64,
    make: impl FnOnce(TrackId, usize) -> Command,
) -> i32 {
    // SAFETY: The caller transfers the native interface's exclusive access contract.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let (Ok(track_index), Ok(index)) = (usize::try_from(track), usize::try_from(index)) else {
        return 0;
    };
    let Some(track) = project.current.tracks.get(track_index) else {
        return 0;
    };
    if index >= track.arrangement.len() {
        return 0;
    }
    i32::from(project.apply(&[make(track.id, index)]).is_ok())
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_remove(
    handle: *mut Project,
    track: u64,
    index: u64,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_placement(handle, track, index, |track, index| {
            Command::RemovePlacement { track, index }
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_set_range(
    handle: *mut Project,
    track: u64,
    index: u64,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_placement(handle, track, index, |track, index| {
            Command::SetPlacementRange {
                track,
                index,
                start_beats,
                length_beats,
            }
        })
    }
}
