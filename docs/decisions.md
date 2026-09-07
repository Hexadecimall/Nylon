# Engineering decisions

## Desktop toolkit

The GUI uses Qt. The native frontend communicates with the Rust control thread
through a narrow interface. Toolkit selection does not change render-thread
constraints. A Qt binding and minimum version have not been selected.
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
