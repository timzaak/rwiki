#!/usr/bin/env python
"""Start test environment: ensure runtime directories exist and start test containers.

The backend defaults to SQLite; the OTel Collector container serves integration
tests that verify OTLP span delivery (gRPC on port 4317). The PostgreSQL
(pgvector) container serves the dual-backend storage matrix (RWIKI_TEST_STORAGE=postgres).
"""

import sys

from lib import docker
from lib.net import wait_for_tcp
from lib.paths import LOG_DIR, RUNTIME_DIR, SCRIPTS_DIR, ensure_dir

OTEL_CONTAINER = "rwiki-test-otel-collector"
OTEL_IMAGE = "otel/opentelemetry-collector-contrib:0.148.0"
OTEL_GRPC_PORT = 4317
OTEL_CONFIG = SCRIPTS_DIR / "otel-collector-test-config.yaml"

PG_CONTAINER = "rwiki-test-postgres"
PG_IMAGE = "pgvector/pgvector:pg17"
PG_PORT = 54329
PG_PASSWORD = "test"
PG_DB = "rwiki_test"


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


def ensure_pg_database() -> bool:
    """Create the rwiki_test base database (per-test tag databases are created
    by the Rust test factory)."""
    code, err = docker.exec_check(
        PG_CONTAINER,
        ["env", f"PGPASSWORD={PG_PASSWORD}", "psql", "-U", "postgres", "-d", "postgres",
         "-c", f"CREATE DATABASE {PG_DB}"],
    )
    # 0 = created; duplicate_database = already exists — both fine
    return code == 0 or "already exists" in err


def start_postgres() -> bool:
    """Start the PostgreSQL (pgvector) container for the dual-backend matrix."""
    if docker.container_running(PG_CONTAINER):
        print(f"  PostgreSQL already running ({PG_CONTAINER})")
        return True

    if docker.container_exists(PG_CONTAINER):
        print(f"  Removing stale PostgreSQL container ({PG_CONTAINER})")
        docker.rm_force_container(PG_CONTAINER)

    ok = docker.run_detached([
        "--name", PG_CONTAINER,
        "-e", f"POSTGRES_PASSWORD={PG_PASSWORD}",
        "-p", f"127.0.0.1:{PG_PORT}:5432",
        PG_IMAGE,
    ])
    if not ok:
        return False

    if not wait_for_tcp("127.0.0.1", PG_PORT, timeout_seconds=30):
        print(f"  ERROR: PostgreSQL did not become ready on port {PG_PORT}", file=sys.stderr)
        return False

    # TCP up ≠ accepting queries; poll pg_isready until the server answers.
    if not docker.wait_pg_ready(PG_CONTAINER, "postgres"):
        print("  ERROR: PostgreSQL did not answer pg_isready probes", file=sys.stderr)
        return False

    if not ensure_pg_database():
        print(f"  ERROR: failed to create {PG_DB} database", file=sys.stderr)
        return False

    print(f"  PostgreSQL ready (pgvector :{PG_PORT}, db={PG_DB})")
    return True


def main() -> int:
    ensure_dir(LOG_DIR)
    ensure_dir(RUNTIME_DIR)

    if not start_otel_collector():
        print("ERROR: Failed to start OTel Collector", file=sys.stderr)
        return 1

    if not start_postgres():
        print("ERROR: Failed to start PostgreSQL test container", file=sys.stderr)
        return 1

    print("Test environment ready (SQLite + PostgreSQL + OTel Collector)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
