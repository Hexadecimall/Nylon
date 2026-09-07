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


def main():
    path = pathlib.Path("src/engine.rs")
    hits = hazards(path.read_text())
    for number in hits:
        print(f"{path}:{number}: callback hazard")
    if not hits:
        print("Render kernel checks: pass")
    return bool(hits)


if __name__ == "__main__":
    sys.exit(main())
