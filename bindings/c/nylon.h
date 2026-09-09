/* C interface to the Nylon core library.
 *
 * Every function is safe to call from the GUI thread only. A project handle
 * is created by nylon_project_new and released by nylon_project_free. All
 * functions accept a null handle and treat it as a failed call.
 *
 * Integer results: 1 on success, 0 on failure.
 */
#ifndef NYLON_H
#define NYLON_H

#ifdef __cplusplus
extern "C" {
#endif

#define NYLON_AUDIO_MAX_NAME 128
#define NYLON_AUDIO_MAX_RATES 8

typedef struct NylonAudioDevice {
    unsigned long long id;
    char name[NYLON_AUDIO_MAX_NAME + 1];
    unsigned int channels;
    int is_default;
    unsigned int sample_rates[NYLON_AUDIO_MAX_RATES];
    unsigned int sample_rate_count;
} NylonAudioDevice;

typedef struct NylonAudioConfig {
    unsigned long long device_id;
    unsigned int sample_rate;
    unsigned int block_frames;
    unsigned int channels;
} NylonAudioConfig;

typedef struct NylonLevels {
    float peak_left;
    float peak_right;
    float rms_left;
    float rms_right;
    int clipped;
} NylonLevels;

typedef struct NylonBounceReport {
    unsigned long long frames;
    float peak_left;
    float peak_right;
} NylonBounceReport;

typedef struct NylonRecordingReport {
    unsigned long long frames;
    unsigned int sample_rate;
    double length_beats;
    unsigned long long lost_blocks;
    unsigned long long lost_frames;
} NylonRecordingReport;

/* Track device kinds: 0 utility, 1 equalizer, 2 compressor, 3 stereo delay,
 * 4 limiter, 5 saturator, 6 gate.
 * Parameter layouts:
 * utility: gain dB, width, balance
 * equalizer: filter kind 0..7, frequency, Q, gain dB
 * compressor: threshold, ratio, knee, attack, release, makeup, sidechain flag
 * stereo delay: delay seconds, feedback, mix
 * limiter: ceiling dB, release seconds, lookahead seconds
 * saturator: drive dB, output dB, mix, curve 0..3, oversampling 0..2,
 * DC filter flag
 * gate: threshold dB, hysteresis dB, attack seconds, hold seconds,
 * release seconds, sidechain flag */
typedef struct NylonTrackDevice {
    int kind;
    int enabled;
    float parameters[7];
} NylonTrackDevice;

/* Track automation parameters: 0 volume, 1 pan, 2 mute, 3 solo.
 * Curves: 0 step, 1 linear, 2 smooth. */
typedef struct NylonAutomationPoint {
    double beat;
    float value;
    int curve;
} NylonAutomationPoint;

/* Allocates a new, empty project. Returns null on allocation failure. */
void* nylon_project_new(void);

/* Releases a project created by nylon_project_new. Null is ignored. */
void nylon_project_free(void* project);

/* Current tempo in beats per minute. Returns 0.0 for a null handle. */
double nylon_project_tempo(const void* project);

/* Sets the tempo. Rejects non-finite values and values outside the
 * supported range without changing the project. */
int nylon_project_set_tempo(void* project, double bpm);

/* Appends a new track. */
int nylon_project_add_track(void* project);

/* Number of tracks in the current snapshot. Returns 0 for a null handle. */
unsigned long long nylon_project_track_count(const void* project);

/* Reverts the most recent command group. Returns 0 when there is nothing
 * to undo. */
int nylon_project_undo(void* project);

/* Re-applies the most recently undone command group. Returns 0 when there
 * is nothing to redo. */
int nylon_project_redo(void* project);

/* Whether undo or redo would succeed right now. */
int nylon_project_can_undo(const void* project);
int nylon_project_can_redo(const void* project);

/* Replaces the project's contents with a new empty project, discarding
 * its history. */
int nylon_project_new_in_place(void* project);

/* Writes the project as a bundle directory at the UTF-8 path, creating it
 * as needed. Reading replaces the project's contents and history with the
 * bundle's. Both return 0 on any I/O or format error. */
int nylon_project_save(void* project, const char* bundle_directory);
int nylon_project_open(void* project, const char* bundle_directory);
int nylon_project_is_modified(const void* project);

/* Writes modified state to an atomic recovery sidecar. A saved project is
 * required. Recovery stays separate from the primary document until selected. */
int nylon_project_autosave(const void* project);
int nylon_project_recovery_available(const char* bundle_directory);
int nylon_project_recover(void* project, const char* bundle_directory);
int nylon_project_discard_recovery(const char* bundle_directory);

/* Fixed-capacity routing graph. Edge kinds: 0 main, 1 pre-fader send,
 * 2 post-fader send, 3 sidechain. Compilation rejects cycles and calculates
 * a processing order plus plugin delay compensation for each edge. */
void* nylon_routing_new(unsigned int node_count);
void nylon_routing_free(void* routing);
int nylon_routing_set_node_latency(void* routing, unsigned int node, unsigned int frames);
int nylon_routing_add_edge(void* routing, unsigned int source, unsigned int destination,
    int kind, float gain, unsigned int* out_index);
void* nylon_routing_compile(const void* routing);
void nylon_compiled_routing_free(void* routing);
unsigned int nylon_compiled_routing_node_count(const void* routing);
int nylon_compiled_routing_order_at(const void* routing, unsigned int index);
int nylon_compiled_routing_edge_delay(
    const void* routing, unsigned int index, unsigned int* out_frames);
int nylon_compiled_routing_output_latency(
    const void* routing, unsigned int node, unsigned int* out_frames);

/* Renders a beat range to a stereo 24-bit WAVE file at the selected sample
 * rate. The report is written only after the file is complete. */
int nylon_render_bounce_wave(const void* project, const char* path, double start_beats,
    double end_beats, unsigned int sample_rate, NylonBounceReport* out);

/* Track kinds: 0 audio, 1 MIDI, 2 return, 3 master, 4 group, 5 cue. */
int nylon_project_add_track_kind(void* project, int kind);

/* Time signature and sample rate of the current snapshot. Getters return
 * 0 for a null handle. */
int nylon_project_time_signature_numerator(const void* project);
int nylon_project_time_signature_denominator(const void* project);
int nylon_project_set_time_signature(void* project, int numerator, int denominator);
unsigned int nylon_project_sample_rate(const void* project);
int nylon_project_set_sample_rate(void* project, unsigned int rate);

/* Track accessors take a zero-based index into the current snapshot. Out of
 * range indices read as the documented fallback and reject writes.
 *
 * nylon_track_name copies up to capacity-1 bytes of UTF-8 into buffer and
 * NUL-terminates when capacity > 0; it returns the full length in bytes
 * excluding the terminator, so a larger buffer can be retried. */
unsigned long long nylon_track_name(const void* project, unsigned long long index, char* buffer,
    unsigned long long capacity);
int nylon_track_set_name(void* project, unsigned long long index, const char* utf8);
int nylon_track_kind(const void* project, unsigned long long index);
int nylon_track_delete(void* project, unsigned long long index);

/* Volume in decibels; negative infinity is silence. Fallback: -inf. */
double nylon_track_volume_db(const void* project, unsigned long long index);
int nylon_track_set_volume_db(void* project, unsigned long long index, double db);
/* Pan from -1 (left) to +1 (right). Fallback: 0. */
double nylon_track_pan(const void* project, unsigned long long index);
int nylon_track_set_pan(void* project, unsigned long long index, double pan);
/* Flags are 0 or 1. Setters reject other values. */
int nylon_track_mute(const void* project, unsigned long long index);
int nylon_track_set_mute(void* project, unsigned long long index, int enabled);
int nylon_track_solo(const void* project, unsigned long long index);
int nylon_track_set_solo(void* project, unsigned long long index, int enabled);
int nylon_track_arm(const void* project, unsigned long long index);
int nylon_track_set_arm(void* project, unsigned long long index, int enabled);
/* Palette index 0..15. Fallback: -1. */
int nylon_track_color_index(const void* project, unsigned long long index);
int nylon_track_set_color_index(void* project, unsigned long long index, int color);
unsigned int nylon_track_latency_frames(const void* project, unsigned long long index);
int nylon_track_set_latency_frames(
    void* project, unsigned long long index, unsigned int frames);

unsigned long long nylon_track_device_count(
    const void* project, unsigned long long track);
int nylon_track_device_get(const void* project, unsigned long long track,
    unsigned long long device, NylonTrackDevice* out);
int nylon_track_device_add(
    void* project, unsigned long long track, const NylonTrackDevice* device);
int nylon_track_device_set(void* project, unsigned long long track,
    unsigned long long index, const NylonTrackDevice* device);
int nylon_track_device_delete(
    void* project, unsigned long long track, unsigned long long index);
int nylon_track_device_move(void* project, unsigned long long track,
    unsigned long long from, unsigned long long to);

unsigned long long nylon_track_automation_count(
    const void* project, unsigned long long track, int parameter);
int nylon_track_automation_get(const void* project, unsigned long long track,
    int parameter, unsigned long long index, NylonAutomationPoint* out);
int nylon_track_automation_set(void* project, unsigned long long track, int parameter,
    const NylonAutomationPoint* points, unsigned long long count);
int nylon_track_automation_clear(
    void* project, unsigned long long track, int parameter);

/* Project routes use track indexes at the interface and stable track IDs in
 * persisted state. Invalid route indexes return ULLONG_MAX, -1, or NaN. */
unsigned long long nylon_project_route_count(const void* project);
unsigned long long nylon_project_route_source(
    const void* project, unsigned long long index);
unsigned long long nylon_project_route_destination(
    const void* project, unsigned long long index);
int nylon_project_route_kind(const void* project, unsigned long long index);
float nylon_project_route_gain(const void* project, unsigned long long index);
int nylon_project_route_add(void* project, unsigned long long source,
    unsigned long long destination, int kind, float gain);
int nylon_project_route_delete(void* project, unsigned long long index);

/* Session scenes. Names use the same UTF-8 buffer convention as track names. */
unsigned long long nylon_scene_count(const void* project);
int nylon_scene_create(void* project, const char* utf8);
int nylon_scene_delete(void* project, unsigned long long scene);
unsigned long long nylon_scene_name(const void* project, unsigned long long scene, char* buffer,
    unsigned long long capacity);
int nylon_scene_set_name(void* project, unsigned long long scene, const char* utf8);

/* Session clip slots are addressed by zero-based track and scene indices.
 * State is 0 for empty or invalid, 1 for MIDI, and 2 for audio. */
int nylon_clip_slot_state(const void* project, unsigned long long track, unsigned long long scene);
int nylon_clip_create_midi(
    void* project, unsigned long long track, unsigned long long scene, double length_beats);
/* Imports a WAVE file into the current project bundle and creates an audio
 * clip. The project must have been saved or opened first. */
int nylon_clip_import_wave(void* project, const char* source_path, unsigned long long track,
    unsigned long long scene, double source_tempo);
int nylon_clip_delete(void* project, unsigned long long track, unsigned long long scene);
unsigned long long nylon_clip_name(const void* project, unsigned long long track,
    unsigned long long scene, char* buffer, unsigned long long capacity);
int nylon_clip_set_name(
    void* project, unsigned long long track, unsigned long long scene, const char* utf8);
int nylon_clip_color_index(
    const void* project, unsigned long long track, unsigned long long scene);
int nylon_clip_set_color_index(
    void* project, unsigned long long track, unsigned long long scene, int color);
double nylon_clip_loop_start(
    const void* project, unsigned long long track, unsigned long long scene);
double nylon_clip_loop_length(
    const void* project, unsigned long long track, unsigned long long scene);
int nylon_clip_set_loop(void* project, unsigned long long track, unsigned long long scene,
    double start_beats, double length_beats);

/* Audio clip settings. Media paths are relative to the project bundle and
 * use the same UTF-8 buffer convention as track names. */
unsigned long long nylon_clip_media_path(const void* project, unsigned long long track,
    unsigned long long scene, char* buffer, unsigned long long capacity);
double nylon_clip_audio_gain_db(
    const void* project, unsigned long long track, unsigned long long scene);
int nylon_clip_set_audio_gain_db(
    void* project, unsigned long long track, unsigned long long scene, double db);
int nylon_clip_audio_reverse(
    const void* project, unsigned long long track, unsigned long long scene);
int nylon_clip_set_audio_reverse(
    void* project, unsigned long long track, unsigned long long scene, int enabled);
int nylon_clip_audio_warp(
    const void* project, unsigned long long track, unsigned long long scene);
double nylon_clip_audio_source_tempo(
    const void* project, unsigned long long track, unsigned long long scene);
int nylon_clip_set_audio_warp(void* project, unsigned long long track, unsigned long long scene,
    int enabled, double source_tempo);

/* MIDI notes use pitches 0..127, velocities 1..127, and finite beat values.
 * note_at writes all four outputs only when it succeeds. */
unsigned long long nylon_clip_note_count(
    const void* project, unsigned long long track, unsigned long long scene);
int nylon_clip_note_at(const void* project, unsigned long long track, unsigned long long scene,
    unsigned long long index, unsigned char* pitch, unsigned char* velocity, double* start_beats,
    double* length_beats);
int nylon_clip_note_add(void* project, unsigned long long track, unsigned long long scene,
    unsigned char pitch, unsigned char velocity, double start_beats, double length_beats);
int nylon_clip_note_remove(void* project, unsigned long long track, unsigned long long scene,
    unsigned long long index);
int nylon_clip_note_move(void* project, unsigned long long track, unsigned long long scene,
    unsigned long long index, unsigned char pitch, unsigned char velocity, double start_beats,
    double length_beats);

/* Arrangement placements reference a session clip and have an independent
 * start and length. */
unsigned long long nylon_arrangement_clip_count(const void* project, unsigned long long track);
int nylon_arrangement_clip_add_from_slot(void* project, unsigned long long track,
    unsigned long long scene, double start_beats, double length_beats);
int nylon_arrangement_clip_range(const void* project, unsigned long long track,
    unsigned long long index, double* start_beats, double* length_beats);
unsigned long long nylon_arrangement_clip_name(const void* project, unsigned long long track,
    unsigned long long index, char* buffer, unsigned long long capacity);
int nylon_arrangement_clip_color_index(
    const void* project, unsigned long long track, unsigned long long index);
int nylon_arrangement_clip_remove(
    void* project, unsigned long long track, unsigned long long index);
int nylon_arrangement_clip_set_range(void* project, unsigned long long track,
    unsigned long long index, double start_beats, double length_beats);

/* Live audio is owned by a separate control-thread handle. The platform
 * stream remains open while the musical transport is stopped, allowing
 * meters and edits to continue crossing block boundaries. */
void* nylon_audio_new(void);
void nylon_audio_free(void* audio);

/* Returns the number of output devices found. Passing a null output pointer
 * queries the count. A non-null output receives up to capacity records. */
unsigned long long nylon_audio_device_list(NylonAudioDevice* out, unsigned long long capacity);
unsigned long long nylon_audio_input_device_list(
    NylonAudioDevice* out, unsigned long long capacity);
int nylon_audio_default_output(unsigned long long* device_id);
int nylon_audio_default_input(unsigned long long* device_id);

/* A recording reserves an empty audio clip slot in a saved project, opens
 * stopped, and writes through a bounded queue. Finishing creates one undoable
 * clip edit. Freeing before finish removes the reserved media file. */
void* nylon_recording_open(const void* project, unsigned long long track,
    unsigned long long scene, unsigned long long device_id,
    unsigned int sample_rate, unsigned int block_frames);
void nylon_recording_free(void* recording);
int nylon_recording_start(void* recording);
int nylon_recording_stop(void* recording);
int nylon_recording_is_running(const void* recording);
int nylon_recording_finish(
    void* recording, void* project, NylonRecordingReport* out);

/* Opens and starts an output callback with the musical transport stopped.
 * Device zero selects the current system default where the backend supports
 * it. Project state is copied before the first callback. */
int nylon_audio_open(void* audio, const void* project, unsigned long long device_id,
    unsigned int sample_rate, unsigned int block_frames);
int nylon_audio_close(void* audio);
int nylon_audio_is_open(const void* audio);
int nylon_audio_config(const void* audio, NylonAudioConfig* out);
int nylon_audio_sync(void* audio, const void* project);
unsigned long long nylon_audio_dropouts(const void* audio);

/* Session launches replace Arrangement playback on affected tracks. A zero
 * quantization starts at the earliest callback boundary. */
int nylon_session_launch_clip(void* audio, const void* project,
    unsigned long long track, unsigned long long scene, double quantization_beats);
int nylon_session_launch_scene(void* audio, const void* project,
    unsigned long long scene, double quantization_beats);
int nylon_session_stop_track(
    void* audio, const void* project, unsigned long long track);
long long nylon_session_active_scene(const void* audio, unsigned long long track);

int nylon_transport_play(void* audio);
int nylon_transport_stop(void* audio);
int nylon_transport_locate(void* audio, double beats);
double nylon_transport_position_beats(void* audio);
int nylon_transport_is_playing(void* audio);

/* Levels are linear amplitudes. The master follows the active track entries
 * in the engine state but has a dedicated accessor here. */
int nylon_track_levels(void* audio, unsigned long long index, NylonLevels* out);
int nylon_master_levels(void* audio, NylonLevels* out);

#ifdef __cplusplus
}
#endif

#endif /* NYLON_H */
