"""Repository text and commit validation."""
import base64
import pathlib
import re
import subprocess
import sys

TOKENS = base64.b64decode("Q2xhdWRlCkFudGhyb3BpYwpzdXBlcnBvd2VycwphZ2VudHMKc2tpbGxzCnBsYW5zCnNwZWNzCkNoYXRHUFQKT3BlbkFJCkNvcGlsb3QKQ28tQXV0aG9yZWQtQnkKR2VuZXJhdGVkIHdpdGgKR2lkZW9uCmdpZGVvbmNveApvdXIKd2UKdXMKbGV0J3MKYXdlc29tZQphbWF6aW5nCm1hZ2ljCmRlbGlnaHRmdWwKYmxhemluZwpzdXBlcmNoYXJnZWQKam91cm5leQpOWFJUCkVtYmVyRHJhZ29uCkxMQUNMCm5leGRvCnB1bGxpbwpIeXByTWFjClJWUk1hY2hpbmUKQ2VyYmVydXMKTHVtZW4KSXJpcwpBdGxhcwpUaXRhbgpGYWJsZQpBcmdvbgpUT0RPCkZJWE1FCnVuaW1wbGVtZW50ZWQhCnRvZG8hCkNvZGV4CmFzc2lzdGFudApHQwpHUFQKR2VtaW5pCkFJLXRvb2xpbmc=").decode().splitlines()
WORD = re.compile(r"(?<![A-Za-z0-9])(?:" + "|".join(map(re.escape, TOKENS)) + r")(?![A-Za-z0-9])", re.I)
PATH = re.compile(r"/(?:U[s]ers|h[o]me|V[o]lumes)/|[A-Z]:\\|[~]/")
MARK = re.compile("[\U0001f000-\U0001faff\u2600-\u27bf\ufe0f]")
DIRECTORY = re.compile(r"(?:^|/)(?:[.]clau" + r"de|[.]cursor|[.]aider[^/]*|[.]ag" + r"ents)(?:/|$)")

def check(label, content):
    failed = False
    for number, line in enumerate(content.splitlines(), 1):
        if WORD.search(line) or PATH.search(line) or MARK.search(line) or DIRECTORY.search(line):
            print(f"{label}:{number}: prohibited text")
            failed = True
    return failed

def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--message":
        content = pathlib.Path(sys.argv[2]).read_text()
        subject = content.splitlines()[0] if content.splitlines() else ""
        return check("commit", content) or not subject or len(subject) > 72
    staged = "--staged" in sys.argv
    names = [p.as_posix() for p in pathlib.Path(".").rglob("*")
             if not set(p.parts) & {".git", "target"}]
    failed = False
    if staged:
        patch = subprocess.check_output(["ghx", "diff", "--staged"]).decode()
        added = "\n".join(line[1:] for line in patch.splitlines() if line.startswith("+"))
        failed |= check("staged", added)
    for name in sorted(filter(None, names)):
        failed |= check("filename", name)
        path = pathlib.Path(name)
        if path.is_symlink():
            print(f"{name}: symbolic link rejected")
            failed = True
            continue
        if not path.is_file():
            continue
        content = path.read_bytes()
        failed |= check(name, content.decode("utf-8", errors="replace"))
    if not failed:
        print("Repository rules: pass")
    return failed

if __name__ == "__main__":
    sys.exit(bool(main()))
