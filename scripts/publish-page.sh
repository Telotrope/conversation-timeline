#!/usr/bin/env bash
# Publishes the page to a stack deployed with HostPage=on
# (docs/plans/2026-10-02-page-hosting.md §3): copies the page's files into a
# temporary folder, with timeline.html renamed index.html and naming the
# deployment itself (§2), adds the deployment's settings file, uploads it all
# to the stack's page bucket, and clears CloudFront's copies.
#
# Usage: scripts/publish-page.sh <stage>   (stack timeline-<stage>;
# uses your usual AWS settings, e.g. AWS_PROFILE)
set -euo pipefail

stage="${1:?usage: scripts/publish-page.sh <stage>}"
if [[ ! "$stage" =~ ^[a-z0-9-]{1,32}$ ]]; then
  echo "\"$stage\" is not a stage name (lowercase letters, digits and hyphens)." >&2
  exit 1
fi
repo="$(cd "$(dirname "$0")/.." && pwd)"
staged="$(mktemp -d)"
trap 'rm -rf "$staged"' EXIT

outputs="$(aws cloudformation describe-stacks --stack-name "timeline-$stage" \
  --query 'Stacks[0].Outputs' --output json)"
page="$(OUTPUTS="$outputs" python3 - <<'PY'
import json, os, sys
outputs = {o["OutputKey"]: o["OutputValue"] for o in json.loads(os.environ["OUTPUTS"])}
missing = [k for k in ("PageBucketName", "PageDistributionId", "PageUrl") if k not in outputs]
if missing:
    sys.exit(f"the stack has no {', '.join(missing)} output; deploy it with HostPage=on first")
print(outputs["PageBucketName"], outputs["PageDistributionId"], outputs["PageUrl"])
PY
)"
read -r bucket distribution page_url <<< "$page"

# The page's files, from an explicit list: nothing else in the repository is
# published.
python3 - "$repo/timeline.html" "$staged/index.html" "$stage" <<'PY'
import sys
source, target, stage = sys.argv[1:]
page = open(source, encoding="utf-8").read()
if 'name="timeline-deploy"' in page:
    sys.exit(f"{source} already has a timeline-deploy tag; only the published copy may have one")
charset = '<meta charset="UTF-8">\n'
if page.count(charset) != 1:
    sys.exit(f"{source} has no single {charset.strip()} line to put the tag after")
tag = f'<meta name="timeline-deploy" content="{stage}">\n'
open(target, "w", encoding="utf-8").write(page.replace(charset, charset + tag))
PY
(cd "$repo" && find frontend \( -name '*.js' -o -name '*.css' \) -not -path 'frontend/tests/*' -not -path '*/node_modules/*' -print0 \
  | xargs -0 -I{} install -D -m 644 {} "$staged/{}")
cp -r "$repo/vendor" "$staged/vendor"
DEPLOY_CONFIG_DIR="$staged/frontend/deploy-configs" "$repo/scripts/write-deploy-config.sh" "$stage" > /dev/null

# Each file is sent with its type given, not guessed: browsers refuse to run
# a module script sent as anything but JavaScript. The five groups cover
# every staged file (checked here), and each sync's --delete removes only
# that group's leftovers.
unknown="$(cd "$staged" && find . -type f -not -name '*.js' -not -name '*.css' -not -name '*.json' -not -name '*.html' \
  -not -name '*.md' -not -name 'LICENSE' -not -name 'LICENSE-*')"
if [ -n "$unknown" ]; then
  echo "No content type is set for these files; add one to scripts/publish-page.sh:" >&2
  echo "$unknown" >&2
  exit 1
fi
publish() {
  local type="$1"; shift
  aws s3 sync "$staged" "s3://$bucket" --delete --cache-control no-cache \
    --exclude '*' "$@" --content-type "$type" --only-show-errors
}
publish 'text/javascript; charset=utf-8' --include '*.js'
publish 'text/css; charset=utf-8' --include '*.css'
publish 'application/json' --include '*.json'
publish 'text/html; charset=utf-8' --include '*.html'
publish 'text/plain; charset=utf-8' --include '*.md' --include '*/LICENSE' --include '*/LICENSE-*'

aws cloudfront create-invalidation --distribution-id "$distribution" --paths '/*' > /dev/null
echo "Published to $page_url"
