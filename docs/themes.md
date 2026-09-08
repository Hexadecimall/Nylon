# Theme format

Themes are plain-text files with the `.theme` extension. The four shipped
themes live in `themes/` and are compiled into the application. A file with
the same name in the user theme directory overrides the built-in copy and is
reloaded when it changes on disk. The user theme directory is
`<AppConfigLocation>/themes`, where `AppConfigLocation` is Qt's per-platform
application configuration directory.

## Syntax

One entry per line, `key = value`. Whitespace around the key and value is
ignored. Blank lines and lines beginning with `#` are comments.

| Prefix | Value | Notes |
| --- | --- | --- |
| `name` | text | Display name. Exactly one per file. |
| `font.<key>` | text | `font.family = system` selects the platform default. |
| `color.<key>` | `#RRGGBB` or `#RRGGBBAA` | Hex digits only. Eight digits are read as RGBA. |
| `metric.<key>` | number | Finite, from 0 to 100000. Pixels unless noted. |

A malformed line is reported with its line number and skipped; the rest of
the file still loads. A duplicate key is reported and the later value wins.
A theme missing any required key is rejected and the previous theme stays
active.

## Required colors

| Key | Use |
| --- | --- |
| `background` | Window and view background |
| `panel` | Menu bar, headers, status bar |
| `raised` | Menus, scrollbar handles |
| `separator` | 1px lines between regions |
| `text.primary` | Labels and values |
| `text.secondary` | Headers, empty-state text, counts |
| `text.disabled` | Disabled controls and menu items |
| `accent` | Selection and active view marker |
| `accent.text` | Text drawn on the accent color |
| `playhead` | Playhead line |
| `selection` | Selection overlay; usually carries alpha |
| `session.slot` | Empty clip slot |
| `session.slot.hover` | Hovered clip slot |
| `session.stop_button` | Stop marker inside a slot |
| `arrangement.ruler` | Bar ruler background |
| `arrangement.lane` | Even track lanes |
| `arrangement.lane.alt` | Odd track lanes |
| `arrangement.grid` | Beat lines |
| `arrangement.grid.bar` | Bar lines |
| `control.background` | Buttons and entry fields |
| `control.border` | Control outline |
| `control.text` | Control text |
| `control.hover` | Hovered control |
| `control.pressed` | Pressed or checked control |
| `control.disabled` | Disabled control text |
| `meter.rms` | Meter RMS segment |
| `meter.peak` | Meter peak segment |
| `meter.clip` | Meter clip indicator |
| `track.1` to `track.16` | Track color palette, cycled by track index |
| `state.on` | Track activator when on |
| `state.solo` | Solo button when on |
| `state.arm` | Record-arm button when on |
| `state.play` | Play button while playing |
| `state.record` | Record button while recording |
| `state.loop` | Loop button when on |
| `browser.background` | Browser and list backgrounds |
| `browser.selection` | Selected row in lists and trees |
| `browser.header` | Browser column header |
| `detail.background` | Detail panel background |
| `mixer.background` | Channel strip background |
| `fader.track` | Fader travel |
| `fader.fill` | Fader fill below the handle |
| `fader.handle` | Fader handle |
| `knob.track` | Knob background arc |
| `knob.arc` | Knob value arc |
| `meter.background` | Meter background |
| `clip.empty.hover` | Hovered empty clip slot |
| `window.border` | Outline of the frameless window |
| `titlebar.background` | Title bar band |
| `panel.border` | Outline of rounded panels |
| `window.control.close` | Close window control |
| `window.control.minimize` | Minimize window control |
| `window.control.zoom` | Maximize/zoom window control |

## Required metrics

| Key | Meaning |
| --- | --- |
| `separator` | Separator thickness |
| `control.height` | Button and field height |
| `control.padding` | Horizontal padding inside controls |
| `transport.height` | Height of the transport strip |
| `session.slot.width` | Clip slot width |
| `session.slot.height` | Clip slot height |
| `session.scene.count` | Number of scene rows |
| `session.master.width` | Width of the master column |
| `arrangement.ruler.height` | Bar ruler height |
| `arrangement.lane.height` | Track lane height |
| `arrangement.header.width` | Track header width |
| `arrangement.pixels_per_bar` | Horizontal zoom |
| `arrangement.bars` | Number of bars the timeline spans |
| `font.size` | Text size in pixels |
| `text.inset` | Horizontal inset of text inside headers and slots |
| `transport.spacing` | Gap between control groups in the transport strip |
| `transport.tempo.width` | Width of the tempo field |
| `session.header.band` | Height of the track color band on a session header |
| `session.stop.size` | Side length of the stop marker in a slot |
| `session.stop.inset` | Distance from the slot edge to the stop marker |
| `arrangement.header.band` | Width of the track color band on a lane header |
| `browser.width` | Initial browser panel width |
| `detail.height` | Initial detail panel height |
| `mixer.height` | Height of the channel strip row |
| `fader.width` | Fader travel width |
| `fader.handle.height` | Fader handle height |
| `knob.size` | Knob diameter |
| `meter.channel.width` | Width of one meter channel |
| `meter.clip.height` | Height of the clip indicator |
| `strip.button.height` | Height of activator, solo, and arm buttons |
| `transport.button.size` | Side of the transport glyph buttons |
| `radius` | Corner radius of panels, menus, and the window |
| `radius.small` | Corner radius of controls, slots, and list rows |
| `panel.gap` | Space between panels |
| `panel.padding` | Inner padding of panels |
| `titlebar.height` | Height of the custom title bar |
| `window.border` | Thickness of the window outline |
| `window.control.size` | Diameter of the window controls |

Metrics that reach the Qt style sheet are clamped before use: `separator`
to 0–16, `control.padding` to 0–64, and `control.height` to 8–256. The views
use the unclamped values.

Layout extents are computed in 64-bit arithmetic and clamped to 16,777,216
pixels per axis, so any combination of accepted metrics and counts is safe;
content past that extent is not reachable by scrolling.

## Shipped themes

| File | Character |
| --- | --- |
| `nylon.theme` | Default. Near-black slate base, neutral greys, single blue accent. |
| `slate.theme` | Mid-grey base for brighter rooms. |
| `graphite.theme` | Darker than Nylon. |
| `paper.theme` | Light base with dark text. Track colors are darkened for legibility. |

Every shipped theme defines the same key set; `test_theme` enforces this.
