#!/usr/bin/env python
"""Stop test environment: stop and remove the OTel Collector container."""

import sys

from lib import docker

OTEL_CONTAINER = "rwiki-test-otel-collector"


def main() -> int:
    if docker.container_exists(OTEL_CONTAINER):
        docker.stop_container(OTEL_CONTAINER)
        docker.rm_force_container(OTEL_CONTAINER)
        print(f"OTel Collector stopped and removed ({OTEL_CONTAINER})")
    else:
        print(f"OTel Collector container not found ({OTEL_CONTAINER}), nothing to stop")

    print("Test environment stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
