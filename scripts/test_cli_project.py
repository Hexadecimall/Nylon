"""Exercise direct project commands through the console executable."""
import json
import pathlib
import subprocess
import struct
import sys
import tempfile
import wave


def main():
    binary = sys.argv[1]
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory)
        project = root / "Session.nylon"
        output = root / "mix.wav"
        source = root / "take.wav"
        with wave.open(str(source), "wb") as audio:
            audio.setnchannels(2)
            audio.setsampwidth(2)
            audio.setframerate(48000)
            audio.writeframes(b"".join(struct.pack("<hh", 8192, -8192) for _ in range(480)))

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
        call("add-track", "audio", "Take")
        tracks = call("tracks")["tracks"]
        assert tracks[0]["name"] == "Lead" and tracks[0]["kind"] == "midi", tracks
        assert tracks[1]["name"] == "Take" and tracks[1]["kind"] == "audio", tracks
        call("set-track-latency", "0", "256")
        call("add-route", "0", "1", "sidechain", "0.5")
        routes = call("routes")["routes"]
        assert routes == [{"destination": 1, "gain": 0.5, "index": 0,
                           "kind": "sidechain", "source": 0}], routes
        call("add-route", "1", "0", "main", succeeds=False)
        call("add-device", "0", "utility", "-6", "1.25", "-0.1")
        call("add-device", "0", "delay", "0.25", "0.4", "0.3")
        chain = call("track-devices", "0")["devices"]
        assert [device["kind"] for device in chain] == ["utility", "delay"], chain
        assert chain[0]["parameters"]["gainDb"] == -6 and chain[0]["enabled"], chain
        call("move-device", "0", "1", "0")
        call("set-device-enabled", "0", "1", "off")
        call("set-device", "0", "0", "equalizer", "peaking", "1000", "0.7", "3")
        chain = call("track-devices", "0")["devices"]
        assert chain[0]["kind"] == "equalizer", chain
        assert chain[0]["parameters"]["filter"] == "peaking", chain
        assert chain[1]["kind"] == "utility" and not chain[1]["enabled"], chain
        call("delete-device", "0", "1")
        assert len(call("track-devices", "0")["devices"]) == 1
        call("add-device", "0", "limiter", "-0.3", "0.1", "0.005")
        limiter = call("track-devices", "0")["devices"][1]
        assert limiter["kind"] == "limiter", limiter
        assert abs(limiter["parameters"]["lookaheadSeconds"] - 0.005) < 1e-7, limiter
        call("add-device", "0", "delay", "0.2", "1", "0.5", succeeds=False)
        call("add-device", "0", "limiter", "-0.3", "0.1", "0.1", succeeds=False)
        call("import-wave", "1", "0", str(source), "120")
        clips = call("clips", "1")["clips"]
        assert clips[0]["kind"] == "audio" and clips[0]["mediaPath"].startswith("Media/"), clips
        call("set-audio-gain", "1", "0", "-3")
        call("place-clip", "1", "0", "0", "1")
        call("set-tempo", "137")
        assert call("info")["tempo"] == 137
        call("undo")
        assert call("info")["tempo"] == 120
        call("redo")
        assert call("info")["tempo"] == 137
        rendered = call("bounce", str(output), "0", "1", "48000")
        assert rendered["frames"] > 0 and output.stat().st_size > 44, rendered
        assert rendered["peakLeft"] > 0 and rendered["peakRight"] > 0, rendered
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
