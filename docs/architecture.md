# Architecture

## Front end boundary

The core owns the engine, transport, signal processing, project state, and
plugin hosting. A front end owns windows, input, layout, and presentation. A
narrow interface carries edits to the control-thread command bus and returns
immutable display state. The audio callback never calls into a front end or
its event loop, and front end code must not retain pointers into mutable
render storage.

A Qt front end was written against this boundary and is parked in
`failedAttempts/`. The boundary itself is unchanged: it is what any front end,
including a future one, links against.

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

## Signal processing

`src/dsp` holds the primitives every device is built from: decibel
conversion, panning laws, parameter smoothing, second-order sections,
band-limited oscillators, an envelope generator, a delay line, and level
metering. Dynamics and nonlinear stages include compression, lookahead
limiting, stereo-linked gating with sidechain detection, saturation with
selectable transfer curves and bounded oversampling, a stereo chorus with
phase-offset modulated delay lines, an algorithmic stereo reverb with
parallel damped combs and diffusion stages, and a resonant auto filter with
envelope, sidechain, drive, and low-frequency modulation. A stereo phaser runs
up to twelve modulated all-pass stages with feedback and channel phase offset.
Each processor is fixed-size. Storage that must outlive a call, such as a delay
buffer, is borrowed from the caller, which allocates it on the
control thread before playback.

Second-order sections carry an analytic magnitude response alongside the
filter itself, so an equalizer curve is drawn from the same coefficients
that process the audio rather than from a separate approximation.

## Mixing

`src/mixer` sums stereo track signals through per-track volume, pan, mute,
and solo into a master strip, metering every strip. Parameter changes
arrive as events carrying the frame they take effect on: an event applies
before the sample at its offset, equal offsets keep input order, and an
offset at the block end applies to the next block. The values themselves
are smoothed, so a change lands on the right sample without stepping the
signal. The whole request is validated before anything changes, so a
refused render leaves both the mixer and the output untouched.

The pan law preserves power across the sweep and is normalized so a
centered strip is unity gain, with a hard-panned strip 3 dB up on that
side.

## Musical time

`src/transport` converts between frames and beats. It advances by whole
blocks, so the same starting position and block length always cover the
same span of musical time. A loop wraps at its end carrying the overshoot.
A tempo or sample rate change keeps the musical position rather than the
frame count, so switching devices does not move the playhead in musical
terms.

## Playback

`src/engine/playback` owns the mixer and the transport and implements the
renderer interface, so one object drives both a device stream and an
offline bounce. It does not read the project model: the control thread
publishes a fixed-size settings snapshot that the engine takes up at a
block boundary, never mid-block.

State travelling the other way uses `src/latest`, a triple buffer. A queue
is the wrong shape for meters and a playhead, because a reader that falls
behind then keeps the oldest entries and drops the newest. The triple
buffer writer never waits and the reader always sees the most recently
completed value.

Instrument tracks sound their notes: the engine schedules each block's
notes onto the frames they were written for, renders every instrument
into its own buffer, and the mixer sums those through the strips. The
settings carry whether to play and where to move the playhead, since once
the engine is handed to a stream the transport cannot be reached
directly. Moving the playhead abandons anything sounding, and stopping
releases notes rather than cutting them dead.

## Instruments and notes

`src/engine/voice` is a polyphonic subtractive instrument. Each voice mixes
two band-limited oscillators, a sine sub oscillator, and deterministic white
noise before a resonant filter and amplitude envelope. Each pitched oscillator
can render up to four detuned copies. A repeated pitch restarts a single voice,
and a full bank takes the quietest sounding voice. Patch values are bounded on
the control side and rendering uses fixed storage.

`src/engine/schedule` converts notes written in beats into note events
carrying the frame they land on. A note contained in one block gets both
its start and its end; a note crossing the edge gets its end in a later
block; a note shorter than a frame still starts before it ends. A full
event buffer reports what it dropped rather than losing it quietly.

`src/engine/sample` owns decoded stereo media and provides a borrowed player
for real-time use. It supports linear or cubic interpolation, rate conversion,
reverse playback, clip gain, seeking, and frame-accurate loops. Playback has
an allocator test because media storage is prepared before rendering starts.

`src/engine/timeline` groups decoded media with beat-based track placements.
Regions are indexed by track before publication, so the callback visits only
placements that can contribute to each buffer. A timeline crosses the same
bounded ownership exchange as other engine state; replaced media returns to
the control thread for destruction.

The engine publishes these together as a score: per instrument track, the
notes, the patch, and whether the track sounds. Arrangement notes use absolute
beats. A launched Session clip replaces arrangement notes on that track and
repeats inside a bounded loop until stopped or replaced.

## Audio devices

`src/audio` defines the backend, stream, and renderer interfaces. Device
names and rate lists are fixed-capacity values, so enumerating devices
costs no allocation. `src/audio/offline` renders on demand rather than
against a clock, which is what a bounce needs and what lets a test step a
render one block at a time.

`src/bounce` maps an immutable project snapshot into the playback engine,
locates to the requested beat, and streams stereo blocks into a WAVE writer.
Notes crossing the range start are retriggered with their remaining duration.
The same project, range, rate, and seed produce identical bytes.

Each platform has one backend, selected by the `platform_audio` flag the
build script sets. A platform with none still compiles and reports the
absence when a stream is asked for. Every backend reports the block size
the host granted rather than the one that was requested, so a caller that
assumes its own request will be wrong on a device that rounds.

