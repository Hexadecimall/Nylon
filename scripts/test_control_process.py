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

        for _ in range(30):
            result = call("info")
            if result.returncode == 0:
                break
            if process.poll() is not None:
                raise RuntimeError(process.stderr.read().decode())
            time.sleep(0.1)
        assert result.returncode == 0, result.stderr
        assert json.loads(result.stdout)["tracks"] == 0
        assert call("add-track").returncode == 0
        assert call("set-tempo", "137").returncode == 0
        state = json.loads(call("info").stdout)
        assert state["tempo"] == 137 and state["tracks"] == 1, state
        assert call("undo").returncode == 0
        assert json.loads(call("info").stdout)["tempo"] == 120
        assert call("view", "arrangement").returncode == 0
        assert json.loads(call("info").stdout)["view"] == "arrangement"
        for mode in ("maximize", "minimize", "restore"):
            assert call("window", mode).returncode == 0
        assert call("set-tempo", "invalid").returncode != 0
        assert call("unsupported").returncode != 0
        print("Qt process control: pass")
    finally:
        process.terminate()
        process.wait(timeout=5)
        process.stderr.close()


if __name__ == "__main__":
    main()
