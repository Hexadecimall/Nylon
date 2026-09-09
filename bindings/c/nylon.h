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

typedef struct NylonTempoChange {
    double beat;
    double tempo;
} NylonTempoChange;

typedef struct NylonRecordingReport {
    unsigned long long frames;
    unsigned int sample_rate;
    double length_beats;
    unsigned long long lost_blocks;
    unsigned long long lost_frames;
} NylonRecordingReport;

/* Track device kinds: 0 utility, 1 equalizer, 2 compressor, 3 stereo delay,
 * 4 limiter, 5 saturator, 6 gate, 7 chorus, 8 reverb, 9 auto filter,
 * 10 phaser.
 * Parameter layouts:
 * utility: gain dB, width, balance
 * equalizer: filter kind 0..7, frequency, Q, gain dB
 * compressor: threshold, ratio, knee, attack, release, makeup, sidechain flag
 * stereo delay: delay seconds, feedback, mix
 * limiter: ceiling dB, release seconds, lookahead seconds
 * saturator: drive dB, output dB, mix, curve 0..3, oversampling 0..2,
 * DC filter flag
 * gate: threshold dB, hysteresis dB, attack seconds, hold seconds,
 * release seconds, sidechain flag
 * chorus: rate Hz, center seconds, depth seconds, feedback, mix, stereo phase
 * reverb: size, decay seconds, damping, diffusion, pre-delay seconds, width, mix
 * auto filter: mode 0..3, cutoff Hz, resonance, drive dB, envelope octaves,
 * envelope attack, envelope release, LFO rate, LFO octaves, mix, sidechain flag
 * phaser: rate Hz, center Hz, depth octaves, feedback, mix, stereo phase, stages */
typedef struct NylonTrackDevice {
    int kind;
    int enabled;
    float parameters[16];
} NylonTrackDevice;

typedef struct NylonClapParameterEvent {
    unsigned int sample_offset;
    unsigned int identifier;
    double value;
} NylonClapParameterEvent;

/* Note event kinds: 0 on, 1 off, 2 choke. */
typedef struct NylonClapNoteEvent {
    unsigned int sample_offset;
    unsigned int kind;
    int note_id;
    short port_index;
    short channel;
    short key;
    double velocity;
} NylonClapNoteEvent;

typedef struct NylonClapParameterInfo {
    unsigned int identifier;
    unsigned int flags;
    char name[256];
    char module[1024];
    double minimum;
    double maximum;
    double default_value;
} NylonClapParameterInfo;

/* Track automation parameters: 0 volume, 1 pan, 2 mute, 3 solo.
 * Curves: 0 step, 1 linear, 2 smooth. */
typedef struct NylonAutomationPoint {
    double beat;
    float value;
    int curve;
} NylonAutomationPoint;

/* Subtractive instrument oscillator shapes: 0 sine, 1 saw, 2 square,
 * 3 triangle. Levels and mix values use 0..1. */
typedef struct NylonInstrumentPatch {
    int shape_a;
    int shape_b;
    float oscillator_mix;
    float oscillator_b_detune_cents;
    float sub_level;
    float noise_level;
    unsigned int unison_voices;
    float unison_detune_cents;
    float attack_seconds;
    float decay_seconds;
    float sustain;
    float release_seconds;
    float cutoff_hz;
    float resonance;
    float level_db;
} NylonInstrumentPatch;

/* Allocates a new, empty project. Returns null on allocation failure. */
void* nylon_project_new(void);

/* Releases a project created by nylon_project_new. Null is ignored. */
void nylon_project_free(void* project);

/* Current tempo in beats per minute. Returns 0.0 for a null handle. */
double nylon_project_tempo(const void* project);

/* Sets the tempo. Rejects non-finite values and values outside the
 * supported range without changing the project. */
int nylon_project_set_tempo(void* project, double bpm);

/* Arrangement tempo changes after beat zero, sorted by beat. */
unsigned long long nylon_project_tempo_change_count(const void* project);
int nylon_project_tempo_change(
    const void* project, unsigned long long index, NylonTempoChange* output);
