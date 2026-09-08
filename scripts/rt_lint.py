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


def strip_tests(source):
    """Drops the test module, which may allocate freely."""
    marker = source.find("#[cfg(test)]")
    return source if marker < 0 else source[:marker]


def main():
    failed = False
    for path in sources():
        if not path.exists():
            continue
        hits = hazards(strip_tests(path.read_text()))
        for number in hits:
            print(f"{path}:{number}: callback hazard")
            failed = True
    if not failed:
        print("Render kernel checks: pass")
    return failed


if __name__ == "__main__":
    sys.exit(main())
