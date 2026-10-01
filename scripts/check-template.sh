#!/usr/bin/env bash
# Checks infra/template.yaml with AWS's own tools before deploying
# (migration plan §V2e, E8): `sam validate --lint` checks the template's
# structure and runs cfn-lint (AWS's CloudFormation checker) on it, which
# catches, among other things, the circular dependency the template's
# bucket-name pattern avoids. Deploys nothing and needs no AWS account.
# Needs the SAM CLI; see infra/README.md.
#
# Usage: scripts/check-template.sh   (uses $AWS_REGION, default us-east-1)
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
if ! command -v sam >/dev/null; then
  echo "The SAM CLI isn't installed; see infra/README.md, step 2." >&2
  exit 1
fi
# SAM sends usage data to AWS unless told not to.
export SAM_CLI_TELEMETRY=0
sam validate --lint --template "$repo/infra/template.yaml" --region "${AWS_REGION:-us-east-1}"