int nylon_project_set_tempo_at(void* project, double beat, double bpm);
int nylon_project_remove_tempo_change(void* project, double beat);

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
int nylon_track_instrument_get(
    const void* project, unsigned long long track, NylonInstrumentPatch* out);
int nylon_track_instrument_set(
    void* project, unsigned long long track, const NylonInstrumentPatch* patch);

unsigned long long nylon_track_device_count(
    const void* project, unsigned long long track);
int nylon_track_device_type(const void* project, unsigned long long track,
    unsigned long long device);
int nylon_track_device_enabled(const void* project, unsigned long long track,
    unsigned long long device);
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
int nylon_track_device_set_enabled(void* project, unsigned long long track,
    unsigned long long device, int enabled);
int nylon_track_plugin_format(const void* project, unsigned long long track,
    unsigned long long device);
unsigned int nylon_track_plugin_latency(const void* project, unsigned long long track,
    unsigned long long device);
unsigned long long nylon_track_plugin_package(const void* project,
    unsigned long long track, unsigned long long device, char* buffer,
    unsigned long long capacity);
unsigned long long nylon_track_plugin_identifier(const void* project,
    unsigned long long track, unsigned long long device, char* buffer,
    unsigned long long capacity);
unsigned long long nylon_track_plugin_state(const void* project,
    unsigned long long track, unsigned long long device, unsigned char* buffer,
    unsigned long long capacity);
unsigned long long nylon_track_plugin_parameter_count(const void* project,
    unsigned long long track, unsigned long long device);
int nylon_track_plugin_parameter_get(const void* project, unsigned long long track,
    unsigned long long device, unsigned long long index, unsigned int* identifier,
    double* value);
int nylon_track_plugin_add(void* project, unsigned long long track, int format,
    const char* package, const char* identifier, unsigned int latency_frames,
    const unsigned char* state, unsigned long long state_length, int enabled);
int nylon_track_plugin_set_state(void* project, unsigned long long track,
    unsigned long long device, const unsigned char* state,
    unsigned long long state_length);
int nylon_track_plugin_parameter_set(void* project, unsigned long long track,
    unsigned long long device, unsigned int identifier, double value);

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
int nylon_clip_notes_quantize(void* project, unsigned long long track, unsigned long long scene,
    double grid_beats, double strength);
int nylon_clip_notes_transpose(
    void* project, unsigned long long track, unsigned long long scene, int semitones);
int nylon_clip_notes_set_velocity(void* project, unsigned long long track,
    unsigned long long scene, unsigned char velocity);
int nylon_clip_notes_humanize(void* project, unsigned long long track, unsigned long long scene,
    double timing_beats, unsigned char velocity_range, unsigned long long seed);

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

/* Plugin discovery identifies packages without loading executable code.
 * Formats: 0 VST3, 1 Audio Unit, 2 CLAP, 3 LV2.
 * States: 0 discovered, 1 quarantined. Invalid indices return -1 for enum
 * accessors and empty text for string accessors. */
void* nylon_plugin_catalog_scan(const char* const* roots, unsigned long long root_count);
void nylon_plugin_catalog_free(void* catalog);
unsigned long long nylon_plugin_catalog_entry_count(const void* catalog);
unsigned long long nylon_plugin_catalog_issue_count(const void* catalog);
int nylon_plugin_catalog_entry_format(const void* catalog, unsigned long long index);
int nylon_plugin_catalog_entry_state(const void* catalog, unsigned long long index);
unsigned long long nylon_plugin_catalog_entry_path(const void* catalog,
    unsigned long long index, char* buffer, unsigned long long capacity);
unsigned long long nylon_plugin_catalog_entry_name(const void* catalog,
    unsigned long long index, char* buffer, unsigned long long capacity);
unsigned long long nylon_plugin_catalog_entry_reason(const void* catalog,
    unsigned long long index, char* buffer, unsigned long long capacity);
