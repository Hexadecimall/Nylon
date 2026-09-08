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
int nylon_project_save(const void* project, const char* bundle_directory);
int nylon_project_open(void* project, const char* bundle_directory);

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

#ifdef __cplusplus
}
#endif

#endif /* NYLON_H */
