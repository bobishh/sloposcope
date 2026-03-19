#!/usr/bin/env python3
"""Generate a watchdog replay-threshold baseline report from replay fixtures."""

from __future__ import annotations

import argparse
import datetime as dt
import json
from pathlib import Path
from statistics import mean


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate watchdog threshold baseline report from replay fixtures."
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="Repository root path (default: inferred from script location).",
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Output markdown report path.",
    )
    return parser.parse_args()


def load_replay_cases(fixture_dir: Path) -> list[dict]:
    if not fixture_dir.is_dir():
        raise FileNotFoundError(f"Replay fixture directory is missing: {fixture_dir}")

    cases: list[dict] = []
    for path in sorted(fixture_dir.glob("*.json")):
        payload = json.loads(path.read_text())
        expected = payload["expected"]
        traces_evaluated = int(
            expected.get("traces_evaluated", expected.get("tracesEvaluated", 0))
        )
        runs_with_issues = int(
            expected.get("runs_with_issues", expected.get("runsWithIssues", 0))
        )
        traces_with_status_change = int(
            expected.get(
                "traces_with_status_change", expected.get("tracesWithStatusChange", 0)
            )
        )
        issues_total_raw = int(
            expected.get("issues_total_raw", expected.get("issuesTotalRaw", 0))
        )
        issues_total_deduped = int(
            expected.get(
                "issues_total_deduped", expected.get("issuesTotalDeduped", 0)
            )
        )
        unique_issue_fingerprints = int(
            expected.get(
                "unique_issue_fingerprints",
                expected.get("uniqueIssueFingerprints", 0),
            )
        )
        traces = payload.get("traces", [])
        cases.append(
            {
                "file": path.name,
                "name": payload.get("name", path.stem),
                "traces_count": len(traces),
                "traces_evaluated": traces_evaluated,
                "runs_with_issues": runs_with_issues,
                "traces_with_status_change": traces_with_status_change,
                "issues_total_raw": issues_total_raw,
                "issues_total_deduped": issues_total_deduped,
                "unique_issue_fingerprints": unique_issue_fingerprints,
            }
        )
    if not cases:
        raise RuntimeError(f"No replay fixtures found in: {fixture_dir}")
    return cases


def ratio(numerator: int, denominator: int) -> float:
    if denominator <= 0:
        return 0.0
    return numerator / denominator


def build_markdown(cases: list[dict], generated_at: str) -> str:
    total_traces = sum(c["traces_evaluated"] for c in cases)
    total_runs_with_issues = sum(c["runs_with_issues"] for c in cases)
    total_status_changes = sum(c["traces_with_status_change"] for c in cases)
    total_issues_raw = sum(c["issues_total_raw"] for c in cases)
    total_issues_deduped = sum(c["issues_total_deduped"] for c in cases)
    total_unique_fingerprints = sum(c["unique_issue_fingerprints"] for c in cases)

    status_change_rate = ratio(total_status_changes, total_traces)
    issue_run_rate = ratio(total_runs_with_issues, total_traces)
    raw_issues_per_trace = ratio(total_issues_raw, total_traces)
    deduped_issues_per_trace = ratio(total_issues_deduped, total_traces)
    dedupe_gain = 1.0 - ratio(total_issues_deduped, max(total_issues_raw, 1))

    per_case_status_change = [ratio(c["traces_with_status_change"], c["traces_evaluated"]) for c in cases]
    per_case_deduped_issues = [ratio(c["issues_total_deduped"], c["traces_evaluated"]) for c in cases]

    lines: list[str] = []
    lines.append("# Watchdog Threshold Tuning Report")
    lines.append("")
    lines.append(f"Generated at: `{generated_at}`")
    lines.append("")
    lines.append("## Baseline Summary")
    lines.append("")
    lines.append(f"- Replay fixture files: **{len(cases)}**")
    lines.append(f"- Total evaluated traces: **{total_traces}**")
    lines.append(f"- Runs with issues: **{total_runs_with_issues}** (`{issue_run_rate:.2%}`)")
    lines.append(f"- Status-change rate: **`{status_change_rate:.2%}`**")
    lines.append(f"- Raw issues / trace: **`{raw_issues_per_trace:.2f}`**")
    lines.append(f"- Deduped issues / trace: **`{deduped_issues_per_trace:.2f}`**")
    lines.append(f"- Dedupe gain: **`{dedupe_gain:.2%}`**")
    lines.append(f"- Total unique issue fingerprints: **{total_unique_fingerprints}**")
    lines.append("")
    lines.append("## Suggested Guard Thresholds")
    lines.append("")
    lines.append(
        f"- `max_status_change_rate`: `{(max(per_case_status_change) + 0.10):.2%}` (max fixture rate + 10pp safety margin)"
    )
    lines.append(
        f"- `max_deduped_issues_per_trace`: `{(max(per_case_deduped_issues) + 0.25):.2f}` (max fixture density + buffer)"
    )
    lines.append(
        f"- `target_mean_deduped_issues_per_trace`: `{mean(per_case_deduped_issues):.2f}`"
    )
    lines.append("")
    lines.append("## Fixture Breakdown")
    lines.append("")
    lines.append(
        "| Fixture | Traces | Runs with issues | Status changes | Raw issues | Deduped issues | Unique fingerprints |"
    )
    lines.append(
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"
    )
    for case in cases:
        lines.append(
            "| "
            + f"{case['file']} ({case['name']})"
            + f" | {case['traces_evaluated']}"
            + f" | {case['runs_with_issues']}"
            + f" | {case['traces_with_status_change']}"
            + f" | {case['issues_total_raw']}"
            + f" | {case['issues_total_deduped']}"
            + f" | {case['unique_issue_fingerprints']}"
            + " |"
        )
    lines.append("")
    lines.append("## Notes")
    lines.append("")
    lines.append("- This report is generated from replay fixture expectations, not production telemetry.")
    lines.append("- Use this baseline for alert-threshold changes and CI regression review.")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    repo_root: Path = args.repo_root.resolve()
    fixture_dir = repo_root / "src-tauri" / "testdata" / "watchdog_replay_cases"
    output_path: Path = args.output.resolve()

    cases = load_replay_cases(fixture_dir)
    generated_at = (
        dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    )
    report = build_markdown(cases, generated_at)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(report + "\n")
    print(f"[watchdog-report] wrote {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
