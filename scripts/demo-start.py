#!/usr/bin/env python
"""Start demo environment: backend (SQLite) + frontend."""

import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request

from lib import docker
from lib.cli import require_executable
from lib.net import wait_for_http_ok
from lib.paths import LOG_DIR, REPO_ROOT, ensure_dir
from lib.proc import spawn_background


BACKEND_PORT = int(os.environ.get("BACKEND_PORT", "18080"))
FRONTEND_PORT = int(os.environ.get("FRONTEND_PORT", "3000"))
DOCS_WEB_PORT = int(os.environ.get("DOCS_WEB_PORT", "3001"))

TUTORIALS_DIR = REPO_ROOT / "docs-web" / "content" / "docs"
RWIKI_URL = "https://rwiki.fornetcode.com"


def _inject_link_frontmatter(file_bytes: bytes, slug: str) -> bytes:
    """Inject or update the `link` field in frontmatter with the docs site URL."""
    text = file_bytes.decode("utf-8-sig")
    doc_link = f"{RWIKI_URL}/docs/{slug}"

    if text.startswith("---"):
        # Already has frontmatter — find closing ---
        end = text.find("\n---", 3)
        if end != -1:
            fm = text[: end + 4]
            rest = text[end + 4 :]
            # Replace existing link or add it before closing ---
            if "link:" in fm:
                lines = fm.splitlines()
                lines = [
                    (
                        f"link: {doc_link}"
                        if line.strip().startswith("link:")
                        else line
                    )
                    for line in lines
                ]
                fm = "\n".join(lines) + "\n"
            else:
                fm = fm.rstrip() + f"\nlink: {doc_link}\n"
            return (fm + rest).encode("utf-8")

    # No frontmatter — add one
    frontmatter = f"---\nlink: {doc_link}\n---\n"
    return (frontmatter + text).encode("utf-8")


def _get_published_filenames(base_url: str, api_token: str) -> set[str]:
    """Fetch all published document filenames from the API."""
    try:
        req = urllib.request.Request(
            f"{base_url}/api/documents",
            headers={"Authorization": f"Bearer {api_token}"},
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read())
        docs = data if isinstance(data, list) else data.get("documents", [])
        return {d.get("fileName") for d in docs if d.get("status") == "published"} - {None}
    except Exception:
        return set()


