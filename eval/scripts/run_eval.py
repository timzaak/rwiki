"""Main eval entry point.

Usage:
    python run_eval.py --dataset <path> --api-url <url> --token <token>
                       [--retrieval-only] [--full] [--baseline <path>]
                       [--judge-model <model>] [--smoke-test]
                       [--output-dir eval/results]
"""

import argparse
import csv
import io
import json
import os
import sys
import time
import tomllib
from pathlib import Path

if sys.stdout.encoding and sys.stdout.encoding.lower() != "utf-8":
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")

import requests

# Add scripts dir to path for local imports
sys.path.insert(0, str(Path(__file__).parent))
from retrieval_metrics import hit_rate, mrr, recall

DEFAULT_TOP_K = 5


def load_dataset(path: str) -> list[dict]:
    """Load a dataset file (.jsonl or .csv).

    JSONL format: one JSON object per line with keys
        id, query, expectedDocIds
    CSV format: columns query, filename (semicolon-separated)
        id is auto-generated as q001, q002, ...
    """
    ext = Path(path).suffix.lower()
    if ext == ".csv":
        return _load_csv(path)
    # Default: treat as JSONL
    return _load_jsonl(path)


def _load_jsonl(path: str) -> list[dict]:
    """Load JSONL dataset."""
    items = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                items.append(json.loads(line))
    return items


def _load_csv(path: str) -> list[dict]:
    """Load CSV dataset with auto-generated IDs."""
    items = []
    with open(path, encoding="utf-8", newline="") as f:
        reader = csv.DictReader(f)
        for i, row in enumerate(reader, start=1):
            filename_val = row.get("filename", "").strip()
            filenames = [fn.strip() for fn in filename_val.split(";") if fn.strip()]
            item = {
                "id": f"q{i:03d}",
                "query": row["query"].strip(),
            }
            if filenames:
                item["filename"] = filenames
            items.append(item)
    return items


def resolve_filenames(dataset: list[dict], api_url: str, token: str) -> None:
    """Resolve filename references to document UUIDs via the API.

    For items that have a ``filename`` field (from CSV input), fetches the
    document list from the API, builds a name-to-id mapping, and replaces
    ``filename`` with ``expectedDocIds`` (list of UUID strings).

    Raises SystemExit if any filename has no matching document.
    """
    needs_resolution = [item for item in dataset if "filename" in item]
    if not needs_resolution:
        return

    url = f"{api_url.rstrip('/')}/api/documents"
    headers = {"Authorization": f"Bearer {token}"}
    resp = requests.get(url, headers=headers, timeout=30)
    resp.raise_for_status()

    docs = resp.json().get("documents", [])
    name_to_id: dict[str, str] = {doc.get("fileName") or doc.get("file_name", ""): doc["id"] for doc in docs}

    for item in needs_resolution:
        resolved = []
        for fn in item["filename"]:
            if fn not in name_to_id:
                print(
                    f"ERROR: filename '{fn}' not found in document list. "
                    f"Available: {sorted(name_to_id.keys())}",
                    file=sys.stderr,
                )
                sys.exit(1)
            resolved.append(name_to_id[fn])
        item["expectedDocIds"] = resolved
        del item["filename"]


def call_eval_api(api_url: str, token: str, query: str, top_k: int = DEFAULT_TOP_K) -> dict:
    """POST to /api/eval/query and return the response JSON."""
    url = f"{api_url.rstrip('/')}/api/eval/query"
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
    }
    payload = {"query": query, "topK": top_k}
    resp = requests.post(url, json=payload, headers=headers, timeout=120)
    resp.raise_for_status()
    return resp.json()


def extract_retrieved_ids(response: dict) -> list[str]:
    """Extract document IDs from search results."""
    return [r["documentId"] for r in response.get("searchResults", [])]


def compute_retrieval_scores(retrieved_ids: list[str], expected_ids: list[str], k: int = DEFAULT_TOP_K) -> dict:
    """Compute all retrieval metrics for one query."""
    return {
        f"hit_rate@{k}": hit_rate(retrieved_ids, expected_ids, k=k),
        f"mrr@{k}": mrr(retrieved_ids, expected_ids, k=k),
        f"recall@{k}": recall(retrieved_ids, expected_ids, k=k),
    }


