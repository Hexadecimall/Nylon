"""Exercise the persistent local control endpoint."""
import json
import pathlib
import subprocess
import sys
import tempfile
import uuid


def main():
    binary = sys.argv[1]
    with tempfile.TemporaryDirectory() as directory:
        project = pathlib.Path(directory) / "Session.nylon"
        created = subprocess.run(
            [binary, "--project", str(project), "new"],
            capture_output=True, text=True, timeout=10,
        )
        assert created.returncode == 0, (created.stdout, created.stderr)
        added = subprocess.run(
            [binary, "--project", str(project), "add-track", "midi", "Lead"],
            capture_output=True, text=True, timeout=10,
        )
        assert added.returncode == 0, (added.stdout, added.stderr)

        endpoint = "nylon-test-" + uuid.uuid4().hex
        server = subprocess.Popen(
            [binary, "--project", str(project), "--endpoint", endpoint,
             "--no-audio", "serve"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            ready = json.loads(server.stdout.readline())
            assert ready == {"audioOpen": False, "endpoint": endpoint, "ok": True}, ready

            def call(command, *arguments, succeeds=True):
                result = subprocess.run(
                    [binary, "--endpoint", endpoint, command, *arguments],
                    capture_output=True, text=True, timeout=10,
                )
                reply = json.loads(result.stdout)
                assert (result.returncode == 0) is succeeds, (result.stdout, result.stderr)
                assert reply["ok"] is succeeds, reply
                return reply

            status = call("status")
            assert status["tracks"] == 1 and status["tempo"] == 120, status
            assert status["audioOpen"] is False and status["playing"] is False, status
            assert status["framesRendered"] == 0 and status["dropouts"] == 0, status
            changed = subprocess.run(
                [binary, "--project", str(project), "set-tempo", "130"],
                capture_output=True, text=True, timeout=10,
            )
            assert changed.returncode == 0, (changed.stdout, changed.stderr)
            assert call("status")["tempo"] == 120
            call("reload")
            assert call("status")["tempo"] == 130
            call("set-tempo", "141")
            assert call("status")["tempo"] == 141
            call("set-tempo-at", "8", "96")
            assert call("tempo-map")["points"] == [
                {"beat": 0, "tempo": 141}, {"beat": 8, "tempo": 96}
            ]
            call("delete-tempo-change", "8")
            call("set-track-volume", "0", "-9")
            call("set-track-mute", "0", "on")
            call("set-instrument", "0", "saw", "square", "0.6", "-7", "0.35", "0.04",
                 "4", "18", "0.01", "0.2", "0.7", "0.3", "2400", "1.2", "-9")
            instrument = call("instrument", "0")["instrument"]
            assert instrument["unisonVoices"] == 4 and instrument["cutoffHz"] == 2400, instrument
            call("add-device", "0", "reverb", "0.6", "2.8", "0.35", "0.7", "0.02", "1", "0.3")
            call("add-device", "0", "chorus", "0.8", "0.012", "0.003", "0.1", "0.5", "0.25")
            devices = call("track-devices", "0")["devices"]
            assert [device["kind"] for device in devices] == ["reverb", "chorus"], devices
            call("move-device", "0", "1", "0")
            call("set-device-enabled", "0", "0", "off")
            devices = call("track-devices", "0")["devices"]
            assert devices[0]["kind"] == "chorus" and devices[0]["enabled"] is False, devices
            call("set-device", "0", "1", "utility", "-3", "1.2", "0")
            call("delete-device", "0", "1")
            devices = call("track-devices", "0")["devices"]
            assert len(devices) == 1 and devices[0]["kind"] == "chorus", devices
            call("add-plugin", "0", "clap", "Effect.clap", "app.nylon.effect", "64", "on")
            devices = call("track-devices", "0")["devices"]
            assert devices[1]["kind"] == "plugin" and devices[1]["latencyFrames"] == 64, devices
            call("set-plugin-parameter", "0", "1", "31", "0.75")
            assert call("track-devices", "0")["devices"][1]["parameters"] == [
                {"identifier": 31, "value": 0.75}
            ]
            call("set-device-enabled", "0", "1", "off")
            assert call("track-devices", "0")["devices"][1]["enabled"] is False
            call("add-device", "0", "reverb", "0.6", "31", "0.35", "0.7", "0.02", "1", "0.3", succeeds=False)
            call("create-midi-clip", "0", "0", "4")
            call("add-note", "0", "0", "60", "80", "0.22", "0.5")
            call("quantize-notes", "0", "0", "0.25", "1")
            call("transpose-notes", "0", "0", "3")
            call("set-note-velocity", "0", "0", "96")
            call("humanize-notes", "0", "0", "0.02", "4", "42")
            notes = call("notes", "0", "0")["notes"]
            assert len(notes) == 1 and notes[0]["pitch"] == 63, notes
            call("transpose-notes", "0", "0", "100", succeeds=False)
            call("undo")
            call("redo")
            call("play", succeeds=False)
            call("panic", succeeds=False)
            call("locate", "bad", succeeds=False)
            call("unknown", succeeds=False)
            call("quit")
            assert server.wait(timeout=10) == 0
            saved = subprocess.run(
                [binary, "--project", str(project), "info"],
                capture_output=True, text=True, timeout=10,
            )
            saved_state = json.loads(saved.stdout)
            assert saved.returncode == 0 and saved_state["tempo"] == 141, saved_state
        finally:
            if server.poll() is None:
                server.terminate()
                server.wait(timeout=5)
        print("Persistent control CLI: pass")


if __name__ == "__main__":
    main()
