"""Reject known callback hazards in the render kernel."""
import pathlib
import re
import sys

FORBIDDEN = re.compile(
    r"\b(?:Mutex|RwLock|Condvar|Arc|Box|Vec|String|HashMap|BTreeMap|"
    r"thread|sleep|yield_now|spawn|println|eprintln|print|eprint|format|"
    r"malloc|realloc|free|alloc|dealloc|unsafe|extern)\b|"
    r"\bstd::(?:fs|io|net|process|sync)\b|\.clone\s*\("
)


def hazards(source):
    lines = source.splitlines()
    return [number for number, line in enumerate(lines, 1)
            if FORBIDDEN.search(line.split("//", 1)[0])]


def sources():
    """Files that run inside the audio callback."""
    yield pathlib.Path("src/engine.rs")
    for path in sorted(pathlib.Path("src/dsp").glob("*.rs")):
        yield path
    yield pathlib.Path("src/dsp.rs")
    yield pathlib.Path("src/mixer.rs")
    yield pathlib.Path("src/transport.rs")
    yield pathlib.Path("src/engine/playback.rs")
    yield pathlib.Path("src/engine/voice.rs")
    yield pathlib.Path("src/engine/schedule.rs")


OPEN = "// off the audio thread"
CLOSE = "// back on the audio thread"


def strip_tests(source):
    """Drops the test module, which may allocate freely."""
    marker = source.find("#[cfg(test)]")
    return source if marker < 0 else source[:marker]


def strip_control(path, source):
    """Blanks the regions a file marks as control-thread only.

    Setup, publication and the type declarations behind them run before
    or beside playback, never inside the callback, so they are allowed to
    allocate. Each region is blanked rather than removed so that a hit
    still reports the line it is on.
    """
    kept = []
    depth = 0
    for number, line in enumerate(source.splitlines(), 1):
        stripped = line.strip()
        if stripped == OPEN:
            depth += 1
        elif stripped == CLOSE:
            if depth == 0:
                raise SystemExit(f"{path}:{number}: region closed but never opened")
            depth -= 1
        kept.append("" if depth else line)
    if depth:
        raise SystemExit(f"{path}: a control-thread region was never closed")
    return "\n".join(kept)


def main():
    failed = False
    for path in sources():
        if not path.exists():
            continue
        hits = hazards(strip_control(path, strip_tests(path.read_text())))
        for number in hits:
            print(f"{path}:{number}: callback hazard")
            failed = True
    if not failed:
        print("Render kernel checks: pass")
    return failed


if __name__ == "__main__":
    sys.exit(main())
