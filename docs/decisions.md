# Engineering decisions

## Desktop toolkit

The GUI uses Qt. The native frontend communicates with the Rust control thread
through a narrow interface. Toolkit selection does not change render-thread
constraints. The frontend is C++17 with Qt Widgets and links directly to the
Rust static library through a C interface. No Rust Qt binding is required.
The supported desktop platforms are macOS, Windows, and Linux. Platform-specific
device backends remain behind the Rust engine boundary.

## Command transactions

A command group either publishes a complete snapshot or preserves the previous
state. Undo and redo move between immutable snapshots. Identifier allocation is
monotonic across undo branches to prevent stale handles from targeting a new track.

## Initial tempo range

Tempo accepts finite values from 20 through 999 beats per minute. Validation takes
place before snapshot publication. This limit belongs to the control model;
the render clock measures integer sample frames.

## Visual language

Panels, controls, menus, clip slots, and the window itself use rounded
corners, with the radii, panel gaps, and padding tokenized in the theme
files (`radius`, `radius.small`, `panel.gap`, `panel.padding`). The window
is frameless with a custom title bar that carries the menus and the window
controls. This replaces the flat, square layout of the first desktop
iteration; density and proportions still follow a session/arrangement
workstation.

