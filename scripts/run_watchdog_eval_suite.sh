#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACT_DIR="${ROOT_DIR}/artifacts"
REPORT_PATH="${ARTIFACT_DIR}/watchdog-threshold-report.md"

echo "[watchdog-eval] running replay/component eval tests"
pushd "${ROOT_DIR}/src-tauri" >/dev/null
cargo test --quiet evaluate_snapshot_matches_fixture_corpus
cargo test --quiet replay_eval_report_tracks_spam_rate_and_status_regressions
cargo test --quiet replay_eval_matches_fixture_corpus
popd >/dev/null

echo "[watchdog-eval] generating threshold report"
python3 "${ROOT_DIR}/scripts/generate_watchdog_threshold_report.py" \
  --repo-root "${ROOT_DIR}" \
  --output "${REPORT_PATH}"

echo "[watchdog-eval] done -> ${REPORT_PATH}"
