#!/usr/bin/env bash
# Prints one timeline of what the page did, what reached the API and what
# our Lambda functions did, from the stage's CloudWatch log groups (plan
# docs/plans/completed/2026-10-02-activity-instrumentation.md §7).
#
# Usage: scripts/activity-timeline.sh <stage> [--since 30m|2h|1d] [--session <id>]
#
# Times are UTC. Needs the `aws` command signed in to the stage's account.
# A missing log group is named on stderr and the others are still read.
set -euo pipefail
exec python3 "$(dirname "${BASH_SOURCE[0]}")/activity_timeline.py" "$@"
