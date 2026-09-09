# Parked work

The Qt desktop front end and its theme files live here. They are kept for
reference and are not built, tested, or shipped.

- `desktop/` - the Qt Widgets application, its widgets, views, and tests
- `themes/` - the theme files the application read
- `docs/` - the notes that described them
- `qt_check.py` - the script that built and exercised the application in CI

The headless command line tool stayed in `cli/`, since it drives the core
directly rather than a window.