def compute_ragas_metrics(query: str, answer: str, contexts: list[str], judge_model: str) -> dict:
    """Compute Faithfulness and Response Relevancy via Ragas."""
    try:
        from datasets import Dataset
        from ragas import evaluate
        from ragas.metrics import faithfulness, response_relevancy
    except ImportError:
        print("WARNING: ragas not installed. Skipping full eval metrics.")
        return {"faithfulness": None, "response_relevancy": None}

    data = {
        "question": [query],
        "answer": [answer],
        "contexts": [contexts],
        "ground_truth": [""],
    }
    dataset = Dataset.from_dict(data)

    result = evaluate(
        dataset,
        metrics=[faithfulness, response_relevancy],
        llm=judge_model,
    )
    scores = result.to_pandas().iloc[0].to_dict()
    return {
        "faithfulness": scores.get("faithfulness"),
        "response_relevancy": scores.get("response_relevancy"),
    }


def smoke_test(api_url: str, token: str) -> bool:
    """Call the eval API once and verify response structure."""
    print("Running smoke test...")
    try:
        resp = call_eval_api(api_url, token, "smoke test query")
    except Exception as e:
        print(f"FAIL: API call failed: {e}")
        return False

    required_fields = ["query", "searchResults", "answer"]
    missing = [f for f in required_fields if f not in resp]
    if missing:
        print(f"FAIL: Missing required fields: {missing}")
        return False

    for sr in resp.get("searchResults", []):
        sr_required = ["documentId", "content", "score"]
        sr_missing = [f for f in sr_required if f not in sr]
        if sr_missing:
            print(f"FAIL: searchResult missing fields: {sr_missing}")
            return False

    print(
        f"PASS: API returned {len(resp.get('searchResults', []))} results, "
        f"answer length={len(resp.get('answer', ''))}"
    )
    return True


def run_eval(
    dataset: list[dict],
    api_url: str,
    token: str,
    full: bool,
    judge_model: str,
    top_k: int = DEFAULT_TOP_K,
) -> list[dict]:
    """Run eval on the full dataset and return results rows."""
    results = []
    for i, item in enumerate(dataset):
        qid = item["id"]
        query = item["query"]
        expected_ids = item.get("expectedDocIds", [])
        print(f"[{i + 1}/{len(dataset)}] {qid}: {query}")

        try:
            api_response = call_eval_api(api_url, token, query, top_k=top_k)
        except requests.HTTPError as e:
            print(f"  ERROR: HTTP {e.response.status_code}")
            row = {
                "id": qid,
                "query": query,
                f"hit_rate@{top_k}": 0.0,
                f"mrr@{top_k}": 0.0,
                f"recall@{top_k}": 0.0,
                "faithfulness": None,
                "response_relevancy": None,
                "error": str(e),
            }
            results.append(row)
            continue

        retrieved_ids = extract_retrieved_ids(api_response)
        scores = compute_retrieval_scores(retrieved_ids, expected_ids, k=top_k)

        row = {
            "id": qid,
            "query": query,
            **scores,
            "answer": api_response.get("answer", ""),
            "retrieved_count": len(retrieved_ids),
        }

        if full:
            contexts = [sr["content"] for sr in api_response.get("searchResults", [])]
            answer = api_response.get("answer", "")
            ragas_scores = compute_ragas_metrics(query, answer, contexts, judge_model)
            row.update(ragas_scores)
        else:
            row.update({"faithfulness": None, "response_relevancy": None})

        row["error"] = ""
        results.append(row)

        # Be polite to the API
        time.sleep(0.5)

    return results


