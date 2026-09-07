use nylon::ffi::*;

#[test]
fn native_edit_cycle_uses_the_command_history() {
    let handle = nylon_project_new();
    assert!(!handle.is_null());
    // SAFETY: This thread owns the live handle until the final free.
    unsafe {
        assert_eq!(nylon_project_tempo(handle), 120.0);
        assert_eq!(nylon_project_set_tempo(handle, 133.5), 1);
        assert_eq!(nylon_project_tempo(handle), 133.5);
        assert_eq!(nylon_project_set_tempo(handle, f64::NAN), 0);
        assert_eq!(nylon_project_add_track(handle), 1);
        assert_eq!(nylon_project_track_count(handle), 1);
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_project_track_count(handle), 0);
        assert_eq!(nylon_project_redo(handle), 1);
        assert_eq!(nylon_project_track_count(handle), 1);
        nylon_project_free(handle);
    }
}

#[test]
fn null_handles_are_rejected() {
    // SAFETY: The interface explicitly accepts null as an invalid handle.
    unsafe {
        assert_eq!(nylon_project_set_tempo(std::ptr::null_mut(), 120.0), 0);
        assert_eq!(nylon_project_add_track(std::ptr::null_mut()), 0);
        assert_eq!(nylon_project_undo(std::ptr::null_mut()), 0);
        assert_eq!(nylon_project_redo(std::ptr::null_mut()), 0);
        assert_eq!(nylon_project_track_count(std::ptr::null()), 0);
        assert_eq!(nylon_project_tempo(std::ptr::null()), 0.0);
        nylon_project_free(std::ptr::null_mut());
    }
}
