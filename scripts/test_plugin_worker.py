"""Exercise persistent CLAP audio processing and process isolation."""

import math
import struct
import subprocess
import sys

HEADER = b"NYWORK4\0"
IDENTIFIER = "app.nylon.fixture"
SAVE_STATE = (1 << 32) - 1
LOAD_STATE = SAVE_STATE - 1


def request(operations):
    data = bytearray(HEADER)
    for operation in operations:
        if operation[0] == "save":
            data.extend(struct.pack("<I", SAVE_STATE))
        elif operation[0] == "load":
            state = operation[1]
            data.extend(struct.pack("<IQ", LOAD_STATE, len(state)))
            data.extend(state)
        else:
            _, block, parameter_events, note_events = operation
            data.extend(
                struct.pack("<III", len(block), len(parameter_events), len(note_events))
            )
            for offset, identifier, value in parameter_events:
                data.extend(struct.pack("<IId", offset, identifier, value))
            for event in note_events:
                data.extend(struct.pack("<IIihhhd", *event))
            for left, right in block:
                data.extend(struct.pack("<ff", left, right))
    data.extend(struct.pack("<I", 0))
    return bytes(data)


def response(data, operations):
    assert data[: len(HEADER)] == HEADER
    cursor = len(HEADER)
    latency, parameter_count, audio_inputs, note_ports = struct.unpack_from(
        "<IIII", data, cursor
    )
    cursor += 16
    assert latency == 32
    assert parameter_count == 1
    assert audio_inputs == 1
    assert note_ports == 1
    identifier, flags, minimum, maximum, default = struct.unpack_from(
        "<IIddd", data, cursor
    )
    cursor += struct.calcsize("<IIddd")
    name_length = struct.unpack_from("<H", data, cursor)[0]
    cursor += 2
    name = data[cursor : cursor + name_length].decode()
    cursor += name_length
    module_length = struct.unpack_from("<H", data, cursor)[0]
    cursor += 2
    module = data[cursor : cursor + module_length].decode()
    cursor += module_length
    assert (identifier, flags, minimum, maximum, default, name, module) == (
        7,
        1 << 5,
        0.0,
        1.0,
        0.5,
        "Gain",
        "Output",
    )
    blocks = []
    saved_states = []
    for operation in operations:
        if operation[0] == "save":
            command, length = struct.unpack_from("<IQ", data, cursor)
            cursor += 12
            assert command == SAVE_STATE
            saved_states.append(data[cursor : cursor + length])
            cursor += length
            continue
        if operation[0] == "load":
            command = struct.unpack_from("<I", data, cursor)[0]
            cursor += 4
            assert command == LOAD_STATE
            continue
        expected = len(operation[1])
        frames = struct.unpack_from("<I", data, cursor)[0]
        cursor += 4
        assert frames == expected
        block = []
        for _ in range(frames):
            block.append(struct.unpack_from("<ff", data, cursor))
            cursor += 8
        blocks.append(block)
    assert cursor == len(data)
    return blocks, saved_states


def main():
    if len(sys.argv) != 4:
        raise SystemExit("expected worker, fixture, and crash fixture")
    worker, fixture, crash_fixture = sys.argv[1:]
    operations = [
        (
            "block",
            [(1.0, -1.0), (0.5, -0.25), (1.0, 0.75)],
            [(1, 7, 0.25)],
            [(2, 0, 41, 0, 2, 60, 0.6)],
        ),
        ("save",),
        ("block", [(-0.5, 0.25)], [(0, 7, 0.75)], []),
        ("load", struct.pack("<d", 0.6)),
        ("block", [(0.125, -0.125)], [], []),
    ]
    command = [worker, "clap", fixture, IDENTIFIER, "48000", "64"]
    result = subprocess.run(command, input=request(operations), capture_output=True, timeout=10)
    assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
    rendered, states = response(result.stdout, operations)
    assert len(states) == 1
    assert math.isclose(struct.unpack("<d", states[0])[0], 0.6, abs_tol=1e-12)
    source_blocks = [operation[1] for operation in operations if operation[0] == "block"]
    expected_gains = [[0.5, 0.25, 0.6], [0.75], [0.6]]
    for source, processed, gains in zip(source_blocks, rendered, expected_gains):
        for original, changed, gain in zip(source, processed, gains):
            assert math.isclose(changed[0], original[0] * gain, abs_tol=1e-7)
            assert math.isclose(changed[1], original[1] * gain, abs_tol=1e-7)

    invalid = subprocess.run(
        [worker, "clap", fixture, "app.nylon.missing", "48000", "64"],
        input=HEADER,
        capture_output=True,
        timeout=10,
    )
    assert invalid.returncode != 0

    crashed = subprocess.run(
        [worker, "clap", crash_fixture, IDENTIFIER, "48000", "64"],
        input=HEADER,
        capture_output=True,
        timeout=10,
    )
    assert crashed.returncode != 0

    invalid_note = subprocess.run(
        command,
        input=request([("block", [(1.0, 1.0)], [], [(0, 0, 9, 0, 16, 60, 0.5)])]),
        capture_output=True,
        timeout=10,
    )
    assert invalid_note.returncode != 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
