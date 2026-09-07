# Desktop frontend

The desktop application is a C++17 Qt 6 Widgets program under `desktop/`.
It links the Rust core as a static library through the C interface declared
in `desktop/nylon.h`. Qt owns windows, input, layout, and painting; the core
owns project state and, later, audio.

## Build

Prerequisites: CMake 3.21 or newer, a C++17 compiler, Qt 6.4 or newer with
the Widgets and Test modules, and a release build of the core:

```sh
cargo build --release
cmake -S desktop -B target/desktop -DCMAKE_BUILD_TYPE=Release
cmake --build target/desktop
ctest --test-dir target/desktop --output-on-failure
```

Set `CMAKE_PREFIX_PATH` to the Qt installation prefix when CMake cannot find
it. `NYLON_CORE_LIBRARY` overrides the core library location; the default is
`target/release/libnylon.a` (`target/release/nylon.lib` with MSVC).

Source paths are remapped to `.` in debug info and `__FILE__` with
`-ffile-prefix-map` (Clang, GCC) or `/pathmap` (MSVC) so binaries do not
record the build machine's directory layout.

## Command line

| Option | Effect |
| --- | --- |
| `--theme <name>` | Theme to load at startup. Default `nylon`. |
| `--tracks <count>` | Add this many tracks to the new project. |
| `--view <name>` | Initial view, `session` or `arrangement`. |
| `--screenshot <file>` | Write a PNG of the main window and exit. |

Screenshots work without a display when `QT_QPA_PLATFORM=offscreen` is set.

## Layout

`src/ProjectBridge` wraps one core project handle. Every edit goes through
it and emits `changed()` when the core accepted the command; views repaint
from the new snapshot. Rejected edits emit nothing.

`src/TransportBar` holds the tempo entry, Add Track, Undo, Redo, and the
Session/Arrangement switch. There is no play control because the core does
not expose a transport yet.

`src/SessionView` paints the clip launch grid: one column per track, one row
per scene, and a master column with scene slots. `src/ArrangementView`
paints the bar ruler, track headers, and lanes with a beat grid. Both derive
every color and size from the active theme and draw an empty state when the
project has no tracks.

`src/MainWindow` assembles the menu bar, transport, the stacked views, and
the status bar, and re-applies the theme whenever `ThemeManager` reports a
change.

## Tests

Three Qt Test executables build alongside the application and run under
`ctest` with the offscreen platform:

- `test_theme`: theme file parsing, validation, the built-in theme set, and
  `ThemeManager` behavior.
- `test_bridge`: the C interface and `ProjectBridge`, including null handles,
  tempo range, undo/redo, and signal emission.
- `test_views`: the assembled main window driven through its real controls,
  with pixel checks against theme tokens.
