"""Pure retrieval metric functions. No external dependencies."""


def hit_rate(retrieved_ids: list[str], expected_ids: list[str], k: int = 5) -> float:
    """Return 1.0 if any expected doc is in top-k, else 0.0."""
    if not expected_ids or not retrieved_ids or k <= 0:
        return 0.0
    top_k = set(retrieved_ids[:k])
    return 1.0 if top_k.intersection(expected_ids) else 0.0


def mrr(retrieved_ids: list[str], expected_ids: list[str], k: int = 5) -> float:
    """Return reciprocal rank of first expected doc in top-k, 0.0 if none found."""
    if not expected_ids or not retrieved_ids or k <= 0:
        return 0.0
    expected_set = set(expected_ids)
    for rank, doc_id in enumerate(retrieved_ids[:k], start=1):
        if doc_id in expected_set:
            return 1.0 / rank
    return 0.0


def recall(retrieved_ids: list[str], expected_ids: list[str], k: int = 5) -> float:
    """Return fraction of expected docs found in top-k."""
    if not expected_ids or not retrieved_ids or k <= 0:
        return 0.0
    top_k = set(retrieved_ids[:k])
    found = top_k.intersection(expected_ids)
    return len(found) / len(expected_ids)
