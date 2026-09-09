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
    clap_fixture = sys.argv[2]
    plugin_probe = sys.argv[3]
    crash_fixture = sys.argv[4]
    plugin_worker = sys.argv[5]
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
        call("set-instrument", "0", "saw", "square", "0.6", "-7", "0.35", "0.04",
             "4", "18", "0.01", "0.2", "0.7", "0.3", "2400", "1.2", "-9")
        instrument = call("instrument", "0")["instrument"]
        assert instrument["shapeA"] == "saw" and instrument["shapeB"] == "square", instrument
        assert instrument["unisonVoices"] == 4 and instrument["cutoffHz"] == 2400, instrument
        call("set-instrument", "1", "saw", "square", "0.6", "-7", "0.35", "0.04",
             "4", "18", "0.01", "0.2", "0.7", "0.3", "2400", "1.2", "-9",
             succeeds=False)
        call("create-midi-clip", "0", "0", "4")
        call("add-note", "0", "0", "60", "80", "0.22", "0.5")
        call("add-note", "0", "0", "64", "100", "0.81", "0.5")
        call("quantize-notes", "0", "0", "0.25", "1")
        call("transpose-notes", "0", "0", "3")
        call("set-note-velocity", "0", "0", "96")
        call("humanize-notes", "0", "0", "0.02", "4", "42")
        notes = call("notes", "0", "0")["notes"]
        assert [note["pitch"] for note in notes] == [63, 67], notes
        assert all(1 <= note["velocity"] <= 127 for note in notes), notes
        assert call("transpose-notes", "0", "0", "100", succeeds=False)["ok"] is False
        call("set-automation", "0", "volume", "0", "-12", "linear", "4", "0", "smooth")
        automation = call("automation", "0", "volume")["points"]
        assert automation == [
            {"beat": 0, "curve": "linear", "value": -12},
            {"beat": 4, "curve": "smooth", "value": 0},
        ], automation
        call("set-automation", "0", "mute", "0", "0.5", "linear", succeeds=False)
        call("clear-automation", "0", "volume")
        assert call("automation", "0", "volume")["points"] == []
        call("undo")
        assert len(call("automation", "0", "volume")["points"]) == 2
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

        call("add-device", "0", "saturator", "9", "-4", "0.75", "diode", "4x", "on")
        saturator = call("track-devices", "0")["devices"][2]
        assert saturator["kind"] == "saturator", saturator
        assert saturator["parameters"] == {
            "curve": "diode",
            "dcFilter": True,
            "driveDb": 9,
            "mix": 0.75,
            "outputDb": -4,
            "oversampling": "4x",
        }, saturator
        call("add-device", "0", "gate", "-32", "8", "0.002", "0.04", "0.15", "on")
        gate = call("track-devices", "0")["devices"][3]
        assert gate["kind"] == "gate", gate
        gate_parameters = gate["parameters"]
        assert gate_parameters["externalSidechain"] is True, gate
        assert gate_parameters["thresholdDb"] == -32, gate
        assert gate_parameters["hysteresisDb"] == 8, gate
        assert abs(gate_parameters["attackSeconds"] - 0.002) < 1e-7, gate
        assert abs(gate_parameters["holdSeconds"] - 0.04) < 1e-7, gate
        assert abs(gate_parameters["releaseSeconds"] - 0.15) < 1e-7, gate
        call("add-device", "0", "chorus", "0.8", "0.012", "0.003", "0.1", "0.5", "0.25")
        chorus = call("track-devices", "0")["devices"][4]
        assert chorus["kind"] == "chorus", chorus
        assert abs(chorus["parameters"]["rateHz"] - 0.8) < 1e-6, chorus
        assert chorus["parameters"]["stereoPhase"] == 0.25, chorus
        call("add-device", "0", "reverb", "0.6", "2.8", "0.35", "0.7", "0.02", "1", "0.3")
        reverb = call("track-devices", "0")["devices"][5]
        assert reverb["kind"] == "reverb", reverb
        assert abs(reverb["parameters"]["decaySeconds"] - 2.8) < 1e-6, reverb
        assert reverb["parameters"]["width"] == 1, reverb
        call("add-device", "0", "auto-filter", "band-pass", "1600", "2.5", "8",
             "-1.5", "0.004", "0.2", "0.75", "1.25", "0.6", "on")
        auto_filter = call("track-devices", "0")["devices"][6]
        assert auto_filter["kind"] == "auto-filter", auto_filter
        assert auto_filter["parameters"]["mode"] == "band-pass", auto_filter
        assert auto_filter["parameters"]["cutoffHz"] == 1600, auto_filter
        assert auto_filter["parameters"]["externalSidechain"] is True, auto_filter
        call("add-device", "0", "phaser", "0.6", "850", "2.25", "0.45", "0.7",
             "0.4", "10")
        phaser = call("track-devices", "0")["devices"][7]
        assert phaser["kind"] == "phaser", phaser
        assert phaser["parameters"]["centerHz"] == 850, phaser
        assert phaser["parameters"]["stages"] == 10, phaser
        call("add-plugin", "0", "clap", "Effect.clap", "app.nylon.effect", "96", "on")
        plugin = call("track-devices", "0")["devices"][8]
        assert plugin == {"enabled": True, "format": "clap", "identifier": "app.nylon.effect",
                          "index": 8, "kind": "plugin", "latencyFrames": 96,
                          "package": "Effect.clap", "stateBytes": 0}, plugin
        call("set-device-enabled", "0", "8", "off")
        assert call("track-devices", "0")["devices"][8]["enabled"] is False
        call("move-device", "0", "8", "0")
        assert call("track-devices", "0")["devices"][0]["kind"] == "plugin"
        call("add-device", "0", "delay", "0.2", "1", "0.5", succeeds=False)
        call("add-device", "0", "limiter", "-0.3", "0.1", "0.1", succeeds=False)
        call("add-device", "0", "saturator", "9", "-4", "0.75", "bad", "4x", "on", succeeds=False)
        call("add-device", "0", "gate", "-32", "8", "0.002", "0.04", "31", "on", succeeds=False)
        call("add-device", "0", "chorus", "0.8", "0.01", "0.02", "0.1", "0.5", "0.25", succeeds=False)
        call("add-device", "0", "reverb", "0.6", "31", "0.35", "0.7", "0.02", "1", "0.3", succeeds=False)
        call("add-device", "0", "auto-filter", "bad", "1600", "2.5", "8", "-1.5",
             "0.004", "0.2", "0.75", "1.25", "0.6", "on", succeeds=False)
        call("add-device", "0", "phaser", "0.6", "850", "2.25", "0.45", "0.7",
             "0.4", "13", succeeds=False)
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
        call("record", "1", "1", "0", succeeds=False)
        call("unsupported", succeeds=False)

        devices = subprocess.run(
            [binary, "devices"], capture_output=True, text=True, timeout=10,
        )
        assert devices.returncode == 0, (devices.stdout, devices.stderr)
        assert isinstance(json.loads(devices.stdout)["devices"], list)
        inputs = subprocess.run(
            [binary, "input-devices"], capture_output=True, text=True, timeout=10,
        )
        assert inputs.returncode == 0, (inputs.stdout, inputs.stderr)
        assert isinstance(json.loads(inputs.stdout)["devices"], list)
        plugins = root / "plugins"
        (plugins / "Alpha.component").mkdir(parents=True)
        (plugins / "Nested" / "Beta.vst3").mkdir(parents=True)
        (plugins / "Gamma.clap").write_bytes(b"binary")
        scanned = subprocess.run(
            [binary, "scan-plugins", str(plugins), str(root / "missing-plugins")],
            capture_output=True, text=True, timeout=10,
        )
        assert scanned.returncode == 0, (scanned.stdout, scanned.stderr)
        catalog = json.loads(scanned.stdout)
        assert [entry["name"] for entry in catalog["plugins"]] == ["Alpha", "Gamma", "Beta"], catalog
        assert [entry["format"] for entry in catalog["plugins"]] == [
            "audio-unit", "clap", "vst3"
        ], catalog
        assert all(entry["state"] == "discovered" for entry in catalog["plugins"]), catalog
        assert len(catalog["issues"]) == 1, catalog
        probed = subprocess.run(
            [binary, "--plugin-probe", plugin_probe, "probe-plugins", clap_fixture],
            capture_output=True, text=True, timeout=10,
        )
        assert probed.returncode == 0, (probed.stdout, probed.stderr)
        probe_catalog = json.loads(probed.stdout)
        fixture = probe_catalog["plugins"][0]
        assert fixture["state"] == "discovered", fixture
        assert fixture["descriptors"] == [{
            "features": ["audio-effect", "stereo"],
            "id": "app.nylon.fixture",
            "name": "Fixture Effect",
            "vendor": "Nylon Contributors",
            "version": "1.0",
        }], fixture
        rejected = subprocess.run(
            [binary, "--plugin-probe", plugin_probe, "probe-plugins", str(plugins)],
            capture_output=True, text=True, timeout=10,
        )
        assert rejected.returncode == 0, (rejected.stdout, rejected.stderr)
        rejected_catalog = json.loads(rejected.stdout)
        bad = next(entry for entry in rejected_catalog["plugins"] if entry["name"] == "Gamma")
        assert bad["state"] == "quarantined" and bad["quarantineReason"], bad
        crashed = subprocess.run(
            [binary, "--plugin-probe", plugin_probe, "probe-plugins", crash_fixture],
            capture_output=True, text=True, timeout=10,
        )
        assert crashed.returncode == 0, (crashed.stdout, crashed.stderr)
        crashed_entry = json.loads(crashed.stdout)["plugins"][0]
        assert crashed_entry["state"] == "quarantined", crashed_entry
        assert crashed_entry["quarantineReason"] == "Probe process crashed", crashed_entry
        inspected = subprocess.run(
            [binary, "--plugin-worker", plugin_worker, "plugin-info", clap_fixture,
             "app.nylon.fixture", "48000", "64"],
            capture_output=True, text=True, timeout=10,
        )
        assert inspected.returncode == 0, (inspected.stdout, inspected.stderr)
        plugin_info = json.loads(inspected.stdout)
        assert plugin_info == {
            "inputAudioPorts": 1,
            "inputNotePorts": 1,
            "latencyFrames": 32,
            "ok": True,
            "parameters": [{
                "default": 0.5,
                "flags": 32,
                "id": 7,
                "maximum": 1,
                "minimum": 0,
                "module": "Output",
                "name": "Gain",
            }],
        }, plugin_info
        invalid_info = subprocess.run(
            [binary, "--plugin-worker", plugin_worker, "plugin-info", clap_fixture,
             "app.nylon.fixture", "48000", "8193"],
            capture_output=True, text=True, timeout=10,
        )
        assert invalid_info.returncode != 0
        assert json.loads(invalid_info.stdout)["error"] == "Invalid plugin block size"
        crashed_info = subprocess.run(
            [binary, "--plugin-worker", plugin_worker, "plugin-info", crash_fixture,
             "app.nylon.fixture"],
            capture_output=True, text=True, timeout=10,
        )
        assert crashed_info.returncode != 0
        assert json.loads(crashed_info.stdout)["error"] == "Could not open the isolated plugin"
        processed = root / "processed.wav"
        plugin_render = subprocess.run(
            [binary, "--plugin-worker", plugin_worker, "process-plugin", clap_fixture,
             "app.nylon.fixture", str(source), str(processed), "64"],
            capture_output=True, text=True, timeout=10,
        )
        assert plugin_render.returncode == 0, (plugin_render.stdout, plugin_render.stderr)
        processed_bytes = processed.read_bytes()
        left, right = struct.unpack_from("<ff", processed_bytes, 44)
        expected = 8192 / 32767 * 0.5
        assert abs(left - expected) < 1e-6 and abs(right + expected) < 1e-6
        crashed_output = root / "crashed.wav"
        crash_render = subprocess.run(
            [binary, "--plugin-worker", plugin_worker, "process-plugin", crash_fixture,
             "app.nylon.fixture", str(source), str(crashed_output)],
            capture_output=True, text=True, timeout=10,
        )
        assert crash_render.returncode != 0
        assert json.loads(crash_render.stdout)["error"] == "Plugin worker crashed"
        assert not crashed_output.exists()
        assert not pathlib.Path(str(crashed_output) + ".partial").exists()
        print("Direct project CLI: pass")


if __name__ == "__main__":
    main()
