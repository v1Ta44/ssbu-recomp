"""Summarize delay-parameter evidence. This does not measure input-to-screen latency."""
import argparse
from collections import Counter
import csv
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def summarize(path):
    with path.open(encoding="utf-8-sig", newline="") as stream:
        rows = list(csv.DictReader(line for line in stream if line.strip() and not line.startswith("#")))
    counts = Counter(row["decision"] for row in rows)
    by_selection = {}
    for row in rows:
        selection = row["selected"]
        item = by_selection.setdefault(selection, dict(calls=0, applied=0, original_values=set(), effective_values=set()))
        item["calls"] += 1
        item["applied"] += row["decision"] == "applied"
        item["original_values"].add(int(row["automatic"]))
        item["effective_values"].add(int(row["effective"]))
    for item in by_selection.values():
        item["original_values"] = sorted(item["original_values"])
        item["effective_values"] = sorted(item["effective_values"])
    return dict(trace=str(path.resolve()), calls=len(rows), decisions=dict(counts), selections=by_selection,
                observed_ldn_states=sorted({int(row["ldn"]) for row in rows}),
                observed_node_counts=sorted({int(row["nodes"]) for row in rows}),
                callers=sorted({row["caller"] for row in rows}),
                limitation="Parameter observations only; original function can reuse prior values. No RTT, end-to-end latency or desync measurement.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", nargs="?", type=Path, help="delay.csv, a run directory, or omit for the most recent run")
    args = parser.parse_args()
    path = args.trace
    if path is None:
        found = sorted((ROOT / "build/latency-slider/runs").glob("*/delay.csv"))
        if not found:
            parser.error("No experiment trace found")
        path = found[-1]
    if path.is_dir():
        path = path / "delay.csv"
    result = summarize(path)
    output = path.with_name(path.stem + ".summary.json")
    output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Calculator calls: {result['calls']}")
    for decision, count in result["decisions"].items():
        print(f"  {decision}: {count}")
    for selected, item in result["selections"].items():
        print(f"  selection {selected}: calls={item['calls']}, applied={item['applied']}, original={item['original_values']}, effective={item['effective_values']}")
    print("These are parameter records, not a latency measurement.")
    print(f"Saved: {output}")
    if not result["calls"]:
        print("No calculator calls were observed. Start/join a game local-wireless session; room-server membership alone is insufficient.")


if __name__ == "__main__":
    main()