unsigned long long nylon_plugin_catalog_issue_path(const void* catalog,
    unsigned long long index, char* buffer, unsigned long long capacity);
unsigned long long nylon_plugin_catalog_issue_message(const void* catalog,
    unsigned long long index, char* buffer, unsigned long long capacity);
int nylon_plugin_catalog_quarantine(
    void* catalog, unsigned long long index, const char* reason);
int nylon_plugin_catalog_retry(void* catalog, unsigned long long index);
int nylon_plugin_catalog_apply_probe(void* catalog, unsigned long long index,
    const unsigned char* bytes, unsigned long long length);
unsigned long long nylon_plugin_catalog_descriptor_count(
    const void* catalog, unsigned long long entry);
unsigned long long nylon_plugin_catalog_descriptor_id(const void* catalog,
    unsigned long long entry, unsigned long long descriptor,
    char* buffer, unsigned long long capacity);
unsigned long long nylon_plugin_catalog_descriptor_name(const void* catalog,
    unsigned long long entry, unsigned long long descriptor,
    char* buffer, unsigned long long capacity);
unsigned long long nylon_plugin_catalog_descriptor_vendor(const void* catalog,
    unsigned long long entry, unsigned long long descriptor,
    char* buffer, unsigned long long capacity);
unsigned long long nylon_plugin_catalog_descriptor_version(const void* catalog,
    unsigned long long entry, unsigned long long descriptor,
    char* buffer, unsigned long long capacity);
unsigned long long nylon_plugin_catalog_descriptor_feature_count(const void* catalog,
    unsigned long long entry, unsigned long long descriptor);
unsigned long long nylon_plugin_catalog_descriptor_feature(const void* catalog,
    unsigned long long entry, unsigned long long descriptor,
    unsigned long long feature, char* buffer, unsigned long long capacity);

/* CLAP instances provide the processing primitive used by isolated plugin
 * workers. Input pointers may both be null for instruments. Non-null audio
 * regions contain `frames` values and may not overlap. Request bits are:
 * restart=1, process=2, callback=4, parameter rescan=8, parameter clear=16,
 * parameter flush=32, latency change=64, state dirty=128,
 * note port change=256. */
void* nylon_clap_instance_open(const char* path, const char* identifier);
void nylon_clap_instance_free(void* instance);
int nylon_clap_instance_activate(void* instance, double sample_rate,
    unsigned int min_frames, unsigned int max_frames);
int nylon_clap_instance_process_stereo(void* instance,
    const float* input_left, const float* input_right,
    float* output_left, float* output_right, unsigned int frames);
int nylon_clap_instance_process_stereo_events(void* instance,
    const float* input_left, const float* input_right,
    float* output_left, float* output_right, unsigned int frames,
    const NylonClapParameterEvent* events, unsigned int event_count);
int nylon_clap_instance_process_stereo_all_events(void* instance,
    const float* input_left, const float* input_right,
    float* output_left, float* output_right, unsigned int frames,
    const NylonClapParameterEvent* parameter_events,
    unsigned int parameter_event_count,
    const NylonClapNoteEvent* note_events, unsigned int note_event_count);
unsigned int nylon_clap_instance_input_note_ports(const void* instance);
unsigned int nylon_clap_instance_input_audio_ports(const void* instance);
unsigned long long nylon_clap_instance_parameter_count(const void* instance);
int nylon_clap_instance_parameter_info(const void* instance,
    unsigned long long index, NylonClapParameterInfo* info);
int nylon_clap_instance_parameter_value(const void* instance,
    unsigned int identifier, double* value);
int nylon_clap_instance_latency(const void* instance, unsigned int* frames);
void* nylon_clap_instance_save_state(const void* instance);
int nylon_clap_instance_load_state(void* instance,
    const unsigned char* bytes, unsigned long long length);
void nylon_clap_state_free(void* state);
unsigned long long nylon_clap_state_size(const void* state);
const unsigned char* nylon_clap_state_data(const void* state);
int nylon_clap_instance_reset(void* instance);
unsigned int nylon_clap_instance_take_requests(const void* instance);