def write_csv(results: list[dict], path: str, top_k: int = DEFAULT_TOP_K) -> None:
    """Write results to a CSV file."""
    if not results:
        print("No results to write.")
        return

    fieldnames = [
        "id", "query",
        f"hit_rate@{top_k}", f"mrr@{top_k}", f"recall@{top_k}",
        "retrieved_count", "answer", "faithfulness", "response_relevancy", "error",
    ]
    # Ensure all keys present
    for row in results:
        for key in fieldnames:
            row.setdefault(key, "")

    with open(path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(results)
    print(f"Results written to {path}")


def print_summary(results: list[dict], top_k: int = DEFAULT_TOP_K) -> None:
    """Print aggregate summary of results."""
    if not results:
        return

    n = len(results)
    avg_hit = sum(r[f"hit_rate@{top_k}"] for r in results) / n
    avg_mrr = sum(r[f"mrr@{top_k}"] for r in results) / n
    avg_recall = sum(r[f"recall@{top_k}"] for r in results) / n
    errors = sum(1 for r in results if r.get("error"))

    print(f"\n=== Summary ({n} queries) ===")
    print(f"  HitRate@{top_k}:  {avg_hit:.3f}")
    print(f"  MRR@{top_k}:      {avg_mrr:.3f}")
    print(f"  Recall@{top_k}:   {avg_recall:.3f}")
    if errors:
        print(f"  Errors:     {errors}")


def main() -> int:
    parser = argparse.ArgumentParser(description="RAG eval runner")
    parser.add_argument("--config", help="Backend TOML config file (reads [api] token)")
    parser.add_argument("--dataset", help="Path to JSONL or CSV golden dataset")
    parser.add_argument("--api-url", default="http://localhost:18080", help="Backend API base URL")
    parser.add_argument("--token", help="Bearer token for auth (or read from config [api] token)")
    parser.add_argument(
        "--retrieval-only",
        action="store_true",
        default=True,
        help="Only compute retrieval metrics (default)",
    )
    parser.add_argument(
        "--full",
        action="store_true",
        help="Also compute Ragas Faithfulness and Response Relevancy",
    )
    parser.add_argument("--baseline", help="Path to baseline CSV for regression check")
    parser.add_argument(
        "--judge-model",
        default="gpt-4o-mini",
        help="LLM model for Ragas judge (default: gpt-4o-mini)",
    )
    parser.add_argument("--smoke-test", action="store_true", help="Run smoke test only")
    parser.add_argument(
        "--output-dir",
        default=str(Path(__file__).parent.parent / "results"),
        help="Output directory for results CSV",
    )
    parser.add_argument(
        "--top-k",
        type=int,
        default=DEFAULT_TOP_K,
        help=f"Number of results to retrieve (default: {DEFAULT_TOP_K})",
    )
    args = parser.parse_args()

    # Resolve token from config if not provided via CLI
    token = args.token
    cfg: dict = {}
    if not token and args.config:
        if not os.path.exists(args.config):
            print(f"ERROR: config file not found: {args.config}", file=sys.stderr)
            return 1
        with open(args.config, "rb") as f:
            cfg = tomllib.load(f)
        token = cfg.get("api", {}).get("token", "")
    if not token:
        print("ERROR: No API token. Provide --config or --token.", file=sys.stderr)
        return 1

    # For full mode, expose LLM API key for Ragas via env var
    if args.full and not os.environ.get("OPENAI_API_KEY"):
        llm_cfg = cfg.get("llm", {}) if cfg else {}
        if args.config and not llm_cfg:
            with open(args.config, "rb") as f:
                llm_cfg = tomllib.load(f).get("llm", {})
        api_key = llm_cfg.get("api_key")
        if api_key:
            os.environ["OPENAI_API_KEY"] = api_key
        base_url = llm_cfg.get("base_url")
        if base_url:
            os.environ["OPENAI_BASE_URL"] = base_url

    if args.smoke_test:
        ok = smoke_test(args.api_url, token)
        return 0 if ok else 1

    if not args.dataset:
        parser.error("--dataset is required when not running --smoke-test")

    dataset = load_dataset(args.dataset)
    print(f"Loaded {len(dataset)} queries from {args.dataset}")

    # Resolve filenames to document IDs if needed (CSV input)
    resolve_filenames(dataset, args.api_url, token)

    results = run_eval(
        dataset=dataset,
        api_url=args.api_url,
        token=token,
        full=args.full,
        judge_model=args.judge_model,
        top_k=args.top_k,
    )

    os.makedirs(args.output_dir, exist_ok=True)
    output_path = os.path.join(args.output_dir, "current.csv")
    write_csv(results, output_path, top_k=args.top_k)

    print_summary(results, top_k=args.top_k)

    if args.baseline:
        import subprocess

        print(f"\nComparing against baseline: {args.baseline}")
        ret = subprocess.call(
            [
                sys.executable,
                str(Path(__file__).parent / "baseline_diff.py"),
                "--current", output_path,
                "--baseline", args.baseline,
            ]
        )
        return ret

    return 0


if __name__ == "__main__":
    sys.exit(main())
