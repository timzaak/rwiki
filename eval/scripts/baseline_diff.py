"""Compare current eval results against a baseline CSV.

Exit code 1 if HitRate@5 regresses beyond threshold (default 10%).
Exit code 0 otherwise.

Usage:
    python baseline_diff.py --current results/current.csv --baseline results/baseline.csv [--threshold 0.1]
"""

import argparse
import sys

import pandas as pd


P0_METRICS = ["hit_rate@5"]

AGGREGATE_METRICS = ["hit_rate@5", "mrr@5", "recall@5"]


def load_csv(path: str) -> pd.DataFrame:
    """Load a results CSV into a DataFrame."""
    return pd.read_csv(path)


def compute_aggregates(df: pd.DataFrame) -> dict[str, float]:
    """Compute mean of each aggregate metric column."""
    result = {}
    for col in AGGREGATE_METRICS:
        if col in df.columns:
            result[col] = df[col].mean()
    return result


def compare_aggregates(
    current_agg: dict[str, float],
    baseline_agg: dict[str, float],
    threshold: float,
) -> list[dict]:
    """Compare aggregate metrics, returning rows for the diff table."""
    rows = []
    for metric in AGGREGATE_METRICS:
        cur = current_agg.get(metric)
        base = baseline_agg.get(metric)
        if cur is None or base is None:
            rows.append({
                "metric": metric,
                "baseline": base,
                "current": cur,
                "delta": None,
                "pct_change": None,
                "regressed": False,
            })
            continue

        delta = cur - base
        pct_change = delta / base if base != 0 else float("inf")
        regressed = metric in P0_METRICS and delta < -threshold

        rows.append({
            "metric": metric,
            "baseline": round(base, 4),
            "current": round(cur, 4),
            "delta": round(delta, 4),
            "pct_change": f"{pct_change:+.1%}",
            "regressed": regressed,
        })
    return rows


def print_diff_table(rows: list[dict]) -> None:
    """Print a human-readable diff table."""
    print(f"{'Metric':<16} {'Baseline':>10} {'Current':>10} {'Delta':>10} {'Change':>10} {'Status':>10}")
    print("-" * 70)
    for r in rows:
        status = "REGRESS" if r["regressed"] else "ok"
        print(
            f"{r['metric']:<16} "
            f"{str(r['baseline']):>10} "
            f"{str(r['current']):>10} "
            f"{str(r['delta']):>10} "
            f"{str(r['pct_change']):>10} "
            f"{status:>10}"
        )


def compare_per_query(current: pd.DataFrame, baseline: pd.DataFrame) -> list[dict]:
    """Compare metrics per query, returning rows for per-query diff."""
    rows = []
    for metric in AGGREGATE_METRICS:
        if metric not in current.columns or metric not in baseline.columns:
            continue
        merged = current[["id", metric]].merge(
            baseline[["id", metric]],
            on="id",
            suffixes=("_current", "_baseline"),
        )
        for _, row in merged.iterrows():
            cur = row[f"{metric}_current"]
            base = row[f"{metric}_baseline"]
            delta = cur - base
            rows.append({
                "id": row["id"],
                "metric": metric,
                "baseline": round(base, 4),
                "current": round(cur, 4),
                "delta": round(delta, 4),
            })
    return rows


def print_per_query_table(rows: list[dict]) -> None:
    """Print a per-query diff table."""
    if not rows:
        return
    print("\nPer-query breakdown:")
    print(f"{'ID':<8} {'Metric':<16} {'Baseline':>10} {'Current':>10} {'Delta':>10}")
    print("-" * 58)
    for r in rows:
        print(
            f"{r['id']:<8} "
            f"{r['metric']:<16} "
            f"{r['baseline']:>10} "
            f"{r['current']:>10} "
            f"{r['delta']:>10}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description="Compare eval results against baseline")
    parser.add_argument("--current", required=True, help="Path to current results CSV")
    parser.add_argument("--baseline", required=True, help="Path to baseline results CSV")
    parser.add_argument(
        "--threshold",
        type=float,
        default=0.1,
        help="Max allowed regression for P0 metrics (default: 0.1 = 10%%)",
    )
    args = parser.parse_args()

    current_df = load_csv(args.current)
    baseline_df = load_csv(args.baseline)

    current_agg = compute_aggregates(current_df)
    baseline_agg = compute_aggregates(baseline_df)

    agg_rows = compare_aggregates(current_agg, baseline_agg, args.threshold)
    print("=== Aggregate comparison ===")
    print_diff_table(agg_rows)

    per_query_rows = compare_per_query(current_df, baseline_df)
    print_per_query_table(per_query_rows)

    any_regressed = any(r["regressed"] for r in agg_rows)
    if any_regressed:
        print("\nFAILED: P0 metric regression detected.")
        return 1

    print("\nPASSED: No P0 metric regression.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