/* Isolated CLAP workers keep plugin code outside the calling process. These
 * functions perform blocking pipe I/O and belong on a dedicated IPC thread. */
void* nylon_clap_worker_open(const char* executable, const char* path,
    const char* identifier, double sample_rate, unsigned int max_frames);
void nylon_clap_worker_free(void* worker);
int nylon_clap_worker_process_stereo(void* worker,
    const float* input_left, const float* input_right,
    float* output_left, float* output_right, unsigned int frames,
    const NylonClapParameterEvent* parameter_events,
    unsigned int parameter_event_count,
    const NylonClapNoteEvent* note_events, unsigned int note_event_count);
unsigned int nylon_clap_worker_input_note_ports(const void* worker);
unsigned int nylon_clap_worker_input_audio_ports(const void* worker);
unsigned long long nylon_clap_worker_parameter_count(const void* worker);
int nylon_clap_worker_parameter_info(const void* worker,
    unsigned long long index, NylonClapParameterInfo* info);
int nylon_clap_worker_latency(const void* worker, unsigned int* frames);
void* nylon_clap_worker_save_state(void* worker);
int nylon_clap_worker_load_state(void* worker,
    const unsigned char* bytes, unsigned long long length);

/* The bridge adds one audio block of latency and keeps pipe I/O off the audio
 * callback. Late or failed processing returns a one-block delayed dry signal. */
void* nylon_clap_bridge_open(const char* executable, const char* path,
    const char* identifier, double sample_rate, unsigned int frames,
    unsigned int queue_depth);
void nylon_clap_bridge_free(void* bridge);
int nylon_clap_bridge_process_stereo(void* bridge,
    const float* input_left, const float* input_right,
    float* output_left, float* output_right, unsigned int frames,
    const NylonClapParameterEvent* parameter_events,
    unsigned int parameter_event_count,
    const NylonClapNoteEvent* note_events, unsigned int note_event_count);
unsigned int nylon_clap_bridge_latency(const void* bridge);
int nylon_clap_bridge_is_running(const void* bridge);
unsigned long long nylon_clap_bridge_submitted_blocks(const void* bridge);
unsigned long long nylon_clap_bridge_completed_blocks(const void* bridge);
unsigned long long nylon_clap_bridge_underruns(const void* bridge);
unsigned long long nylon_clap_bridge_queue_drops(const void* bridge);
unsigned long long nylon_clap_bridge_worker_failures(const void* bridge);

/* Live audio is owned by a separate control-thread handle. The platform
 * stream remains open while the musical transport is stopped, allowing
 * meters and edits to continue crossing block boundaries. */
void* nylon_audio_new(void);
void nylon_audio_free(void* audio);
int nylon_audio_configure_plugin_host(void* audio, const char* worker,
    const char* const* roots, unsigned long long root_count);

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
unsigned long long nylon_audio_frames_rendered(const void* audio);

/* Session launches replace Arrangement playback on affected tracks. A zero
 * quantization starts at the earliest callback boundary. */
int nylon_session_launch_clip(void* audio, const void* project,
    unsigned long long track, unsigned long long scene, double quantization_beats);
int nylon_session_launch_scene(void* audio, const void* project,
    unsigned long long scene, double quantization_beats);
int nylon_session_stop_track(
    void* audio, const void* project, unsigned long long track);
long long nylon_session_active_scene(const void* audio, unsigned long long track);

/* Live MIDI commands enter a bounded queue and take effect at the next audio
 * callback boundary. They work while the transport is stopped. */
int nylon_live_note_on(void* audio, const void* project,
    unsigned long long track, unsigned char pitch, unsigned char velocity);
int nylon_live_note_off(void* audio, const void* project,
    unsigned long long track, unsigned char pitch);
int nylon_live_all_notes_off(
    void* audio, const void* project, unsigned long long track);
int nylon_live_all_notes_off_all(void* audio, const void* project);

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
