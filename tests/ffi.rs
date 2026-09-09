use nylon::ffi::*;

#[test]
fn native_audio_plugin_host_configuration_validates_inputs() {
    let audio = nylon_audio_new();
    let roots = [c"plugins".as_ptr(), c"more-plugins".as_ptr()];
    // SAFETY: This thread owns the audio handle and every string is terminated.
    unsafe {
        assert_eq!(
            nylon_audio_configure_plugin_host(
                audio,
                c"bin/nylon-plugin-worker".as_ptr(),
                roots.as_ptr(),
                roots.len() as u64,
            ),
            1
        );
        assert_eq!(
            nylon_audio_configure_plugin_host(
                audio,
                c"bin/nylon-plugin-worker".as_ptr(),
                std::ptr::null(),
                0,
            ),
            0
        );
        assert_eq!(
            nylon_audio_configure_plugin_host(audio, std::ptr::null(), roots.as_ptr(), 2),
            0
        );
        nylon_audio_free(audio);
    }
}

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
fn native_plugin_devices_preserve_metadata_state_and_shared_order() {
    let handle = nylon_project_new();
    let state = [1_u8, 3, 5, 7];
    // SAFETY: This thread owns the project and all pointed-to storage.
    unsafe {
        assert_eq!(nylon_project_add_track(handle), 1);
        assert_eq!(
            nylon_track_plugin_add(
                handle,
                0,
                2,
                c"Effect.clap".as_ptr(),
                c"app.nylon.effect".as_ptr(),
                96,
                state.as_ptr(),
                state.len() as u64,
                1,
            ),
            1
        );
        assert_eq!(nylon_track_device_count(handle, 0), 1);
        assert_eq!(nylon_track_device_type(handle, 0, 0), 1);
        assert_eq!(nylon_track_device_enabled(handle, 0, 0), 1);
        assert_eq!(nylon_track_plugin_format(handle, 0, 0), 2);
        assert_eq!(nylon_track_plugin_latency(handle, 0, 0), 96);
        let mut package = [0 as std::ffi::c_char; 32];
        assert_eq!(
            nylon_track_plugin_package(handle, 0, 0, package.as_mut_ptr(), 32),
            11
        );
        assert_eq!(
            std::ffi::CStr::from_ptr(package.as_ptr()).to_bytes(),
            b"Effect.clap"
        );
        let mut restored = [0_u8; 4];
        assert_eq!(
            nylon_track_plugin_state(handle, 0, 0, restored.as_mut_ptr(), 4),
            4
        );
        assert_eq!(restored, state);
        assert_eq!(nylon_track_plugin_parameter_count(handle, 0, 0), 0);
        assert_eq!(nylon_track_plugin_parameter_set(handle, 0, 0, 71, 0.625), 1);
        assert_eq!(nylon_track_plugin_parameter_count(handle, 0, 0), 1);
        let mut identifier = 0;
        let mut value = 0.0;
        assert_eq!(
            nylon_track_plugin_parameter_get(handle, 0, 0, 0, &mut identifier, &mut value,),
            1
        );
        assert_eq!((identifier, value), (71, 0.625));
        assert_eq!(
            nylon_track_plugin_parameter_set(handle, 0, 0, 71, f64::NAN),
            0
        );
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_track_plugin_parameter_count(handle, 0, 0), 0);
        assert_eq!(nylon_track_device_set_enabled(handle, 0, 0, 0), 1);
        assert_eq!(nylon_track_device_enabled(handle, 0, 0), 0);
        assert_eq!(
            nylon_track_plugin_set_state(handle, 0, 0, [9_u8, 8].as_ptr(), 2),
            1
        );
        assert_eq!(
            nylon_track_plugin_state(handle, 0, 0, std::ptr::null_mut(), 0),
            2
        );
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(
            nylon_track_plugin_state(handle, 0, 0, std::ptr::null_mut(), 0),
            4
        );
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
        assert_eq!(
            nylon_track_instrument_get(std::ptr::null(), 0, std::ptr::null_mut()),
            0
        );
        nylon_project_free(std::ptr::null_mut());
        assert_eq!(nylon_audio_close(std::ptr::null_mut()), 0);
        assert_eq!(nylon_audio_is_open(std::ptr::null()), 0);
        assert_eq!(nylon_audio_frames_rendered(std::ptr::null()), 0);
        assert_eq!(nylon_transport_play(std::ptr::null_mut()), 0);
        assert_eq!(nylon_transport_stop(std::ptr::null_mut()), 0);
        assert_eq!(nylon_transport_locate(std::ptr::null_mut(), 1.0), 0);
        assert_eq!(nylon_transport_position_beats(std::ptr::null_mut()), 0.0);
        assert_eq!(
            nylon_session_launch_clip(std::ptr::null_mut(), std::ptr::null(), 0, 0, 1.0),
            0
        );
        assert_eq!(
            nylon_session_launch_scene(std::ptr::null_mut(), std::ptr::null(), 0, 1.0),
            0
        );
        assert_eq!(
            nylon_session_stop_track(std::ptr::null_mut(), std::ptr::null(), 0),
            0
        );
        assert_eq!(nylon_session_active_scene(std::ptr::null(), 0), -1);
        assert_eq!(
            nylon_live_note_on(std::ptr::null_mut(), std::ptr::null(), 0, 60, 100),
            0
        );
        assert_eq!(
            nylon_live_note_off(std::ptr::null_mut(), std::ptr::null(), 0, 60),
            0
        );
        assert_eq!(
            nylon_live_all_notes_off(std::ptr::null_mut(), std::ptr::null(), 0),
            0
        );
        assert_eq!(
            nylon_live_all_notes_off_all(std::ptr::null_mut(), std::ptr::null()),
            0
        );
        assert_eq!(nylon_audio_default_output(std::ptr::null_mut()), 0);
        assert_eq!(nylon_audio_default_input(std::ptr::null_mut()), 0);
        assert_eq!(nylon_recording_start(std::ptr::null_mut()), 0);
        assert_eq!(nylon_recording_stop(std::ptr::null_mut()), 0);
        assert_eq!(nylon_recording_is_running(std::ptr::null()), 0);
        nylon_recording_free(std::ptr::null_mut());
        assert!(nylon_clap_instance_open(std::ptr::null(), std::ptr::null()).is_null());
        assert_eq!(
            nylon_clap_instance_activate(std::ptr::null_mut(), 48_000.0, 1, 256),
            0
        );
        assert_eq!(
            nylon_clap_instance_process_stereo(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            ),
            0
        );
        assert_eq!(
            nylon_clap_instance_process_stereo_events(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                0,
            ),
            0
        );
        assert_eq!(
            nylon_clap_instance_process_stereo_all_events(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            ),
            0
        );
        assert_eq!(nylon_clap_instance_input_note_ports(std::ptr::null()), 0);
        assert_eq!(nylon_clap_instance_input_audio_ports(std::ptr::null()), 0);
        let mut parameter = NylonClapParameterInfo::default();
        let mut value = 0.0;
        let mut latency = 0;
        assert_eq!(nylon_clap_instance_parameter_count(std::ptr::null()), 0);
        assert_eq!(
            nylon_clap_instance_parameter_info(std::ptr::null(), 0, &mut parameter),
            0
        );
        assert_eq!(
            nylon_clap_instance_parameter_value(std::ptr::null(), 0, &mut value),
            0
        );
        assert_eq!(
            nylon_clap_instance_latency(std::ptr::null(), &mut latency),
            0
        );
        assert!(nylon_clap_instance_save_state(std::ptr::null()).is_null());
        assert_eq!(
            nylon_clap_instance_load_state(std::ptr::null_mut(), std::ptr::null(), 0),
            0
        );
        assert_eq!(nylon_clap_state_size(std::ptr::null()), 0);
        assert!(nylon_clap_state_data(std::ptr::null()).is_null());
        nylon_clap_state_free(std::ptr::null_mut());
        assert_eq!(nylon_clap_instance_reset(std::ptr::null_mut()), 0);
        assert_eq!(nylon_clap_instance_take_requests(std::ptr::null()), 0);
        nylon_clap_instance_free(std::ptr::null_mut());
        assert!(
            nylon_clap_worker_open(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                48_000.0,
                256,
            )
            .is_null()
        );
        assert_eq!(
            nylon_clap_worker_process_stereo(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            ),
            0
        );
        assert_eq!(nylon_clap_worker_input_note_ports(std::ptr::null()), 0);
        assert_eq!(nylon_clap_worker_input_audio_ports(std::ptr::null()), 0);
        assert_eq!(nylon_clap_worker_parameter_count(std::ptr::null()), 0);
        assert_eq!(
            nylon_clap_worker_parameter_info(std::ptr::null(), 0, &mut parameter),
            0
        );
        assert_eq!(nylon_clap_worker_latency(std::ptr::null(), &mut latency), 0);
        assert!(nylon_clap_worker_save_state(std::ptr::null_mut()).is_null());
        assert_eq!(
            nylon_clap_worker_load_state(std::ptr::null_mut(), std::ptr::null(), 0),
            0
        );
        nylon_clap_worker_free(std::ptr::null_mut());
        assert!(
            nylon_clap_bridge_open(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                48_000.0,
                256,
                3,
            )
            .is_null()
        );
        assert_eq!(
            nylon_clap_bridge_process_stereo(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            ),
            0
        );
        assert_eq!(nylon_clap_bridge_latency(std::ptr::null()), 0);
        assert_eq!(nylon_clap_bridge_is_running(std::ptr::null()), 0);
        assert_eq!(nylon_clap_bridge_submitted_blocks(std::ptr::null()), 0);
        assert_eq!(nylon_clap_bridge_completed_blocks(std::ptr::null()), 0);
        assert_eq!(nylon_clap_bridge_underruns(std::ptr::null()), 0);
        assert_eq!(nylon_clap_bridge_queue_drops(std::ptr::null()), 0);
        assert_eq!(nylon_clap_bridge_worker_failures(std::ptr::null()), 0);
        nylon_clap_bridge_free(std::ptr::null_mut());
        let mut device = NylonTrackDevice::default();
        assert_eq!(nylon_track_device_count(std::ptr::null(), 0), 0);
        assert_eq!(
            nylon_track_device_get(std::ptr::null(), 0, 0, &mut device),
            0
        );
        assert_eq!(nylon_track_device_add(std::ptr::null_mut(), 0, &device), 0);
        assert_eq!(
            nylon_track_device_set(std::ptr::null_mut(), 0, 0, &device),
            0
        );
        assert_eq!(nylon_track_device_delete(std::ptr::null_mut(), 0, 0), 0);
        assert_eq!(nylon_track_device_move(std::ptr::null_mut(), 0, 0, 0), 0);
        let mut report = NylonBounceReport::default();
        assert_eq!(
            nylon_render_bounce_wave(
                std::ptr::null(),
                c"target/null.wav".as_ptr(),
                0.0,
                1.0,
                48_000,
                &mut report,
            ),
            0
        );
        nylon_audio_free(std::ptr::null_mut());
    }
}