`src/audio/coreaudio` drives a hardware output unit on macOS. The device
decides its own buffer size, so the requested block is a request and the
granted size is reported back; the maximum slice is set to the largest
block the engine can fill, because a device asking for more than the
requested size otherwise renders nothing. A static core carries no record
of the frameworks it calls into, so anything linking it names them; the
imported target in `bindings/cmake/NylonCore.cmake` does that.

`src/audio/alsa` drives Linux. The library is opened through the dynamic
loader rather than linked, so the core needs nothing installed to build
and a machine without it reports no devices. ALSA offers no callback: a
stream owns a thread that renders a block and writes it, which gives the
renderer the same contract a callback host does. The device rounds the
requested block to its own period, and underruns are recovered and
counted as dropouts.

`src/audio/wasapi` drives Windows. Its interfaces are declared in tree, as
the other backends declare theirs. A shared-mode client signals an event
when it wants audio and a thread waits on it, renders, and copies into the
buffer the client hands out; the client converts for a device mixing at
another rate. A wait that times out is counted as a dropout.

## Recording

A `Capturer` is the mirror of a `Renderer` and makes the same promise
about the audio thread. A host that can record implements `InputBackend`
rather than a wider `Backend`, because a host may play without recording,
and every backend implements it: the offline one takes whatever it is
handed, and the three platform backends read from a device. A capture
stream reports frames, dropouts and a lost device exactly as a playback
stream does.

Nothing in the core opens an input on its own. A recording begins when a
caller opens a capture stream, which is what keeps a microphone from
being read by a program that was only asked to play.

`src/audio/recording` copies fixed-capacity blocks into a bounded SPSC queue.
The device callback performs no file access or allocation. A separate thread
drains blocks into a 32-bit floating-point WAVE file and reports any frames
rejected by a full queue. Project recording reserves a unique file in `Media/`;
finishing creates one normal undoable clip command, while abandoning the handle
removes the reserved file.

Tests that need a real device are marked ignored and run deliberately.

## Plugin processing

Plugin discovery recognizes VST3, Audio Unit, CLAP, and LV2 packages without
loading executable code. CLAP descriptor probing and audio processing run in
separate processes. A failed load, malformed descriptor, timeout, or process
failure cannot corrupt the project process and can quarantine the catalog
entry.

An active CLAP instance validates stereo ports, reports its processing latency,
enumerates parameter metadata, and accepts up to 1,024 ordered parameter value
events per block. Each event carries a sample offset, and its value applies to
that frame. Event records live in fixed storage allocated when the instance is
opened. The worker protocol publishes latency and parameter metadata during its
handshake, then carries bounded audio blocks, parameter events, and CLAP note
on, off, and choke events. The host follows the declared audio-input topology,
so instruments with no audio input receive events and produce stereo output.
The core worker client starts and owns the process, validates bounded handshake
metadata, reuses request and response buffers, and rejects malformed event
ranges before sending a block. It performs blocking pipe I/O and therefore runs
on a dedicated IPC thread. A process watchdog bounds startup to ten seconds and
each processing or state response to two seconds. A missed deadline terminates
the worker, closes its request pipe, and makes later calls fail immediately. A
bounded bridge adds one block of declared latency
between that thread and the render callback. Every audio and event buffer is
allocated when the bridge opens. The callback uses wait-free queues and returns
a delayed dry block when the worker misses its deadline. Submitted, completed,
underrun, queue-drop, and worker-failure counters expose its health. State save
and load commands transfer opaque plugin state with a 256 MiB limit. State I/O
never enters an audio callback.

## Benchmarks

The render benchmark reports nanoseconds per 256-frame stereo block after warmup.
The reverb benchmark measures one active stereo instance over the same block.
The auto-filter benchmark measures a driven stereo instance with envelope and
low-frequency modulation active.
The phaser benchmark measures a six-stage stereo instance with feedback.
The plugin bridge benchmark measures callback-side block submission and delayed
response collection without including plugin process time.
The mixer benchmark measures a 32-track block with automation. The audio timeline
benchmark measures the complete callback for 32 tracks and 64 looping regions.
Both arrangement benchmarks report their share of the block's real-time budget.
CI compares the previous revision and candidate on the same runner, alternating
21 measurement pairs. A run fails only when the median rises more than 5% and the
two samples separate, because at a few tens of nanoseconds a block, 5% is inside
the noise of a shared runner. Dedicated performance hardware remains preferable.

The project model publishes immutable snapshots through grouped commands. Undo
and redo retain prior snapshots in memory. Track identifiers remain unique even
after branching from an earlier undo state. Failed command groups publish nothing.

Each track stores one ordered automation lane per mixer parameter. Points use
beat positions and step, linear, or smooth segment shapes. Lane replacement and
clearing are normal grouped commands, so both operations participate in undo and
survive project persistence. The control thread publishes prepared lanes through
a bounded state exchange. The callback schedules segment boundaries into fixed
storage, applies mute and solo at the requested frame, and evaluates volume and
pan for each sample. Live playback and offline bounce use the same path.

Native clients use the C or C++ interfaces for project editing, device playback,
recording, routing, persistence, and deterministic bounce. Plugin isolation and
several workstation subsystems remain separate implementation areas.
