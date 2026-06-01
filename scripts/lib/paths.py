from pathlib import Path
import os
import subprocess


def _resolve_repo_root() -> Path:
    """Resolve the project root directory.

    Priority:
    1. AI_PROJECT_ROOT override for intentional shared-script use
    2. git rev-parse --show-toplevel anchored to this scripts tree
    3. Walk this scripts tree upward for CLAUDE.md / .git marker
    4. parents[2] relative to this file
    """
    override = os.environ.get("CLAUDE_PROJECT_DIR")
    if override:
        return Path(override).expanduser().resolve()

    override = os.environ.get("AI_PROJECT_ROOT")
    if override:
        return Path(override).expanduser().resolve()

    script_project_root = Path(__file__).resolve().parents[2]

    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=script_project_root,
            capture_output=True, text=True, timeout=5,
        )
        if result.returncode == 0:
            git_root = Path(result.stdout.strip()).resolve()
            if git_root.is_dir():
                return git_root
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    for parent in [script_project_root, *script_project_root.parents]:
        if (parent / "CLAUDE.md").is_file() or (parent / ".git").exists():
            return parent

    return script_project_root


REPO_ROOT = _resolve_repo_root()
SCRIPTS_DIR = Path(__file__).resolve().parents[1]
LOG_DIR = REPO_ROOT / "log"
RUNTIME_DIR = LOG_DIR / "runtime"
TEST_CONFIG_DIR = RUNTIME_DIR / "test-config"
BACKEND_TEST_LOG = LOG_DIR / "backend-test-output.log"


def ensure_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path