#[test]
fn native_instrument_patch_is_typed_and_undoable() {
    let handle = nylon_project_new();
    // SAFETY: This thread owns the handle and the patch record until release.
    unsafe {
        assert_eq!(nylon_project_add_track_kind(handle, 1), 1);
        assert_eq!(nylon_project_add_track_kind(handle, 0), 1);
        let mut patch = NylonInstrumentPatch::default();
        assert_eq!(nylon_track_instrument_get(handle, 0, &mut patch), 1);
        assert_eq!(patch.shape_a, 1);
        assert_eq!(patch.shape_b, 2);
        patch.oscillator_mix = 0.7;
        patch.sub_level = 0.4;
        patch.noise_level = 0.05;
        patch.unison_voices = 4;
        patch.unison_detune_cents = 18.0;
        patch.cutoff_hz = 2_400.0;
        assert_eq!(nylon_track_instrument_set(handle, 0, &patch), 1);
        let mut read = NylonInstrumentPatch::default();
        assert_eq!(nylon_track_instrument_get(handle, 0, &mut read), 1);
        assert_eq!(read, patch);
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_track_instrument_get(handle, 0, &mut read), 1);
        assert_ne!(read, patch);
        assert_eq!(nylon_project_redo(handle), 1);
        assert_eq!(nylon_track_instrument_get(handle, 0, &mut read), 1);
        assert_eq!(read, patch);

        patch.unison_voices = 0;
        assert_eq!(nylon_track_instrument_set(handle, 0, &patch), 0);
        assert_eq!(nylon_track_instrument_set(handle, 1, &read), 0);
        assert_eq!(nylon_track_instrument_get(handle, 1, &mut read), 0);
        assert_eq!(
            nylon_track_instrument_get(handle, 0, std::ptr::null_mut()),
            0
        );
        assert_eq!(nylon_track_instrument_set(handle, 0, std::ptr::null()), 0);
        nylon_project_free(handle);
    }
}

