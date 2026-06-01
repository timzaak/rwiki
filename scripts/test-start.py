#!/usr/bin/env python
"""Start test environment: ensure runtime directories exist and start OTel Collector.

The backend uses SQLite. The OTel Collector container is started for
integration tests that verify OTLP span delivery (gRPC on port 4317).
"""

import sys

from lib import docker
from lib.net import wait_for_tcp
from lib.paths import LOG_DIR, RUNTIME_DIR, SCRIPTS_DIR, ensure_dir

OTEL_CONTAINER = "rwiki-test-otel-collector"
OTEL_IMAGE = "otel/opentelemetry-collector-contrib:0.148.0"
OTEL_GRPC_PORT = 4317
OTEL_CONFIG = SCRIPTS_DIR / "otel-collector-test-config.yaml"


def start_otel_collector() -> bool:
    """Start the OTel Collector container for integration tests."""
    if docker.container_running(OTEL_CONTAINER):
        print(f"  OTel Collector already running ({OTEL_CONTAINER})")
        return True

    if docker.container_exists(OTEL_CONTAINER):
        print(f"  Removing stale OTel Collector container ({OTEL_CONTAINER})")
        docker.rm_force_container(OTEL_CONTAINER)

    ok = docker.run_detached([
        "--name", OTEL_CONTAINER,
        "-p", f"{OTEL_GRPC_PORT}:{OTEL_GRPC_PORT}",
        "-v", f"{OTEL_CONFIG}:/etc/otelcol-contrib/config.yaml:ro",
        OTEL_IMAGE,
    ])
    if not ok:
        return False

    if not wait_for_tcp("127.0.0.1", OTEL_GRPC_PORT, timeout_seconds=15):
        print(f"  ERROR: OTel Collector did not become ready on port {OTEL_GRPC_PORT}", file=sys.stderr)
        return False

    print(f"  OTel Collector ready (gRPC :{OTEL_GRPC_PORT})")
    return True


def main() -> int:
    ensure_dir(LOG_DIR)
    ensure_dir(RUNTIME_DIR)

    if not start_otel_collector():
        print("ERROR: Failed to start OTel Collector", file=sys.stderr)
        return 1

    print("Test environment ready (SQLite + OTel Collector)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
