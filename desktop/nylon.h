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

#ifdef __cplusplus
}
#endif

#endif /* NYLON_H */
