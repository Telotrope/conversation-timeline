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
out="$repo/frontend/deploy-configs/$stage.json"

outputs="$(aws cloudformation describe-stacks --stack-name "timeline-$stage" \
  --query 'Stacks[0].Outputs' --output json)"

mkdir -p "$(dirname "$out")"
OUTPUTS="$outputs" python3 - "$out" <<'PY'
import json, os, sys
outputs = {o["OutputKey"]: o["OutputValue"] for o in json.loads(os.environ["OUTPUTS"])}
missing = [k for k in ("ApiUrl", "CognitoDomain", "UserPoolClientId") if k not in outputs]
if missing:
    sys.exit(f"the stack has no {', '.join(missing)} output; deploy the current template first")
config = {
    "apiBase": outputs["ApiUrl"],
    "cognitoDomain": outputs["CognitoDomain"],
    "clientId": outputs["UserPoolClientId"],
}
with open(sys.argv[1], "w") as f:
    json.dump(config, f, indent=2)
    f.write("\n")
print(f"wrote {sys.argv[1]}")
PY
echo "Open the page with ?deploy=$stage once, e.g. http://localhost:8000/timeline.html?deploy=$stage"
