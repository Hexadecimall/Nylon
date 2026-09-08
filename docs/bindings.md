# Bindings

The core library exposes one C interface; every other binding is built on
it. All functions are safe to call from a single control thread at a time.
Nothing here may be called from the audio thread.

## C

Header: `bindings/c/nylon.h`. Link against the core library
(`target/release/libnylon.a`, `libnylon.so`, or `nylon.lib`).

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
| `nylon_track_*` | name, kind, volume, pan, mute, solo, arm, color, deletion |
| `nylon_scene_*` | scene count, creation, deletion, names |
| `nylon_clip_*` | slot state, MIDI clip creation, name, color, loop, notes |
| `nylon_arrangement_*` | clips placed on the timeline |

Track kinds: 0 audio, 1 MIDI, 2 return, 3 master, 4 group, 5 cue.

## C++

Header: `bindings/cpp/nylon.hpp`, library `nyloncpp` (built by
`bindings/cpp/CMakeLists.txt`; link it with `-lnyloncpp` plus the core).
`nylon::Project` is a move-only owner of a project handle with one method
per C function, `std::string` for names, and `nylon::MidiNote` /
`nylon::BeatRange` value types. Consumers include the directory with
`add_subdirectory(bindings/cpp)` and link the `nyloncpp` target, which
carries the core library and the C header directory.

`bindings/cmake/NylonCore.cmake` defines the imported `nylon_core` target;
`NYLON_CORE_LIBRARY` selects a static or shared core.

## Rust

The crate itself is the Rust binding: `nylon::project::Project` and the
`Command` enum under `src/` are what the C functions call.
