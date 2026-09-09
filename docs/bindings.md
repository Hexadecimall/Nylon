# Bindings

The core library exposes one C interface; every other binding is built on
it. All functions are safe to call from a single control thread at a time.
Nothing here may be called from the audio thread.

## C

Header: `bindings/c/nylon.h`. Link against the core library
(`libnylon.dylib`, `libnylon.so`, or `nylon.dll`). Static archives are
also built for embedding.

Conventions:

- A project is an opaque `void*` from `nylon_project_new`, released with
  `nylon_project_free`. Every function accepts a null handle and treats it
  as a failed call.
- Functions that change state return `int`: 1 on success, 0 on rejection.
  A rejected call leaves the project unchanged. Accepted calls are one undo
  step each.
- Getters return the documented fallback for a null handle or an index out
  of range (0, 0.0, -1, or negative infinity for volume).
- Strings are UTF-8. Name getters take a buffer and its capacity, copy up to
  `capacity - 1` bytes, NUL-terminate when `capacity > 0`, and return the
  full length in bytes so a larger buffer can be retried.
- Indices are zero-based `unsigned long long` positions into the current
  snapshot: tracks, scenes, notes, and arrangement clips.
- Musical positions and lengths are `double` beats.

Groups of functions:

| Prefix | Covers |
| --- | --- |
| `nylon_project_*` | tempo, time signature, sample rate, undo/redo, bundle save/open, reset |
| `nylon_track_*` | name, kind, volume, pan, mute, solo, arm, color, devices, deletion |
| `nylon_scene_*` | scene count, creation, deletion, names |
| `nylon_clip_*` | slot state, MIDI creation, WAVE import, clip settings, notes |
| `nylon_arrangement_*` | clips placed on the timeline |
| `nylon_audio_*` | input/output devices, stream lifecycle, granted configuration, dropouts, project publication |
| `nylon_recording_*` | input capture, file finalization, and project clip registration |
| `nylon_transport_*` | play, stop, locate, position |
| `nylon_track_levels`, `nylon_master_levels` | live linear peak and RMS readings |
| `nylon_render_*` | deterministic offline file rendering |
| `nylon_clap_*` | CLAP lifecycle, parameter metadata, latency, and stereo processing |

Track kinds: 0 audio, 1 MIDI, 2 return, 3 master, 4 group, 5 cue.

## C++

Header: `bindings/cpp/nylon.hpp`, library `nyloncpp` (built by
`bindings/cpp/CMakeLists.txt`; link it with `-lnyloncpp` plus the core).
`nylon::Project` is a move-only owner of a project handle with one method
per project C function, `std::string` for names, and `nylon::MidiNote` /
`nylon::BeatRange` value types. `nylon::TrackDevice` carries a typed device,
its enabled state, and sixteen fixed parameter slots across the ABI.
Device kinds include utility, equalizer, compressor, delay, limiter, saturator,
gate, chorus, reverb, and auto filter.
`nylon::InstrumentPatch` configures both oscillators, sub and noise levels,
unison, amplitude envelope, filter, and output level.
`Project::bounceWave` writes a selected beat
range as stereo 24-bit WAVE and returns its frame count and peaks. Audio clip
imports copy decoded WAVE sources into the bundle's `Media/` directory before
the undoable clip edit is accepted.
`Project::isModified` reports edits since the last primary save.
`Project::autosave` atomically writes a recovery sidecar inside an opened or
saved bundle. `Project::recoveryAvailable`, `Project::recover`, and
`Project::discardRecovery` support startup recovery. Saving recovered state
replaces the primary document and removes the sidecar.
`nylon::AudioEngine` owns the platform
stream and provides device enumeration, transport control, synchronization,
configuration, dropout, and meter access. Consumers include the directory with
`add_subdirectory(bindings/cpp)` and link the `nyloncpp` target, which
carries the core library and the C header directory.
`nylon::Recording` is a move-only input handle. It reserves media in a saved
project, opens stopped, and creates one undoable audio clip when finished.
`nylon::ClapInstance` exposes plugin activation, parameter metadata and current
values, reported latency, and stereo processing. A caller can pass a bounded,
sample-ordered array of parameter and note events to processing without
allocating in the binding. Note events retain their identifier, port, channel,
key, velocity, and exact frame offset. Production playback places each instance in a separate worker
process so a plugin fault does not terminate the controlling process. State
save returns an owned opaque byte buffer in C and a byte vector in C++. Loading
restores the same opaque data with a 256 MiB upper bound.