def _upload_and_publish_markdown(
    base_url: str, api_token: str, md_path: "pathlib.Path"
) -> bool:
    """Upload a single .md file and publish it."""
    t0 = time.time()
    filename = md_path.name

    boundary = "----PythonFormBoundary"
    with open(md_path, "rb") as f:
        file_bytes = f.read()

    slug = md_path.stem  # e.g. "getting-started" from "getting-started.mdx"
    file_bytes = _inject_link_frontmatter(file_bytes, slug)

    body = (
        f"--{boundary}\r\n"
        f'Content-Disposition: form-data; name="file"; filename="{filename}"\r\n'
        f"Content-Type: text/markdown; charset=utf-8\r\n"
        f"\r\n"
    ).encode() + file_bytes + f"\r\n--{boundary}--\r\n".encode()

    req = urllib.request.Request(
        f"{base_url}/api/documents/upload",
        data=body,
        headers={
            "Content-Type": f"multipart/form-data; boundary={boundary}",
            "Authorization": f"Bearer {api_token}",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = json.loads(resp.read())
        doc_id = data.get("id")
        row_count = data.get("rowCount", "?")
        print(f"  {filename}: uploaded (id={doc_id}, {row_count} rows)")
    except Exception as e:
        print(f"  {filename}: ERROR upload failed: {e}")
        return False

    if data.get("status") == "failed":
        print(f"  {filename}: ERROR indexing failed: {data.get('errorMessage', '')}")
        return False

    pub_req = urllib.request.Request(
        f"{base_url}/api/documents/{doc_id}/publish",
        data=b"",
        headers={"Authorization": f"Bearer {api_token}"},
        method="PATCH",
    )
    try:
        with urllib.request.urlopen(pub_req, timeout=10) as resp:
            json.loads(resp.read())
        elapsed = time.time() - t0
        print(f"  {filename}: published ({elapsed:.1f}s)")
    except Exception as e:
        print(f"  {filename}: WARNING publish failed: {e}")
        return False

    return True


def _upload_and_publish_openapi(
    base_url: str, api_token: str, json_path: "pathlib.Path"
) -> bool:
    """Upload an OpenAPI JSON file and publish it."""
    t0 = time.time()
    filename = json_path.name

    boundary = "----PythonFormBoundaryJSON"
    with open(json_path, "rb") as f:
        file_bytes = f.read()

    body = (
        f"--{boundary}\r\n"
        f'Content-Disposition: form-data; name="file"; filename="{filename}"\r\n'
        f"Content-Type: application/json\r\n"
        f"\r\n"
    ).encode() + file_bytes + f"\r\n--{boundary}--\r\n".encode()

    req = urllib.request.Request(
        f"{base_url}/api/documents/upload",
        data=body,
        headers={
            "Content-Type": f"multipart/form-data; boundary={boundary}",
            "Authorization": f"Bearer {api_token}",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = json.loads(resp.read())
        doc_id = data.get("id")
        row_count = data.get("rowCount", "?")
        print(f"  {filename}: uploaded (id={doc_id}, {row_count} endpoints)")
    except Exception as e:
        print(f"  {filename}: ERROR upload failed: {e}")
        return False

    if data.get("status") == "failed":
        print(f"  {filename}: ERROR indexing failed: {data.get('errorMessage', '')}")
        return False

    pub_req = urllib.request.Request(
        f"{base_url}/api/documents/{doc_id}/publish",
        data=b"",
        headers={"Authorization": f"Bearer {api_token}"},
        method="PATCH",
    )
    try:
        with urllib.request.urlopen(pub_req, timeout=10) as resp:
            json.loads(resp.read())
        elapsed = time.time() - t0
        print(f"  {filename}: published ({elapsed:.1f}s)")
    except Exception as e:
        print(f"  {filename}: WARNING publish failed: {e}")
        return False

    return True


def seed_tutorials(base_url: str) -> bool:
    """Upload tutorial .mdx files from docs-web/content/docs/."""
    if not TUTORIALS_DIR.exists():
        print(f"SKIP: tutorials directory not found: {TUTORIALS_DIR}")
        return True

    md_files = sorted(
        f for f in TUTORIALS_DIR.glob("*.mdx") if f.stem != "index"
    )
    if not md_files:
        print("SKIP: no .mdx files found in docs directory")
        return True

    api_token = os.environ.get("API_TOKEN", "demo-token")
    published = _get_published_filenames(base_url, api_token)

    to_upload = [f for f in md_files if f.name not in published]
    if not to_upload:
        print(f"SKIP: all {len(md_files)} tutorials already published")
        return True

    skipped = len(md_files) - len(to_upload)
    if skipped:
        print(f"SKIP: {skipped} tutorials already published")

    print(f"Seeding {len(to_upload)} tutorials ...")
    failures = 0
    for md_path in to_upload:
        if not _upload_and_publish_markdown(base_url, api_token, md_path):
            failures += 1

    if failures:
        print(f"WARNING: {failures}/{len(to_upload)} tutorials failed to upload")
        return False

    print(f"All {len(to_upload)} tutorials seeded successfully")
    return True


def seed_openapi(base_url: str) -> bool:
    """Upload the OpenAPI spec JSON to the document system for RAG."""
    openapi_path = REPO_ROOT / "docs-web" / "openapi.json"
    if not openapi_path.exists():
        print("SKIP: openapi.json not found (backend may not expose it)")
        return True

    api_token = os.environ.get("API_TOKEN", "demo-token")
    published = _get_published_filenames(base_url, api_token)

    filename = openapi_path.name
    if filename in published:
        print(f"SKIP: {filename} already published")
        return True

    print(f"Seeding OpenAPI spec ({filename}) ...")
    return _upload_and_publish_openapi(base_url, api_token, openapi_path)


def main() -> int:
    ensure_dir(LOG_DIR)
    backend_log = LOG_DIR / "backend-demo.log"
    frontend_log = LOG_DIR / "frontend-demo.log"

    # Backend
    cargo = require_executable("cargo")
    env = os.environ.copy()
    env["SERVER_PORT"] = str(BACKEND_PORT)
    env["APP_CONFIG"] = "config/demo.toml"

    spawn_background(
        name="demo-backend",
        command=[cargo, "run"],
        cwd=REPO_ROOT / "backend",
        stdout_path=backend_log,
        env=env,
    )

    if not wait_for_http_ok(f"http://127.0.0.1:{BACKEND_PORT}/health", 60):
        print("ERROR: Demo backend failed to start")
        return 1

    # Seed tutorial documents
    seed_ok = seed_tutorials(f"http://127.0.0.1:{BACKEND_PORT}")
    if not seed_ok:
        print("WARNING: tutorial seed failed, continuing without seed data")

    # Frontend
    npm = require_executable("npm", windows_fallback="npm.cmd")

    # Build chat widget JS and place into backend/static/ for backend to serve
    print("Building chat widget...")
    subprocess.run(
        [npm, "run", "build:widget"],
        cwd=REPO_ROOT / "frontend",
        check=True,
    )
    static_dir = REPO_ROOT / "backend" / "static"
    static_dir.mkdir(exist_ok=True)
    shutil.copy2(
        REPO_ROOT / "frontend" / "dist" / "rwiki-chat.js",
        static_dir / "rwiki-chat.js",
    )

    fe_env = os.environ.copy()
    fe_env["VITE_API_BASE_URL"] = f"http://localhost:{BACKEND_PORT}"
    spawn_background(
        name="demo-frontend",
        command=[npm, "run", "dev"],
        cwd=REPO_ROOT / "frontend",
        stdout_path=frontend_log,
        env=fe_env,
    )

    # Docs-web: fetch latest OpenAPI spec from running backend
    openapi_path = REPO_ROOT / "docs-web" / "openapi.json"
    try:
        req = urllib.request.Request(f"http://127.0.0.1:{BACKEND_PORT}/api-docs/openapi.json")
        with urllib.request.urlopen(req, timeout=10) as resp:
            openapi_json = resp.read()
        openapi_path.write_bytes(openapi_json)
        print(f"OpenAPI spec saved to {openapi_path}")
    except Exception as e:
        print(f"WARNING: failed to fetch OpenAPI spec: {e}")

    # Upload OpenAPI spec to document system for RAG
    seed_openapi_ok = seed_openapi(f"http://127.0.0.1:{BACKEND_PORT}")
    if not seed_openapi_ok:
        print("WARNING: OpenAPI spec seed failed, continuing without API docs in RAG")

    docs_log = LOG_DIR / "docs-web-demo.log"
    docs_env = os.environ.copy()
    docs_env["VITE_RWIKI_API_URL"] = f"http://localhost:{BACKEND_PORT}"
    spawn_background(
        name="demo-docs-web",
        command=[npm, "run", "dev"],
        cwd=REPO_ROOT / "docs-web",
        stdout_path=docs_log,
        env=docs_env,
    )

    print(
        f"Demo environment started. "
        f"Frontend=http://localhost:{FRONTEND_PORT} "
        f"Docs=http://localhost:{DOCS_WEB_PORT} "
        f"Backend=http://localhost:{BACKEND_PORT} "
        f"Logs={backend_log},{frontend_log},{docs_log}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
