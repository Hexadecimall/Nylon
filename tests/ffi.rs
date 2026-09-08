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

#[test]
fn native_mixer_controls_validate_and_restore_history() {
    let handle = nylon_project_new();
    // SAFETY: This thread owns the live handle and all string buffers.
    unsafe {
        assert_eq!(nylon_project_add_track_kind(handle, 1), 1);
        assert_eq!(nylon_track_kind(handle, 0), 1);
        assert_eq!(nylon_track_set_name(handle, 0, c"Lead".as_ptr()), 1);
        let mut name = [0 as std::ffi::c_char; 32];
        assert_eq!(
            nylon_track_name(handle, 0, name.as_mut_ptr(), name.len() as u64),
            4
        );
        assert_eq!(std::ffi::CStr::from_ptr(name.as_ptr()).to_bytes(), b"Lead");
        assert_eq!(nylon_track_set_volume_db(handle, 0, -9.0), 1);
        assert_eq!(nylon_track_volume_db(handle, 0), -9.0);
        assert_eq!(nylon_track_set_pan(handle, 0, 0.25), 1);
        assert_eq!(nylon_track_pan(handle, 0), 0.25);
        assert_eq!(nylon_track_set_mute(handle, 0, 1), 1);
        assert_eq!(nylon_track_mute(handle, 0), 1);
        assert_eq!(nylon_track_set_solo(handle, 0, 1), 1);
        assert_eq!(nylon_track_solo(handle, 0), 1);
        assert_eq!(nylon_track_set_arm(handle, 0, 1), 1);
        assert_eq!(nylon_track_arm(handle, 0), 1);
        assert_eq!(nylon_track_set_color_index(handle, 0, 15), 1);
        assert_eq!(nylon_track_color_index(handle, 0), 15);
        assert_eq!(nylon_project_can_undo(handle), 1);
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_track_color_index(handle, 0), 0);
        assert_eq!(nylon_project_can_redo(handle), 1);
        assert_eq!(nylon_track_set_mute(handle, 0, 2), 0);
        assert_eq!(nylon_track_set_color_index(handle, 0, 16), 0);
        assert_eq!(nylon_track_set_pan(handle, u64::MAX, 0.0), 0);
        assert_eq!(nylon_project_add_track_kind(handle, -1), 0);
        assert_eq!(nylon_track_delete(handle, 0), 1);
        assert_eq!(nylon_project_track_count(handle), 0);
        assert_eq!(nylon_project_new_in_place(handle), 1);
        assert_eq!(nylon_project_can_undo(handle), 0);
        nylon_project_free(handle);
    }
}

#[test]
fn native_save_open_preserves_state_and_rejects_corrupt_documents() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::path::PathBuf::from("target").join(format!("native-save-{tick}"));
    let path = std::ffi::CString::new(directory.to_str().unwrap()).unwrap();
    let handle = nylon_project_new();
    // SAFETY: This thread owns the handle and supplies terminated path strings.
    unsafe {
        assert_eq!(nylon_project_add_track_kind(handle, 1), 1);
        assert_eq!(nylon_project_set_tempo(handle, 145.0), 1);
        assert_eq!(nylon_project_save(handle, path.as_ptr()), 1);
        assert_eq!(nylon_project_new_in_place(handle), 1);
        assert_eq!(nylon_project_open(handle, path.as_ptr()), 1);
        assert_eq!(nylon_project_tempo(handle), 145.0);
        assert_eq!(nylon_project_track_count(handle), 1);
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_project_tempo(handle), 120.0);
        std::fs::write(directory.join("project.nylon"), b"invalid").unwrap();
        assert_eq!(nylon_project_open(handle, path.as_ptr()), 0);
        assert_eq!(nylon_project_track_count(handle), 1);
        assert_eq!(nylon_project_can_redo(handle), 1);
        nylon_project_free(handle);
    }
    std::fs::remove_dir_all(directory).unwrap();
}
