#!/usr/bin/env python
"""Stop demo environment."""

import os
import sys
import time

from lib.paths import LOG_DIR
from lib.proc import kill_process_by_port, wait_process_exit


def main() -> int:
    # Kill processes by port
    backend_port = int(os.environ.get("BACKEND_PORT", "18080"))
    frontend_port = int(os.environ.get("FRONTEND_PORT", "3000"))
    docs_web_port = int(os.environ.get("DOCS_WEB_PORT", "3001"))

    for port in (backend_port, frontend_port, docs_web_port):
        kill_process_by_port(port)

    # Give processes time to release file handles
    time.sleep(1)

    # Clean up demo logs
    if LOG_DIR.exists():
        for pattern in ("backend-demo.log*", "frontend-demo.log*", "docs-web-demo.log*"):
            for f in LOG_DIR.glob(pattern):
                try:
                    f.unlink(missing_ok=True)
                except PermissionError:
                    print(f"Warning: could not delete {f} (still locked)", file=sys.stderr)

    print("Demo environment stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
