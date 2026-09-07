//! Native control interface. Handles belong to one control thread and must
//! not be accessed concurrently or used after release.

use crate::project::{Command, Project, TrackKind};

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
