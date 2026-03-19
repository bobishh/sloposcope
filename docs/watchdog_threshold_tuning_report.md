# Watchdog Threshold Tuning Report

Generated at: `2026-03-10T01:05:26Z`

## Baseline Summary

- Replay fixture files: **1**
- Total evaluated traces: **3**
- Runs with issues: **2** (`66.67%`)
- Status-change rate: **`33.33%`**
- Raw issues / trace: **`0.67`**
- Deduped issues / trace: **`0.33`**
- Dedupe gain: **`50.00%`**
- Total unique issue fingerprints: **1**

## Suggested Guard Thresholds

- `max_status_change_rate`: `43.33%` (max fixture rate + 10pp safety margin)
- `max_deduped_issues_per_trace`: `0.58` (max fixture density + buffer)
- `target_mean_deduped_issues_per_trace`: `0.33`

## Fixture Breakdown

| Fixture | Traces | Runs with issues | Status changes | Raw issues | Deduped issues | Unique fingerprints |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| missing_validation_dedupe.json (missing_validation_dedupe) | 3 | 2 | 1 | 2 | 1 | 1 |

## Notes

- This report is generated from replay fixture expectations, not production telemetry.
- Use this baseline for alert-threshold changes and CI regression review.

