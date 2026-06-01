#!/usr/bin/env python
"""Run Playwright tests in debug mode with verbose logging."""

import os
import subprocess
import sys

from lib.paths import REPO_ROOT, SCRIPTS_DIR


def main() -> int:
    test_file = sys.argv[1] if len(sys.argv) > 1 else ""
    args = [sys.executable, str(SCRIPTS_DIR / "demo-test-runner.py")]
    if test_file:
        args.append(test_file)
    args.extend(["--log-level", "verbose"])

    env = dict(os.environ)
    env["DEBUG"] = "pw:api,pw:network"
    env["PLAYWRIGHT_TRACE"] = "on"

    print("=== Playwright Debug Mode ===")
    print(f"Test file: {test_file}")
    print("Mode: fast")
    print("DEBUG: pw:api,pw:network")
    print("TRACE: on")
    print("Log level: verbose")
    print("")
    print("Running test...")
    print("")
    result = subprocess.run(args, env=env, cwd=str(REPO_ROOT))
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
