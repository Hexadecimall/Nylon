"""Exercise the local Qt endpoint through the shipped console executable."""
import json
import os
import subprocess
import sys
import time
import uuid


def main():
    endpoint = "nylon-test-" + uuid.uuid4().hex
    environment = dict(os.environ, QT_QPA_PLATFORM="offscreen")
    process = subprocess.Popen(
        [sys.argv[1], "--workspace", "--control", endpoint], env=environment,
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
    )
    try:
        def call(*arguments):
            return subprocess.run(
                [sys.argv[2], "--endpoint", endpoint, *arguments],
                capture_output=True, text=True, timeout=6,
            )

        def reply(*arguments):
            """Runs a command that is expected to succeed and returns its
            reply. A failure names the command, the exit code and whatever
            the client wrote, which is what a bare parse error hid."""
            result = call(*arguments)
            if result.returncode != 0 or not result.stdout.strip():
                raise AssertionError(
                    "{} exited {} with stdout {!r} and stderr {!r}".format(
                        " ".join(arguments), result.returncode, result.stdout, result.stderr))
            try:
                return json.loads(result.stdout)
            except json.JSONDecodeError as error:
                raise AssertionError("{} replied {!r}: {}".format(
                    " ".join(arguments), result.stdout, error)) from None

        for _ in range(30):
            result = call("info")
            if result.returncode == 0:
                break
            if process.poll() is not None:
                raise RuntimeError(process.stderr.read().decode())
            time.sleep(0.1)
        assert result.returncode == 0, result.stderr
        assert reply("info")["tracks"] == 0
        reply("add-track")
        reply("set-tempo", "137")
        state = reply("info")
        assert state["tempo"] == 137 and state["tracks"] == 1, state
        reply("undo")
        assert reply("info")["tempo"] == 120
        reply("view", "arrangement")
        assert reply("info")["view"] == "arrangement"
        for mode in ("maximize", "minimize", "restore"):
            reply("window", mode)
        assert call("set-tempo", "invalid").returncode != 0
        assert call("unsupported").returncode != 0
        print("Qt process control: pass")
    finally:
        process.terminate()
        process.wait(timeout=5)
        process.stderr.close()


if __name__ == "__main__":
    main()
