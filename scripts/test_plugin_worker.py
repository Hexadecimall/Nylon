"""Exercise persistent CLAP audio processing and process isolation."""

import math
import struct
import subprocess
import sys

HEADER = b"NYWORK2\0"
IDENTIFIER = "app.nylon.fixture"


def request(blocks):
    data = bytearray(HEADER)
    for block, events in blocks:
        data.extend(struct.pack("<I", len(block)))
        data.extend(struct.pack("<I", len(events)))
        for offset, identifier, value in events:
            data.extend(struct.pack("<IId", offset, identifier, value))
        for left, right in block:
            data.extend(struct.pack("<ff", left, right))
    data.extend(struct.pack("<I", 0))
    return bytes(data)


def response(data, lengths):
    assert data[: len(HEADER)] == HEADER
    cursor = len(HEADER)
    latency, parameter_count = struct.unpack_from("<II", data, cursor)
    cursor += 8
    assert latency == 32
    assert parameter_count == 1
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
    for expected in lengths:
        frames = struct.unpack_from("<I", data, cursor)[0]
        cursor += 4
        assert frames == expected
        block = []
        for _ in range(frames):
            block.append(struct.unpack_from("<ff", data, cursor))
            cursor += 8
        blocks.append(block)
    assert cursor == len(data)
    return blocks


def main():
    if len(sys.argv) != 4:
        raise SystemExit("expected worker, fixture, and crash fixture")
    worker, fixture, crash_fixture = sys.argv[1:]
    blocks = [
        ([(1.0, -1.0), (0.5, -0.25), (0.0, 0.75)], [(1, 7, 0.25)]),
        ([(-0.5, 0.25), (0.125, -0.125)], []),
    ]
    command = [worker, "clap", fixture, IDENTIFIER, "48000", "64"]
    result = subprocess.run(command, input=request(blocks), capture_output=True, timeout=10)
    assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
    rendered = response(result.stdout, [len(block) for block, _ in blocks])
    expected_gains = [[0.5, 0.25, 0.25], [0.25, 0.25]]
    for (source, _), processed, gains in zip(blocks, rendered, expected_gains):
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
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
