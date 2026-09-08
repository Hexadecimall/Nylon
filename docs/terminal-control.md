# Terminal control

The Qt desktop accepts local terminal commands when launched with
`--control <endpoint>`. The endpoint is disabled by default. Connections are
restricted to the current account by Qt local socket permissions. No elevated
privileges or system installation are required.

Start the desktop with `--workspace --control nylon-local`. Use the console
executable from the build directory:

```sh
target/desktop/nylon-control --endpoint nylon-local info
target/desktop/nylon-control --endpoint nylon-local set-tempo 128
target/desktop/nylon-control --endpoint nylon-local add-track
target/desktop/nylon-control --endpoint nylon-local undo
target/desktop/nylon-control --endpoint nylon-local redo
target/desktop/nylon-control --endpoint nylon-local view arrangement
target/desktop/nylon-control --endpoint nylon-local view session
target/desktop/nylon-control --endpoint nylon-local window maximize
target/desktop/nylon-control --endpoint nylon-local window minimize
target/desktop/nylon-control --endpoint nylon-local window restore
```

On Windows the console executable has an `.exe` suffix. Each response is a JSON
object with an `ok` field. Rejected commands and connection failures return a
nonzero exit status. Project edits use the desktop command bridge and share its
undo history. Window commands execute on the Qt event loop.

Each connection carries one newline-terminated JSON request, limited to 8192
bytes, with `command` and a string array named `args`. Idle connections expire
after two seconds. Startup fails if the requested endpoint cannot be opened;
existing endpoints are never removed automatically.