`bindings/cmake/NylonCore.cmake` defines the imported `nylon_core` target;
`NYLON_CORE_LIBRARY` selects a static or shared core.

## Command line

`nylon-control COMMAND` operates directly on the library:

```text
nylon-control --project Session.nylon new
nylon-control --project Session.nylon info
nylon-control --project Session.nylon add-track midi Lead
nylon-control --project Session.nylon tracks
nylon-control --project Session.nylon set-instrument 0 saw square 0.6 -7 0.35 0.04 4 18 0.01 0.2 0.7 0.3 2400 1.2 -9
nylon-control --project Session.nylon instrument 0
nylon-control --project Session.nylon add-device 0 chorus 0.8 0.012 0.003 0.1 0.5 0.25
nylon-control --project Session.nylon add-device 0 reverb 0.6 2.8 0.35 0.7 0.02 1 0.3
nylon-control --project Session.nylon add-device 0 auto-filter band-pass 1600 2.5 8 -1.5 0.004 0.2 0.75 1.25 0.6 on
nylon-control --project Session.nylon create-midi-clip 0 0 4
nylon-control --project Session.nylon add-note 0 0 60 100 0.25 0.5
nylon-control --project Session.nylon quantize-notes 0 0 0.25 1
nylon-control --project Session.nylon transpose-notes 0 0 12
nylon-control --project Session.nylon set-note-velocity 0 0 96
nylon-control --project Session.nylon humanize-notes 0 0 0.02 4 42
nylon-control --project Session.nylon notes 0 0
nylon-control --project Session.nylon import-wave 1 0 take.wav 120
nylon-control --project Session.nylon clips 1
nylon-control --project Session.nylon place-clip 1 0 0 16
nylon-control --project Session.nylon set-audio-gain 1 0 -3
nylon-control --project Session.nylon add-device 0 utility -6 1 0
nylon-control --project Session.nylon add-device 0 equalizer peaking 1000 0.7 3
nylon-control --project Session.nylon add-device 0 compressor -18 4 6 0.01 0.1 0 off
nylon-control --project Session.nylon add-device 0 delay 0.25 0.4 0.3
nylon-control --project Session.nylon add-device 0 limiter -0.3 0.1 0.005
nylon-control --project Session.nylon add-device 0 saturator 9 -4 0.75 diode 4x on
nylon-control --project Session.nylon add-device 0 gate -32 8 0.002 0.04 0.15 on
nylon-control --project Session.nylon track-devices 0
nylon-control --project Session.nylon set-device-enabled 0 1 off
nylon-control --project Session.nylon set-automation 0 volume 0 -12 linear 4 0 smooth
nylon-control --project Session.nylon automation 0 volume
nylon-control --project Session.nylon clear-automation 0 volume
nylon-control --project Session.nylon move-device 0 1 0
nylon-control --project Session.nylon delete-device 0 1
nylon-control --project Session.nylon set-tempo 128
nylon-control --project Session.nylon bounce mix.wav 0 64 48000
nylon-control --project Session.nylon record 1 0 8.0
nylon-control --project Session.nylon record 1 0 8.0 DEVICE RATE BLOCK
nylon-control --project Session.nylon recovery-status
nylon-control --project Session.nylon recover
nylon-control devices
nylon-control input-devices
```

Every reply is one JSON object. Direct edit commands save the bundle only
after the core accepts the change. Recording duration is in seconds. Its
optional device, sample rate, and block size fields default to the system
input, project rate, and 256 frames.
Device-chain commands also accept `--endpoint` and edit the running engine.

## Rust

The crate itself is the Rust binding: `nylon::project::Project` and the
`Command` enum under `src/` are what the C functions call.
