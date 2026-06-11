"""Auto-generate golden dataset from documents uploaded to RWiki.

Usage:
    python generate_dataset.py \
        --config ../backend/config/demo.toml \
        --api-url http://localhost:18080 \
        --output datasets/auto_v1.jsonl
"""

import argparse
import io
import json
import os
import sys
import time
import tomllib
from pathlib import Path

import requests
from openai import OpenAI

if sys.stdout.encoding and sys.stdout.encoding.lower() != "utf-8":
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")


def fetch_documents(api_url: str, token: str) -> list[dict]:
    """Fetch all published documents from the RWiki API."""
    url = f"{api_url.rstrip('/')}/api/documents"
    headers = {"Authorization": f"Bearer {token}"}
    resp = requests.get(url, headers=headers, timeout=30)
    resp.raise_for_status()
    body = resp.json()
    docs = body.get("documents", [])
    return [d for d in docs if d.get("status") == "published"]


def fetch_chunks(api_url: str, token: str, query: str, doc_id: str, top_k: int = 20) -> list[str]:
    """Retrieve chunks for a document via the eval query API, filtered to that doc."""
    url = f"{api_url.rstrip('/')}/api/eval/query"
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
    }
    payload = {"query": query, "topK": top_k}
    resp = requests.post(url, json=payload, headers=headers, timeout=120)
    resp.raise_for_status()
    data = resp.json()

    matching = [
        sr["content"]
        for sr in data.get("searchResults", [])
        if sr.get("documentId") == doc_id and sr.get("content")
    ]
    return matching


def generate_questions(
    client: OpenAI,
    model: str,
    chunks: list[str],
    n: int,
) -> list[str]:
    """Use an LLM to generate realistic user questions from document chunks."""
    context = "\n\n---\n\n".join(chunks)
    prompt = (
        "Below are chunks from a knowledge base document.\n"
        "\n"
        f"<document>\n{context}\n</document>\n"
        "\n"
        f"Generate exactly {n} realistic questions that a user might ask about this content.\n"
        "Requirements:\n"
        "- Questions must be diverse: some factual, some operational (how-to), some comparative.\n"
        "- Sound natural, as if a real user typed them into a search box.\n"
        "- Do NOT quote or paraphrase the source text directly.\n"
        "- Questions should be specific enough that a RAG system would retrieve this document to answer them.\n"
        "- Output ONLY a JSON array of strings, no other text.\n"
        f"- Example format: [\"question 1\", \"question 2\", \"question {n}\"]\n"
    )

    response = client.chat.completions.create(
        model=model,
        messages=[{"role": "user", "content": prompt}],
        temperature=0.7,
        max_tokens=1024,
    )
    text = response.choices[0].message.content.strip()

    # Strip markdown fences if present
    if text.startswith("```"):
        lines = text.split("\n")
        lines = [l for l in lines if not l.startswith("```")]
        text = "\n".join(lines).strip()

    try:
        questions = json.loads(text)
    except json.JSONDecodeError:
        print(f"  WARNING: LLM did not return valid JSON. Raw: {text[:200]}")
        return []

    if not isinstance(questions, list):
        print(f"  WARNING: LLM did not return a list. Got: {type(questions)}")
        return []

    return [q for q in questions if isinstance(q, str) and q.strip()][:n]


def load_llm_config(config_path: str) -> dict:
    """Read [llm] section from a backend TOML config file."""
    with open(config_path, "rb") as f:
        data = tomllib.load(f)
    return data.get("llm", {})


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate golden dataset from RWiki documents")
    parser.add_argument("--config", help="Backend TOML config file (reads [llm] and [api] sections)")
    parser.add_argument("--api-url", default="http://localhost:18080", help="RWiki backend API base URL")
    parser.add_argument("--token", help="Bearer token for auth (or read from config [api] token)")
    parser.add_argument("--output", default=str(Path(__file__).parent.parent / "datasets" / "auto_v1.jsonl"),
                        help="Output JSONL file path")
    parser.add_argument("--questions-per-doc", type=int, default=3, help="Questions to generate per document (default: 3)")
    parser.add_argument("--llm-model", default=None, help="LLM model (overrides config)")
    parser.add_argument("--llm-base-url", default=None, help="LLM API base URL (overrides config)")
    parser.add_argument("--llm-api-key", default=None, help="LLM API key (overrides config)")
    args = parser.parse_args()

    # Load config file if provided
    llm_cfg = {}
    api_cfg = {}
    if args.config:
        if not os.path.exists(args.config):
            print(f"ERROR: config file not found: {args.config}")
            return 1
        with open(args.config, "rb") as f:
            cfg = tomllib.load(f)
        llm_cfg = cfg.get("llm", {})
        api_cfg = cfg.get("api", {})

    # Resolve settings: CLI flag > config file > env var
    api_key = args.llm_api_key or llm_cfg.get("api_key")
    if not api_key:
        print("ERROR: No LLM API key. Provide --config or --llm-api-key.")
        return 1

    base_url = args.llm_base_url or llm_cfg.get("base_url", "https://api.openai.com/v1")
    model = args.llm_model or llm_cfg.get("model", "gpt-4o-mini")
    token = args.token or api_cfg.get("token", "")

    if not token:
        print("ERROR: No API token. Provide --config, --token.")
        return 1

    client = OpenAI(api_key=api_key, base_url=base_url)

    # Fetch documents
    print("Fetching published documents...")
    docs = fetch_documents(args.api_url, token)
    print(f"Found {len(docs)} published document(s)")

    if not docs:
        print("No published documents. Exiting.")
        return 0

    # Ensure output directory exists
    os.makedirs(os.path.dirname(os.path.abspath(args.output)), exist_ok=True)

    counter = 0
    with open(args.output, "w", encoding="utf-8") as f:
        for i, doc in enumerate(docs):
            doc_id = doc["id"]
            file_name = doc.get("fileName") or doc.get("file_name", "")
            stem = Path(file_name).stem if file_name else doc_id
            print(f"\n[{i + 1}/{len(docs)}] Processing: {file_name} ({doc_id})")

            # Retrieve chunks for this document
            try:
                chunks = fetch_chunks(args.api_url, token, stem, doc_id)
            except requests.HTTPError as e:
                print(f"  WARNING: Failed to fetch chunks: HTTP {e.response.status_code}")
                continue

            if not chunks:
                print(f"  WARNING: No chunks found for document {doc_id}, skipping")
                continue

            print(f"  Retrieved {len(chunks)} chunk(s)")

            # Generate questions
            try:
                questions = generate_questions(client, model, chunks, args.questions_per_doc)
            except Exception as e:
                print(f"  WARNING: LLM call failed: {e}")
                continue

            if not questions:
                print(f"  WARNING: No questions generated, skipping")
                continue

            # Write to JSONL
            for q in questions:
                counter += 1
                entry = {
                    "id": f"q{counter:03d}",
                    "query": q,
                    "expectedDocIds": [doc_id],
                }
                f.write(json.dumps(entry, ensure_ascii=False) + "\n")
            print(f"  Generated {len(questions)} question(s)")

            # Be polite to the API
            time.sleep(0.5)

    print(f"\nDone. Wrote {counter} question(s) to {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
