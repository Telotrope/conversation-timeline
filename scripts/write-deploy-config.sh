#!/usr/bin/env bash
# Writes frontend/deploy-configs/<stage>.json from the deployed stack's
# outputs, so the page can use that deployment: open
# timeline.html?deploy=<stage> once (migration plan §V2e, E5). The file is
# ignored by git; it describes one person's deployment.
#
# Usage: scripts/write-deploy-config.sh <stage>   (stack timeline-<stage>;
# uses your usual AWS settings, e.g. AWS_PROFILE)
set -euo pipefail

stage="${1:?usage: scripts/write-deploy-config.sh <stage>}"
if [[ ! "$stage" =~ ^[a-z0-9-]{1,32}$ ]]; then
  echo "\"$stage\" is not a stage name (lowercase letters, digits and hyphens)." >&2
  exit 1
fi
repo="$(cd "$(dirname "$0")/.." && pwd)"
# DEPLOY_CONFIG_DIR exists for scripts/test-deploy-scripts.sh, so the test
# never overwrites your real settings.
out="${DEPLOY_CONFIG_DIR:-$repo/frontend/deploy-configs}/$stage.json"

outputs="$(aws cloudformation describe-stacks --stack-name "timeline-$stage" \
  --query 'Stacks[0].Outputs' --output json)"

# The page's code version, stamped on every activity record so what the page
# showed can be read from its code (docs/plans/2026-10-02-activity-
# instrumentation.md §4). "-dirty" marks uncommitted changes.
if ! page_version="$(git -C "$repo" describe --always --dirty 2>&1)"; then
  echo "could not read the page's version from git ($page_version); records will say \"unknown\"" >&2
  page_version="unknown"
fi

mkdir -p "$(dirname "$out")"
OUTPUTS="$outputs" PAGE_VERSION="$page_version" python3 - "$out" <<'PY'
import json, os, sys
outputs = {o["OutputKey"]: o["OutputValue"] for o in json.loads(os.environ["OUTPUTS"])}
missing = [k for k in ("ApiUrl", "CognitoDomain", "UserPoolClientId") if k not in outputs]
if missing:
    sys.exit(f"the stack has no {', '.join(missing)} output; deploy the current template first")
config = {
    "apiBase": outputs["ApiUrl"],
    "cognitoDomain": outputs["CognitoDomain"],
    "clientId": outputs["UserPoolClientId"],
    # Whether the page records what the user does (docs/plans/
    # 2026-10-02-activity-instrumentation.md §4). A stack deployed before
    # the RecordActivity output existed records nothing.
    "recordActivity": outputs.get("RecordActivity") == "on",
    "pageVersion": os.environ["PAGE_VERSION"],
}
with open(sys.argv[1], "w") as f:
    json.dump(config, f, indent=2)
    f.write("\n")
print(f"wrote {sys.argv[1]}")
PY
echo "Open your page (the stack's FrontendUrl) with ?deploy=$stage added, once."
