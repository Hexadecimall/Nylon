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

## Commands and shortcuts

Every menu entry is a named `QAction`. View > Command Palette (Ctrl+K or
Ctrl+Shift+P) searches them by name with subsequence matching, shows the
current shortcut, and triggers the highlighted entry; disabled entries are
listed but inert. Preferences > Shortcuts edits the key of each action,
refuses keys already in use, and restores defaults. Overrides live in the
application settings under `shortcuts/<action name>` and are applied when
the window is built.

## Tests

Three Qt Test executables build alongside the application and run under
`ctest` with the offscreen platform:

- `test_theme`: theme file parsing, validation, the built-in theme set, and
  `ThemeManager` behavior.
- `test_bridge`: the C interface and `ProjectBridge`, including null handles,
  tempo range, undo/redo, and signal emission.
- `test_views`: the assembled main window driven through its real controls,
  with pixel checks against theme tokens.
- `test_widgets`: the custom controls (knob, fader, meter, button, value box).
- `test_commands`: the command palette and shortcut overrides.
- `test_layout`: the integer layout helpers.

## Portable layout

`cmake --install target/desktop --prefix <dir>` produces a self-contained
directory:

```
Nylon/
  Bin/         Nylon executable, nylon-control, qt.conf
  Frameworks/  the core library and the Qt runtime
  Resources/   themes/, icons/, font/
  Plugins/     Qt platform, image, style plugins; nylon/ for feature plugins
```

On macOS the Qt runtime is deployed with `macdeployqt` and everything is
signed ad hoc; sign with a distribution identity before shipping. On
Linux the executable carries `DT_RPATH` of `$ORIGIN/../Frameworks`, so the
libraries the plugins load are found without an environment variable. On
Windows the loader searches only the executable's directory, so the Qt
DLLs sit in `Bin/` next to the executables while `Plugins/` keeps the
layout; `windeployqt` places them. Pass `NYLON_WINDEPLOYQT_QTPATHS` when
deploying a cross-compiled kit with the host kit's `windeployqt`.

The build workflow installs this layout on every target, launches the
installed executable from a different directory as a smoke test, and
uploads the directory as the artifact.

