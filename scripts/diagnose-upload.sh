#!/usr/bin/env bash
# Prints what happened to one upload on a remote deployment
# (docs/plans/2026-10-02-deployment-operating-guide.md §5): when the file
# landed in S3, what DynamoDB holds for it (outcome, attempts, last error),
# then its requests and processing attempts from the logs.
#
# Usage: scripts/diagnose-upload.sh <dev|public> <upload-id> [--since 1d]
# The upload ID is in the page's messages and in scripts/activity-timeline.sh.
set -euo pipefail

env="${1:-}"; upload="${2:-}"; shift 2 || true
if [[ "$env" != dev && "$env" != public ]] \
   || [[ ! "$upload" =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]]; then
  echo "usage: scripts/diagnose-upload.sh <dev|public> <upload-id> [--since 1d]" >&2
  exit 2
fi
since="1d"
if [ "${1:-}" = --since ] && [ -n "${2:-}" ]; then since="$2"; fi
repo="$(cd "$(dirname "$0")/.." && pwd)"
export AWS_PROFILE="${AWS_PROFILE:-timeline}" AWS_REGION="${AWS_REGION:-us-east-1}"
aws sts get-caller-identity --query Account --output text > /dev/null 2>&1 \
  || { echo "AWS sign-in isn't valid. Run: aws login --profile timeline --remote" >&2; exit 1; }

bucket="$(aws cloudformation describe-stacks --stack-name "timeline-$env" \
  --query "Stacks[0].Outputs[?OutputKey=='RawUploadsBucketName'].OutputValue" --output text)"
# The template names the table timeline-conversations-<stage>.
table="timeline-conversations-$env"

echo "== The file in S3 ($bucket)"
landed="$(aws s3api list-objects-v2 --bucket "$bucket" --prefix raw/ \
  --query "Contents[?contains(Key, '$upload')].[Key,LastModified,Size]" --output text)"
user=""
if [ -z "$landed" ] || [ "$landed" = None ]; then
  echo "  no raw/<user>/$upload.json: never uploaded, or deleted since"
else
  read -r key modified size <<< "$landed"
  user="$(cut -d/ -f2 <<< "$key")"
  echo "  $key landed $modified, $size bytes"
fi

echo "== What DynamoDB holds ($table)"
values="{\":a\": {\"S\": \"UPLOAD#$upload\"}, \":b\": {\"S\": \"PROGRESS#$upload\"}"
if [ -n "$user" ]; then
  items="$(aws dynamodb query --table-name "$table" --consistent-read \
    --key-condition-expression "pk = :u" --filter-expression "sk IN (:a, :b)" \
    --expression-attribute-values "$values, \":u\": {\"S\": \"$user\"}}" --output json)"
else
  # Without the file, the user isn't known: look through the whole table.
  items="$(aws dynamodb scan --table-name "$table" --consistent-read \
    --filter-expression "sk IN (:a, :b)" --expression-attribute-values "$values}" --output json)"
fi
ITEMS="$items" python3 - <<'PY'
import json, os
items = json.loads(os.environ["ITEMS"]).get("Items", [])
def plain(value):
    (kind, inner), = value.items()
    return inner if kind != "L" else [plain(v) for v in inner]
if not items:
    print("  no rows: processing never recorded an attempt or an outcome")
for item in sorted(items, key=lambda i: i["sk"]["S"]):
    row = {k: plain(v) for k, v in item.items() if k not in ("pk", "sk")}
    what = "outcome" if item["sk"]["S"].startswith("UPLOAD#") else "attempts"
    print(f"  {what}: " + ", ".join(f"{k} {v}" for k, v in sorted(row.items())))
PY

echo "== Requests and processing attempts (logs, last $since)"
"$repo/scripts/activity-timeline.sh" "$env" --since "$since" --upload "$upload"
