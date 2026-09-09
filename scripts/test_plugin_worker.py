"""Exercise persistent CLAP audio processing and process isolation."""

import math
import struct
import subprocess
import sys

HEADER = b"NYWORK1\0"
IDENTIFIER = "app.nylon.fixture"


def request(blocks):
    data = bytearray(HEADER)
    for block in blocks:
        data.extend(struct.pack("<I", len(block)))
        for left, right in block:
            data.extend(struct.pack("<ff", left, right))
    data.extend(struct.pack("<I", 0))
    return bytes(data)


def response(data, lengths):
    assert data[: len(HEADER)] == HEADER
    cursor = len(HEADER)
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
        [(1.0, -1.0), (0.5, -0.25), (0.0, 0.75)],
        [(-0.5, 0.25), (0.125, -0.125)],
    ]
    command = [worker, "clap", fixture, IDENTIFIER, "48000", "64"]
    result = subprocess.run(command, input=request(blocks), capture_output=True, timeout=10)
    assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
    rendered = response(result.stdout, [len(block) for block in blocks])
    for source, processed in zip(blocks, rendered):
        for original, changed in zip(source, processed):
            assert math.isclose(changed[0], original[0] * 0.5, abs_tol=1e-7)
            assert math.isclose(changed[1], original[1] * 0.5, abs_tol=1e-7)

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
