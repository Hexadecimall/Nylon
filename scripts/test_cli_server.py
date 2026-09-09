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
            call("set-track-volume", "0", "-9")
            call("set-track-mute", "0", "on")
            call("undo")
            call("redo")
            call("play", succeeds=False)
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
