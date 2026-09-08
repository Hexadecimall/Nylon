"""Exercise direct project commands through the console executable."""
import json
import pathlib
import subprocess
import sys
import tempfile


def main():
    binary = sys.argv[1]
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory)
        project = root / "Session.nylon"
        output = root / "mix.wav"

        def call(command, *arguments, succeeds=True):
            result = subprocess.run(
                [binary, "--project", str(project), command, *arguments],
                capture_output=True, text=True, timeout=10,
            )
            if succeeds:
                assert result.returncode == 0, (result.stdout, result.stderr)
            else:
                assert result.returncode != 0, result.stdout
            reply = json.loads(result.stdout)
            assert reply["ok"] is succeeds, reply
            return reply

        call("new")
        state = call("info")
        assert state["tempo"] == 120 and state["tracks"] == 0, state
        call("add-track", "midi", "Lead")
        tracks = call("tracks")["tracks"]
        assert tracks[0]["name"] == "Lead" and tracks[0]["kind"] == "midi", tracks
        call("set-tempo", "137")
        assert call("info")["tempo"] == 137
        call("undo")
        assert call("info")["tempo"] == 120
        call("redo")
        assert call("info")["tempo"] == 137
        rendered = call("bounce", str(output), "0", "1", "48000")
        assert rendered["frames"] > 0 and output.stat().st_size > 44, rendered
        call("set-tempo", "invalid", succeeds=False)
        call("unsupported", succeeds=False)

        devices = subprocess.run(
            [binary, "devices"], capture_output=True, text=True, timeout=10,
        )
        assert devices.returncode == 0, (devices.stdout, devices.stderr)
        assert isinstance(json.loads(devices.stdout)["devices"], list)
        print("Direct project CLI: pass")


if __name__ == "__main__":
    main()
