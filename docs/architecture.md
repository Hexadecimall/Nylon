# Architecture

## Desktop boundary

The desktop frontend uses Qt. Rust owns the engine, transport, DSP, project state,
and plugin hosting. Qt owns windows, input, layout, and presentation. A narrow
native interface carries edits to the control-thread command bus and returns
immutable display state. The audio callback does not call Qt or enter its event
loop. GUI code must not retain pointers into mutable render storage.

## Render kernel

The render kernel operates on borrowed stereo frame slices. Callback storage is
allocated by the caller before playback. The maximum block contains 2,048 frames
and 256 gain changes. The kernel performs no allocation, synchronization, logging,
or operating system calls.

An event offset identifies the first frame affected by its gain. Events at equal
offsets execute in input order. An event at the block end changes the next block.
Validation precedes output writes and state changes. Invalid requests preserve
output, gain, and transport position. Stopping the sample clock preserves live
input processing.

The allocation test counts allocation, reallocation, and deallocation operations
on the calling thread. This is a runtime check of exercised paths, not a static
proof. Render partition tests compare output across several block sizes.
`scripts/rt_lint.py` rejects known allocator, lock, I/O, foreign-call, and cloning
syntax in the render kernel. It is a conservative text check, not a transitive
analysis of library calls; the allocation harness remains mandatory.

The state exchange publishes prepared storage through a single pending slot.
The callback moves replaced storage to a reclamation queue. Only the control
thread destroys retired storage. If reclamation stalls, the callback retains
its current state and defers the next publication. Endpoints must be destroyed
on the control thread after the callback stops.

The render benchmark reports nanoseconds per 256-frame stereo block after warmup.
A statistically stable regression gate requires a dedicated runner and a stored
baseline; the current shared-runner job reports measurements only.

The project model publishes immutable snapshots through grouped commands. Undo
and redo retain prior snapshots in memory. Track identifiers remain unique even
after branching from an earlier undo state. Failed command groups publish nothing.

Qt integration, device backends, routing, plugin isolation, and project persistence
are not implemented yet. These require separate integration checks
before playback can be described as production-ready.