#[test]
fn native_bounce_writes_the_selected_range() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::path::PathBuf::from("target").join(format!("native-bounce-{tick}.wav"));
    let native_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let handle = nylon_project_new();
    let mut report = NylonBounceReport::default();
    // SAFETY: This thread owns the handle, path string, and report.
    unsafe {
        assert_eq!(
            nylon_render_bounce_wave(handle, native_path.as_ptr(), 2.0, 3.0, 48_000, &mut report,),
            1
        );
        assert_eq!(report.frames, 24_000);
        assert_eq!(report.peak_left, 0.0);
        assert_eq!(report.peak_right, 0.0);
        assert_eq!(
            nylon_render_bounce_wave(handle, native_path.as_ptr(), 3.0, 2.0, 48_000, &mut report,),
            0
        );
        nylon_project_free(handle);
    }
    let bytes = std::fs::read(&path).unwrap();
    let decoded = nylon::wave::read(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(decoded.format.sample_rate, 48_000);
    assert_eq!(decoded.frames(), 24_000);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn native_audio_handle_reports_closed_state() {
    let project = nylon_project_new();
    let audio = nylon_audio_new();
    assert!(!project.is_null());
    assert!(!audio.is_null());
    // SAFETY: This thread owns both handles and every output record.
    unsafe {
        assert_eq!(nylon_audio_is_open(audio), 0);
        assert_eq!(nylon_transport_play(audio), 0);
        assert_eq!(nylon_transport_stop(audio), 0);
        assert_eq!(nylon_transport_locate(audio, 4.0), 0);
        assert_eq!(nylon_transport_locate(audio, f64::NAN), 0);
        assert_eq!(nylon_transport_position_beats(audio), 0.0);
        assert_eq!(nylon_transport_is_playing(audio), 0);
        assert_eq!(nylon_audio_dropouts(audio), 0);
        assert_eq!(nylon_audio_frames_rendered(audio), 0);
        let mut config = NylonAudioConfig::default();
        assert_eq!(nylon_audio_config(audio, &mut config), 0);
        let mut master = NylonLevels::default();
        assert_eq!(nylon_master_levels(audio, &mut master), 1);
        assert_eq!(master, NylonLevels::default());
        assert_eq!(nylon_track_levels(audio, 0, &mut master), 0);
        assert_eq!(nylon_audio_sync(audio, project), 0);
        assert_eq!(nylon_session_launch_clip(audio, project, 0, 0, 1.0), 0);
        assert_eq!(nylon_session_launch_scene(audio, project, 0, 1.0), 0);
        assert_eq!(nylon_session_stop_track(audio, project, 0), 0);
        assert_eq!(nylon_session_active_scene(audio, 0), -1);
        assert_eq!(nylon_live_note_on(audio, project, 0, 60, 100), 0);
        assert_eq!(nylon_live_note_off(audio, project, 0, 60), 0);
        assert_eq!(nylon_live_all_notes_off(audio, project, 0), 0);
        assert_eq!(nylon_live_all_notes_off_all(audio, project), 0);
        let count = nylon_audio_device_list(std::ptr::null_mut(), 0);
        assert!(count <= nylon::runtime::MAX_DEVICES as u64);
        let input_count = nylon_audio_input_device_list(std::ptr::null_mut(), 0);
        assert!(input_count <= nylon::runtime::MAX_DEVICES as u64);
        nylon_audio_free(audio);
        nylon_project_free(project);
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs a real audio output device"]
fn native_audio_interface_drives_the_platform_stream() {
    let project = nylon_project_new();
    let audio = nylon_audio_new();
    // SAFETY: This thread owns both handles and the output configuration.
    unsafe {
        let mut device = 0;
        assert_eq!(nylon_audio_default_output(&mut device), 1);
        assert_eq!(nylon_audio_open(audio, project, device, 48_000, 256), 1);
        assert_eq!(nylon_audio_is_open(audio), 1);
        let mut config = NylonAudioConfig::default();
        assert_eq!(nylon_audio_config(audio, &mut config), 1);
        assert_eq!(config.channels, 2);
        assert_eq!(nylon_transport_play(audio), 1);
        let mut position = 0.0;
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            position = nylon_transport_position_beats(audio);
            if position > 0.0 {
                break;
            }
        }
        assert!(position > 0.0, "{position}");
        assert_eq!(nylon_transport_locate(audio, 8.0), 1);
        assert_eq!(nylon_transport_stop(audio), 1);
        assert_eq!(nylon_audio_close(audio), 1);
        assert_eq!(nylon_audio_is_open(audio), 0);
        nylon_audio_free(audio);
        nylon_project_free(project);
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
fn native_track_devices_are_typed_ordered_and_undoable() {
    let handle = nylon_project_new();
    // SAFETY: This thread owns the handle and every device record.
    unsafe {
        assert_eq!(nylon_project_add_track(handle), 1);
        let utility = NylonTrackDevice {
            kind: 0,
            enabled: 0,
            parameters: [
                -6.0, 1.25, -0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        let delay = NylonTrackDevice {
            kind: 3,
            enabled: 1,
            parameters: [
                0.25, 0.4, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        let limiter = NylonTrackDevice {
            kind: 4,
            enabled: 1,
            parameters: [
                -0.3, 0.1, 0.005, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        let saturator = NylonTrackDevice {
            kind: 5,
            enabled: 1,
            parameters: [
                9.0, -4.0, 0.75, 3.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        let gate = NylonTrackDevice {
            kind: 6,
            enabled: 1,
            parameters: [
                -32.0, 8.0, 0.002, 0.04, 0.15, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0,
            ],
        };
        let chorus = NylonTrackDevice {
            kind: 7,
            enabled: 1,
            parameters: [
                0.8, 0.012, 0.003, 0.1, 0.5, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        let reverb = NylonTrackDevice {
            kind: 8,
            enabled: 1,
            parameters: [
                0.6, 2.8, 0.35, 0.7, 0.02, 1.0, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        let mut auto_filter = NylonTrackDevice {
            kind: 9,
            enabled: 1,
            ..NylonTrackDevice::default()
        };
        auto_filter.parameters[..11].copy_from_slice(&[
            2.0, 1_600.0, 2.5, 8.0, -1.5, 0.004, 0.2, 0.75, 1.25, 0.6, 1.0,
        ]);
        let mut phaser = NylonTrackDevice {
            kind: 10,
            enabled: 1,
            ..NylonTrackDevice::default()
        };
        phaser.parameters[..7].copy_from_slice(&[0.6, 850.0, 2.25, 0.45, 0.7, 0.4, 10.0]);
        assert_eq!(nylon_track_device_add(handle, 0, &utility), 1);
        assert_eq!(nylon_track_device_add(handle, 0, &delay), 1);
        assert_eq!(nylon_track_device_add(handle, 0, &limiter), 1);
        assert_eq!(nylon_track_device_add(handle, 0, &saturator), 1);
        assert_eq!(nylon_track_device_add(handle, 0, &gate), 1);
        assert_eq!(nylon_track_device_add(handle, 0, &chorus), 1);
        assert_eq!(nylon_track_device_add(handle, 0, &reverb), 1);
        assert_eq!(nylon_track_device_add(handle, 0, &auto_filter), 1);
        assert_eq!(nylon_track_device_add(handle, 0, &phaser), 1);
        assert_eq!(nylon_track_device_count(handle, 0), 9);
        let mut read = NylonTrackDevice::default();
        assert_eq!(nylon_track_device_get(handle, 0, 0, &mut read), 1);
        assert_eq!(read, utility);
        assert_eq!(nylon_track_device_move(handle, 0, 1, 0), 1);
        assert_eq!(nylon_track_device_get(handle, 0, 0, &mut read), 1);
        assert_eq!(read, delay);
        assert_eq!(nylon_track_device_get(handle, 0, 2, &mut read), 1);
        assert_eq!(read, limiter);
        assert_eq!(nylon_track_device_get(handle, 0, 3, &mut read), 1);
        assert_eq!(read, saturator);
        assert_eq!(nylon_track_device_get(handle, 0, 4, &mut read), 1);
        assert_eq!(read, gate);
        assert_eq!(nylon_track_device_get(handle, 0, 5, &mut read), 1);
        assert_eq!(read, chorus);
        assert_eq!(nylon_track_device_get(handle, 0, 6, &mut read), 1);
        assert_eq!(read, reverb);
        assert_eq!(nylon_track_device_get(handle, 0, 7, &mut read), 1);
        assert_eq!(read, auto_filter);
        assert_eq!(nylon_track_device_get(handle, 0, 8, &mut read), 1);
        assert_eq!(read, phaser);

        let changed = NylonTrackDevice {
            kind: 0,
            enabled: 1,
            parameters: [
                -3.0, 0.5, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        assert_eq!(nylon_track_device_set(handle, 0, 1, &changed), 1);
        assert_eq!(nylon_track_device_get(handle, 0, 1, &mut read), 1);
        assert_eq!(read, changed);
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_track_device_get(handle, 0, 1, &mut read), 1);
        assert_eq!(read, utility);

        let invalid = NylonTrackDevice {
            kind: 3,
            enabled: 1,
            parameters: [
                0.25, 1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        };
        assert_eq!(nylon_track_device_set(handle, 0, 0, &invalid), 0);
        assert_eq!(nylon_track_device_delete(handle, 0, 0), 1);
        assert_eq!(nylon_track_device_count(handle, 0), 8);
        assert_eq!(nylon_track_device_get(handle, 0, 8, &mut read), 0);
        assert_eq!(
            nylon_track_device_get(handle, 0, 0, std::ptr::null_mut()),
            0
        );
        nylon_project_free(handle);
    }
}

#[test]
fn native_track_automation_is_typed_and_undoable() {
    let handle = nylon_project_new();
    // SAFETY: This thread owns the handle and every point record.
    unsafe {
        assert_eq!(nylon_project_add_track(handle), 1);
        let points = [
            NylonAutomationPoint {
                beat: 0.0,
                value: -12.0,
                curve: 1,
            },
            NylonAutomationPoint {
                beat: 4.0,
                value: 0.0,
                curve: 2,
            },
        ];
        assert_eq!(
            nylon_track_automation_set(handle, 0, 0, points.as_ptr(), points.len() as u64),
            1
        );
        assert_eq!(nylon_track_automation_count(handle, 0, 0), 2);
        let mut read = NylonAutomationPoint::default();
        assert_eq!(nylon_track_automation_get(handle, 0, 0, 1, &mut read), 1);
        assert_eq!(read, points[1]);
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_track_automation_count(handle, 0, 0), 0);
        assert_eq!(nylon_project_redo(handle), 1);
        assert_eq!(nylon_track_automation_clear(handle, 0, 0), 1);
        assert_eq!(nylon_track_automation_count(handle, 0, 0), 0);

        let invalid = [NylonAutomationPoint {
            beat: 0.0,
            value: 0.5,
            curve: 1,
        }];
        assert_eq!(
            nylon_track_automation_set(handle, 0, 2, invalid.as_ptr(), 1),
            0
        );
        assert_eq!(
            nylon_track_automation_set(handle, 0, 0, std::ptr::null(), 1),
            0
        );
        assert_eq!(
            nylon_track_automation_get(handle, 0, 0, 0, std::ptr::null_mut()),
            0
        );
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

#[test]
fn native_recovery_keeps_the_primary_document_separate() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::path::PathBuf::from("target").join(format!("native-recovery-{tick}"));
    let path = std::ffi::CString::new(directory.to_str().unwrap()).unwrap();
    let handle = nylon_project_new();
    // SAFETY: This thread owns the handle and supplies a terminated path string.
    unsafe {
        assert_eq!(nylon_project_save(handle, path.as_ptr()), 1);
        assert_eq!(nylon_project_is_modified(handle), 0);
        assert_eq!(nylon_project_set_tempo(handle, 152.0), 1);
        assert_eq!(nylon_project_is_modified(handle), 1);
        assert_eq!(nylon_project_autosave(handle), 1);
        assert_eq!(nylon_project_recovery_available(path.as_ptr()), 1);
        assert_eq!(nylon_project_new_in_place(handle), 1);
        assert_eq!(nylon_project_open(handle, path.as_ptr()), 1);
        assert_eq!(nylon_project_tempo(handle), 120.0);
        assert_eq!(nylon_project_recover(handle, path.as_ptr()), 1);
        assert_eq!(nylon_project_tempo(handle), 152.0);
        assert_eq!(nylon_project_is_modified(handle), 1);
        assert_eq!(nylon_project_discard_recovery(path.as_ptr()), 1);
        assert_eq!(nylon_project_recovery_available(path.as_ptr()), 0);
        nylon_project_free(handle);
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn native_routing_reports_order_and_compensation() {
    let routing = nylon_routing_new(4);
    assert!(!routing.is_null());
    // SAFETY: This thread owns both routing handles and output values.
    unsafe {
        assert_eq!(nylon_routing_set_node_latency(routing, 0, 128), 1);
        assert_eq!(nylon_routing_set_node_latency(routing, 1, 32), 1);
        assert_eq!(nylon_routing_set_node_latency(routing, 2, 64), 1);
        let mut slow = 99;
        let mut medium = 99;
        assert_eq!(nylon_routing_add_edge(routing, 0, 3, 0, 1.0, &mut slow), 1);
        assert_eq!(
            nylon_routing_add_edge(routing, 1, 2, 3, 1.0, &mut medium),
            1
        );
        assert_eq!(
            nylon_routing_add_edge(routing, 2, 3, 2, 0.5, &mut medium),
            1
        );
        assert_eq!(
            nylon_routing_add_edge(routing, 2, 3, 2, 0.5, &mut medium),
            0
        );
        let compiled = nylon_routing_compile(routing);
        assert!(!compiled.is_null());
        assert_eq!(nylon_compiled_routing_node_count(compiled), 4);
        assert_eq!(nylon_compiled_routing_order_at(compiled, 0), 0);
        assert_eq!(nylon_compiled_routing_order_at(compiled, 4), -1);
        let mut frames = 0;
        assert_eq!(
            nylon_compiled_routing_edge_delay(compiled, slow, &mut frames),
            1
        );
        assert_eq!(frames, 0);
        assert_eq!(
            nylon_compiled_routing_edge_delay(compiled, medium, &mut frames),
            1
        );
        assert_eq!(frames, 32);
        assert_eq!(
            nylon_compiled_routing_output_latency(compiled, 3, &mut frames),
            1
        );
        assert_eq!(frames, 128);
        nylon_compiled_routing_free(compiled);
        nylon_routing_free(routing);
    }
}

#[test]
fn native_project_routing_is_editable_by_track_index() {
    let handle = nylon_project_new();
    // SAFETY: This thread owns the project handle until release.
    unsafe {
        assert_eq!(nylon_project_add_track_kind(handle, 0), 1);
        assert_eq!(nylon_project_add_track_kind(handle, 2), 1);
        assert_eq!(nylon_track_set_latency_frames(handle, 0, 512), 1);
        assert_eq!(nylon_track_latency_frames(handle, 0), 512);
        assert_eq!(nylon_project_route_add(handle, 0, 1, 2, 0.75), 1);
        assert_eq!(nylon_project_route_count(handle), 1);
        assert_eq!(nylon_project_route_source(handle, 0), 0);
        assert_eq!(nylon_project_route_destination(handle, 0), 1);
        assert_eq!(nylon_project_route_kind(handle, 0), 2);
        assert_eq!(nylon_project_route_gain(handle, 0), 0.75);
        assert_eq!(nylon_project_route_add(handle, 1, 0, 0, 1.0), 0);
        assert_eq!(nylon_project_route_count(handle), 1);
        assert_eq!(nylon_project_route_delete(handle, 0), 1);
        assert_eq!(nylon_project_route_count(handle), 0);
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_project_route_count(handle), 1);
        nylon_project_free(handle);
    }
}

#[test]
fn native_session_notes_and_arrangement_are_editable() {
    let handle = nylon_project_new();
    // SAFETY: This thread owns the handle and every output buffer until release.
    unsafe {
        assert_eq!(nylon_project_add_track_kind(handle, 1), 1);
        assert_eq!(nylon_scene_create(handle, c"Verse".as_ptr()), 1);
        assert_eq!(nylon_scene_count(handle), 9);
        assert_eq!(nylon_clip_slot_state(handle, 0, 0), 0);
        assert_eq!(nylon_clip_create_midi(handle, 0, 0, 4.0), 1);
        assert_eq!(nylon_clip_slot_state(handle, 0, 0), 1);
        assert_eq!(nylon_clip_set_name(handle, 0, 0, c"Chords".as_ptr()), 1);
        assert_eq!(nylon_clip_set_color_index(handle, 0, 0, 6), 1);
        assert_eq!(nylon_clip_set_loop(handle, 0, 0, 1.0, 3.0), 1);
        assert_eq!(nylon_clip_note_add(handle, 0, 0, 64, 100, 1.0, 0.5), 1);
        assert_eq!(nylon_clip_note_add(handle, 0, 0, 60, 110, 0.0, 1.0), 1);
        assert_eq!(nylon_clip_note_count(handle, 0, 0), 2);
        let (mut pitch, mut velocity) = (0, 0);
        let (mut start, mut length) = (0.0, 0.0);
        assert_eq!(
            nylon_clip_note_at(
                handle,
                0,
                0,
                0,
                &mut pitch,
                &mut velocity,
                &mut start,
                &mut length,
            ),
            1
        );
        assert_eq!((pitch, velocity, start, length), (60, 110, 0.0, 1.0));
        assert_eq!(nylon_clip_note_move(handle, 0, 0, 0, 61, 90, 2.0, 0.25), 1);
        assert_eq!(
            nylon_arrangement_clip_add_from_slot(handle, 0, 0, 8.0, 4.0),
            1
        );
        assert_eq!(nylon_arrangement_clip_count(handle, 0), 1);
        assert_eq!(
            nylon_arrangement_clip_range(handle, 0, 0, &mut start, &mut length),
            1
        );
        assert_eq!((start, length), (8.0, 4.0));
        let mut name = [0 as std::ffi::c_char; 32];
        assert_eq!(
            nylon_arrangement_clip_name(handle, 0, 0, name.as_mut_ptr(), name.len() as u64),
            6
        );
        assert_eq!(
            std::ffi::CStr::from_ptr(name.as_ptr()).to_bytes(),
            b"Chords"
        );
        assert_eq!(nylon_arrangement_clip_color_index(handle, 0, 0), 6);
        assert_eq!(nylon_clip_note_add(handle, 0, 0, 128, 100, 0.0, 1.0), 0);
        assert_eq!(nylon_clip_note_add(handle, 0, 0, 60, 0, 0.0, 1.0), 0);
        assert_eq!(nylon_clip_create_midi(handle, 0, 0, 4.0), 0);
        assert_eq!(nylon_project_undo(handle), 1);
        assert_eq!(nylon_arrangement_clip_count(handle, 0), 0);
        assert_eq!(nylon_project_redo(handle), 1);
        assert_eq!(nylon_arrangement_clip_count(handle, 0), 1);
        nylon_project_free(handle);
    }
}

#[test]
fn native_audio_clip_import_and_edits_are_exposed() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::path::PathBuf::from("target").join(format!("native-media-{tick}"));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("take.wav");
    let file = std::fs::File::create(&source).unwrap();
    let mut writer =
        nylon::wave::WaveWriter::new(file, nylon::wave::Format::stereo(48_000)).unwrap();
    writer.write_stereo(&vec![[0.25, -0.25]; 480]).unwrap();
    writer.finish().unwrap();
    let bundle = directory.join("Session.nylonproject");
    let bundle_text = std::ffi::CString::new(bundle.to_str().unwrap()).unwrap();
    let source_text = std::ffi::CString::new(source.to_str().unwrap()).unwrap();
    let handle = nylon_project_new();
    // SAFETY: This thread owns the project and every native string and buffer.
    unsafe {
        assert_eq!(nylon_project_save(handle, bundle_text.as_ptr()), 1);
        assert_eq!(nylon_project_add_track_kind(handle, 0), 1);
        assert_eq!(
            nylon_clip_import_wave(handle, source_text.as_ptr(), 0, 0, 120.0),
            1
        );
        assert_eq!(nylon_clip_slot_state(handle, 0, 0), 2);
        assert_eq!(nylon_clip_audio_gain_db(handle, 0, 0), 0.0);
        assert_eq!(nylon_clip_set_audio_gain_db(handle, 0, 0, -5.0), 1);
        assert_eq!(nylon_clip_audio_gain_db(handle, 0, 0), -5.0);
        assert_eq!(nylon_clip_set_audio_reverse(handle, 0, 0, 1), 1);
        assert_eq!(nylon_clip_audio_reverse(handle, 0, 0), 1);
        assert_eq!(nylon_clip_set_audio_warp(handle, 0, 0, 1, 128.0), 1);
        assert_eq!(nylon_clip_audio_warp(handle, 0, 0), 1);
        assert_eq!(nylon_clip_audio_source_tempo(handle, 0, 0), 128.0);
        let mut path = [0 as std::ffi::c_char; 64];
        let length = nylon_clip_media_path(handle, 0, 0, path.as_mut_ptr(), path.len() as u64);
        assert!(length > 0);
        assert!(
            std::ffi::CStr::from_ptr(path.as_ptr())
                .to_str()
                .unwrap()
                .starts_with("Media/")
        );
        assert_eq!(nylon_project_save(handle, bundle_text.as_ptr()), 1);
        nylon_project_free(handle);
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn native_plugin_catalog_exposes_discovery_and_quarantine() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::path::PathBuf::from("target").join(format!("native-plugins-{tick}"));
    std::fs::create_dir_all(root.join("Alpha.component")).unwrap();
    std::fs::create_dir_all(root.join("Nested/Beta.vst3")).unwrap();
    std::fs::write(root.join("Effect.clap"), b"binary").unwrap();
    let root_text = std::ffi::CString::new(root.to_str().unwrap()).unwrap();
    let missing_text = std::ffi::CString::new(root.join("missing").to_str().unwrap()).unwrap();
    let roots = [root_text.as_ptr(), missing_text.as_ptr()];
    // SAFETY: This thread owns every input string and the returned catalog handle.
    unsafe {
        let catalog = nylon_plugin_catalog_scan(roots.as_ptr(), roots.len() as u64);
        assert!(!catalog.is_null());
        assert_eq!(nylon_plugin_catalog_entry_count(catalog), 3);
        assert_eq!(nylon_plugin_catalog_issue_count(catalog), 1);
        assert_eq!(nylon_plugin_catalog_entry_format(catalog, 0), 1);
        assert_eq!(nylon_plugin_catalog_entry_state(catalog, 0), 0);
        let mut name = [0 as std::ffi::c_char; 32];
        assert_eq!(
            nylon_plugin_catalog_entry_name(catalog, 0, name.as_mut_ptr(), name.len() as u64),
            5
        );
        assert_eq!(std::ffi::CStr::from_ptr(name.as_ptr()).to_bytes(), b"Alpha");
        let descriptors = [nylon::plugin::probe::Descriptor {
            id: "app.nylon.fixture".into(),
            name: "Fixture".into(),
            vendor: "Nylon Contributors".into(),
            version: "1.0".into(),
            features: vec!["audio-effect".into()],
        }];
        let mut protocol = Vec::new();
        nylon::plugin::probe::write_protocol(&descriptors, &mut protocol).unwrap();
        assert_eq!(
            nylon_plugin_catalog_apply_probe(catalog, 1, protocol.as_ptr(), protocol.len() as u64),
            1
        );
        assert_eq!(nylon_plugin_catalog_descriptor_count(catalog, 1), 1);
        let mut identifier = [0 as std::ffi::c_char; 32];
        assert_eq!(
            nylon_plugin_catalog_descriptor_id(
                catalog,
                1,
                0,
                identifier.as_mut_ptr(),
                identifier.len() as u64
            ),
            17
        );
        assert_eq!(
            std::ffi::CStr::from_ptr(identifier.as_ptr()).to_bytes(),
            b"app.nylon.fixture"
        );
        assert_eq!(
            nylon_plugin_catalog_descriptor_feature_count(catalog, 1, 0),
            1
        );
        assert_eq!(
            nylon_plugin_catalog_quarantine(catalog, 0, c"Probe process exited".as_ptr()),
            1
        );
        assert_eq!(nylon_plugin_catalog_entry_state(catalog, 0), 1);
        assert_eq!(nylon_plugin_catalog_retry(catalog, 0), 1);
        assert_eq!(nylon_plugin_catalog_entry_state(catalog, 0), 0);
        assert_eq!(nylon_plugin_catalog_entry_format(catalog, 3), -1);
        nylon_plugin_catalog_free(catalog);
        assert!(nylon_plugin_catalog_scan(std::ptr::null(), 1).is_null());
    }
    std::fs::remove_dir_all(root).unwrap();
}
