"""Main eval entry point.

Usage:
    python run_eval.py --dataset <path> --api-url <url> --token <token>
                       [--retrieval-only] [--full] [--baseline <path>]
                       [--judge-model <model>] [--smoke-test]
                       [--output-dir eval/results]
"""

import argparse
import csv
import json
import os
import sys
import time
from pathlib import Path

import requests

# Add scripts dir to path for local imports
sys.path.insert(0, str(Path(__file__).parent))
from retrieval_metrics import hit_rate, mrr, recall

K = 5


def load_dataset(path: str) -> list[dict]:
    """Load a JSONL dataset file."""
    items = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                items.append(json.loads(line))
    return items


def call_eval_api(api_url: str, token: str, query: str, top_k: int = K) -> dict:
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


def compute_retrieval_scores(retrieved_ids: list[str], expected_ids: list[str]) -> dict:
    """Compute all retrieval metrics for one query."""
    return {
        "hit_rate@5": hit_rate(retrieved_ids, expected_ids, k=K),
        "mrr@5": mrr(retrieved_ids, expected_ids, k=K),
        "recall@5": recall(retrieved_ids, expected_ids, k=K),
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
        resp = call_eval_api(api_url, token, "smoke test query", top_k=K)
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
) -> list[dict]:
    """Run eval on the full dataset and return results rows."""
    results = []
    for i, item in enumerate(dataset):
        qid = item["id"]
        query = item["query"]
        expected_ids = item.get("expectedDocIds", [])
        print(f"[{i + 1}/{len(dataset)}] {qid}: {query}")

        try:
            api_response = call_eval_api(api_url, token, query)
        except requests.HTTPError as e:
            print(f"  ERROR: HTTP {e.response.status_code}")
            row = {
                "id": qid,
                "query": query,
                "hit_rate@5": 0.0,
                "mrr@5": 0.0,
                "recall@5": 0.0,
                "faithfulness": None,
                "response_relevancy": None,
                "error": str(e),
            }
            results.append(row)
            continue

        retrieved_ids = extract_retrieved_ids(api_response)
        scores = compute_retrieval_scores(retrieved_ids, expected_ids)

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


def write_csv(results: list[dict], path: str) -> None:
    """Write results to a CSV file."""
    if not results:
        print("No results to write.")
        return

    fieldnames = [
        "id", "query", "hit_rate@5", "mrr@5", "recall@5",
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


def print_summary(results: list[dict]) -> None:
    """Print aggregate summary of results."""
    if not results:
        return

    n = len(results)
    avg_hit = sum(r["hit_rate@5"] for r in results) / n
    avg_mrr = sum(r["mrr@5"] for r in results) / n
    avg_recall = sum(r["recall@5"] for r in results) / n
    errors = sum(1 for r in results if r.get("error"))

    print(f"\n=== Summary ({n} queries) ===")
    print(f"  HitRate@5:  {avg_hit:.3f}")
    print(f"  MRR@5:      {avg_mrr:.3f}")
    print(f"  Recall@5:   {avg_recall:.3f}")
    if errors:
        print(f"  Errors:     {errors}")


def main() -> int:
    parser = argparse.ArgumentParser(description="RAG eval runner")
    parser.add_argument("--dataset", required=True, help="Path to JSONL golden dataset")
    parser.add_argument("--api-url", required=True, help="Backend API base URL")
    parser.add_argument("--token", required=True, help="Bearer token for auth")
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
    args = parser.parse_args()

    if args.smoke_test:
        ok = smoke_test(args.api_url, args.token)
        return 0 if ok else 1

    dataset = load_dataset(args.dataset)
    print(f"Loaded {len(dataset)} queries from {args.dataset}")

    results = run_eval(
        dataset=dataset,
        api_url=args.api_url,
        token=args.token,
        full=args.full,
        judge_model=args.judge_model,
    )

    os.makedirs(args.output_dir, exist_ok=True)
    output_path = os.path.join(args.output_dir, "current.csv")
    write_csv(results, output_path)

    print_summary(results)

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
