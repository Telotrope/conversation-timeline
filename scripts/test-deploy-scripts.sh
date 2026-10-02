#!/usr/bin/env bash
#
# Tests write-deploy-config.sh, aws-dev-token.sh and publish-page.sh against
# a stand-in for the `aws` command, which records what it was asked and
# answers with canned output. Run manually:
#
#   scripts/test-deploy-scripts.sh
#
# The canned answers follow the AWS CLI's documented output shapes; they
# were not captured from a real deployment. The first real deployment runs
# both scripts for real (migration plan §V2e, checks D3 and the
# walkthrough in infra/README.md).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
pass() { echo "  PASS: $1"; }
fail() { echo "  FAIL: $1" >&2; failures=$((failures + 1)); }

# The stand-in `aws`: logs its arguments, then answers from
# $work/answer-<subcommand>.
#
# For `cognito-idp`, it reads the file named by --cli-input-json twice and
# refuses an empty second read the way the real one does: aws-cli 2.37.8
# appears to open that file twice, so a pipe arrives empty the second time
# (docs/analysis/2026-10-02-deployment-checks-status.md, check D3). It records
# the request, the file's path and its permissions, and answers with an error
# instead of a token while $work/fail-cognito-idp exists.
#
# For `s3 sync`, it also keeps a copy of the first folder it is asked to
# upload (publish-page.sh deletes its own afterward), in $work/synced.
mkdir -p "$work/bin"
cat > "$work/bin/aws" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$STUB_DIR/argv.log"
if [ "$1" = "cognito-idp" ]; then
  path=""; prev=""
  for arg in "$@"; do [ "$prev" = "--cli-input-json" ] && path="${arg#file://}"; prev="$arg"; done
  first_read="$(cat "$path")"
  if [ -z "$first_read" ] || [ -z "$(cat "$path")" ]; then
    echo "aws: [ERROR]: An error occurred (ParamValidation): Error parsing parameter 'cli-input-json': Invalid JSON received." >&2
    exit 252
  fi
  cat "$path" > "$STUB_DIR/request.log"
  echo "$path" > "$STUB_DIR/request-path.log"
  stat -c %a "$path" > "$STUB_DIR/request-mode.log"
  if [ -e "$STUB_DIR/fail-cognito-idp" ]; then
    echo "aws: [ERROR]: An error occurred (NotAuthorizedException) when calling the InitiateAuth operation: Incorrect username or password." >&2
    exit 254
  fi
fi
if [ "$1" = "s3" ] && [ "$2" = "sync" ] && [ ! -e "$STUB_DIR/synced" ]; then
  cp -r "$3" "$STUB_DIR/synced"
fi
cat "$STUB_DIR/answer-$1"
STUB
chmod +x "$work/bin/aws"
export STUB_DIR="$work" PATH="$work/bin:$PATH" DEPLOY_CONFIG_DIR="$work/configs"

echo "write-deploy-config.sh"
cat > "$work/answer-cloudformation" <<'JSON'
[
  {"OutputKey": "ApiUrl", "OutputValue": "https://abc123.execute-api.us-east-1.amazonaws.com", "Description": "Base URL of the deployed HTTP API."},
  {"OutputKey": "UserPoolId", "OutputValue": "us-east-1_AbCdEf123"},
  {"OutputKey": "UserPoolClientId", "OutputValue": "4hj2k3l4m5n6o7p8q9r0s1t2u3"},
  {"OutputKey": "CognitoDomain", "OutputValue": "https://timeline-dev-123456789012.auth.us-east-1.amazoncognito.com"},
  {"OutputKey": "RawUploadsBucketName", "OutputValue": "timeline-uploads-dev-123456789012"}
]
JSON
if "$REPO_ROOT/scripts/write-deploy-config.sh" dev > "$work/out.txt"; then
  pass "succeeds with the stack's outputs"
else
  fail "failed with the stack's outputs: $(cat "$work/out.txt")"
fi
grep -q -- "--stack-name timeline-dev" "$work/argv.log" && pass "asks for stack timeline-dev" \
  || fail "did not ask for stack timeline-dev: $(cat "$work/argv.log")"
# The page's own parser must accept what the script wrote.
if (cd "$REPO_ROOT/frontend" && node --input-type=module -e "
  import { parseDeployConfig } from './core/deploy-config.js';
  import fs from 'node:fs';
  const c = parseDeployConfig(fs.readFileSync('$work/configs/dev.json', 'utf8'));
  if (c.apiBase !== 'https://abc123.execute-api.us-east-1.amazonaws.com') throw new Error(c.apiBase);
  if (c.cognitoDomain !== 'https://timeline-dev-123456789012.auth.us-east-1.amazoncognito.com') throw new Error(c.cognitoDomain);
  if (c.clientId !== '4hj2k3l4m5n6o7p8q9r0s1t2u3') throw new Error(c.clientId);
"); then
  pass "the page accepts the file it wrote"
else
  fail "the page refused the file it wrote: $(cat "$work/configs/dev.json")"
fi

python3 -c 'import json,sys; assert json.load(open(sys.argv[1]))["recordActivity"] is False' "$work/configs/dev.json" \
  && pass "records no activity when the stack has no RecordActivity output" \
  || fail "recordActivity without the output: $(cat "$work/configs/dev.json")"

python3 - "$work/answer-cloudformation" <<'PY'
import json, sys
outputs = json.load(open(sys.argv[1]))
outputs.append({"OutputKey": "RecordActivity", "OutputValue": "on"})
json.dump(outputs, open(sys.argv[1], "w"))
PY
"$REPO_ROOT/scripts/write-deploy-config.sh" dev > "$work/out.txt"
python3 -c 'import json,sys; assert json.load(open(sys.argv[1]))["recordActivity"] is True' "$work/configs/dev.json" \
  && pass "records activity when the stack's RecordActivity output is on" \
  || fail "recordActivity with the output on: $(cat "$work/configs/dev.json")"
expected_version="$(git -C "$REPO_ROOT" describe --always --dirty)"
python3 -c 'import json,sys; assert json.load(open(sys.argv[1]))["pageVersion"] == sys.argv[2]' \
  "$work/configs/dev.json" "$expected_version" \
  && pass "writes the page's version from git" \
  || fail "pageVersion: $(cat "$work/configs/dev.json"), expected $expected_version"

echo '[{"OutputKey": "ApiUrl", "OutputValue": "https://x.example"}]' > "$work/answer-cloudformation"
if "$REPO_ROOT/scripts/write-deploy-config.sh" old > "$work/out.txt" 2>&1; then
  fail "succeeded although the stack lacks outputs"
else
  grep -q "CognitoDomain, UserPoolClientId" "$work/out.txt" && pass "names the missing outputs" \
    || fail "did not name the missing outputs: $(cat "$work/out.txt")"
fi

if "$REPO_ROOT/scripts/write-deploy-config.sh" "../evil" > "$work/out.txt" 2>&1; then
  fail "accepted a stage name with a path in it"
else
  pass "refuses a stage name with a path in it"
fi

echo "aws-dev-token.sh"
: > "$work/argv.log"
echo "4hj2k3l4m5n6o7p8q9r0s1t2u3" > "$work/answer-cloudformation"
echo "eyJ.test.token" > "$work/answer-cognito-idp"
secret='p@ss, "word" with commas'
if token="$(printf '%s\n' "$secret" | "$REPO_ROOT/scripts/aws-dev-token.sh" alice@example.com 2>"$work/err.txt")"; then
  [ "$token" = "eyJ.test.token" ] && pass "prints the token" || fail "printed: $token"
else
  fail "exited with an error: $(cat "$work/err.txt")"
fi
grep -qF "$secret" "$work/argv.log" && fail "the password appeared on a command line" \
  || pass "the password never appears on a command line"
python3 - "$work/request.log" "$secret" <<'PY' && pass "sends a password sign-in for the client and user" || fail "wrong request: $(cat "$work/request.log")"
import json, sys
r = json.load(open(sys.argv[1]))
assert r["AuthFlow"] == "USER_PASSWORD_AUTH", r
assert r["ClientId"] == "4hj2k3l4m5n6o7p8q9r0s1t2u3", r
assert r["AuthParameters"] == {"USERNAME": "alice@example.com", "PASSWORD": sys.argv[2]}, r
PY
[ "$(cat "$work/request-mode.log" 2>/dev/null)" = "600" ] && pass "the request file is readable by its owner only" \
  || fail "the request file's permissions were: $(cat "$work/request-mode.log" 2>/dev/null)"
request_file="$(cat "$work/request-path.log" 2>/dev/null || true)"
[ -n "$request_file" ] && [ ! -e "$request_file" ] && pass "the request file is deleted after success" \
  || fail "the request file was not deleted after success: ${request_file:-(none recorded)}"

: > "$work/request-path.log"
touch "$work/fail-cognito-idp"
if printf '%s\n' "$secret" | "$REPO_ROOT/scripts/aws-dev-token.sh" alice@example.com > "$work/out.txt" 2>&1; then
  fail "succeeded although Cognito refused the sign-in"
else
  grep -q "NotAuthorizedException" "$work/out.txt" && pass "passes on Cognito's refusal" \
    || fail "did not pass on Cognito's refusal: $(cat "$work/out.txt")"
fi
rm "$work/fail-cognito-idp"
request_file="$(cat "$work/request-path.log" 2>/dev/null || true)"
[ -n "$request_file" ] && [ ! -e "$request_file" ] && pass "the request file is deleted after a refusal" \
  || fail "the request file was not deleted after a refusal: ${request_file:-(none recorded)}"

echo "publish-page.sh"
: > "$work/argv.log"
: > "$work/answer-s3"
: > "$work/answer-cloudfront"
page_outputs='[
  {"OutputKey": "ApiUrl", "OutputValue": "https://abc123.execute-api.us-east-1.amazonaws.com"},
  {"OutputKey": "UserPoolClientId", "OutputValue": "4hj2k3l4m5n6o7p8q9r0s1t2u3"},
  {"OutputKey": "CognitoDomain", "OutputValue": "https://timeline-dev-123456789012.auth.us-east-1.amazoncognito.com"},
  {"OutputKey": "PageUrl", "OutputValue": "https://howangryami.telotrope.ai/"},
  {"OutputKey": "PageBucketName", "OutputValue": "timeline-page-dev-123456789012"},
  {"OutputKey": "PageDistributionId", "OutputValue": "E2QWRUHAPOMQZL"},
  {"OutputKey": "PageDnsTarget", "OutputValue": "d111111abcdef8.cloudfront.net"}
]'

# A stack deployed without HostPage=on has no page outputs.
echo '[{"OutputKey": "ApiUrl", "OutputValue": "https://x.example"}]' > "$work/answer-cloudformation"
if "$REPO_ROOT/scripts/publish-page.sh" dev > "$work/out.txt" 2>&1; then
  fail "succeeded although the stack has no page outputs"
else
  grep -q "deploy it with HostPage=on first" "$work/out.txt" && pass "asks for HostPage=on when the stack isn't hosting" \
    || fail "did not ask for HostPage=on: $(cat "$work/out.txt")"
fi
grep -q "^s3 \|^cloudfront " "$work/argv.log" && fail "uploaded although the stack isn't hosting" \
  || pass "uploads nothing when the stack isn't hosting"

if "$REPO_ROOT/scripts/publish-page.sh" "../evil" > "$work/out.txt" 2>&1; then
  fail "accepted a stage name with a path in it"
else
  pass "refuses a stage name with a path in it"
fi

echo "$page_outputs" > "$work/answer-cloudformation"
: > "$work/argv.log"
if "$REPO_ROOT/scripts/publish-page.sh" dev > "$work/out.txt" 2>&1; then
  pass "succeeds with a hosting stack's outputs"
else
  fail "failed with a hosting stack's outputs: $(cat "$work/out.txt")"
fi
grep -q "Published to https://howangryami.telotrope.ai/" "$work/out.txt" && pass "prints the page's address" \
  || fail "did not print the page's address: $(cat "$work/out.txt")"

expected="$( (cd "$REPO_ROOT" && { echo index.html; echo frontend/deploy-configs/dev.json;
  find frontend -name '*.js' -not -path 'frontend/tests/*' -not -path '*/node_modules/*'; find vendor -type f; }) | sort)"
actual="$( (cd "$work/synced" && find . -type f | sed 's|^\./||') | sort)"
[ "$expected" = "$actual" ] && pass "uploads exactly the page's files, timeline.html as index.html" \
  || fail "uploaded a different file list: $(diff <(echo "$expected") <(echo "$actual"))"

python3 - "$REPO_ROOT/timeline.html" "$work/synced/index.html" <<'PY' && pass "index.html is timeline.html plus one tag naming the stage" || fail "index.html is wrong"
import sys
source, published = (open(p, encoding="utf-8").read() for p in sys.argv[1:])
charset = '<meta charset="UTF-8">\n'
tag = '<meta name="timeline-deploy" content="dev">\n'
assert published.count('name="timeline-deploy"') == 1, published[:400]
assert published.replace(tag, "", 1) == source
assert published.index(tag) == published.index(charset) + len(charset)
PY

if (cd "$REPO_ROOT/frontend" && node --input-type=module -e "
  import { parseDeployConfig } from './core/deploy-config.js';
  import fs from 'node:fs';
  const c = parseDeployConfig(fs.readFileSync('$work/synced/frontend/deploy-configs/dev.json', 'utf8'));
  if (c.apiBase !== 'https://abc123.execute-api.us-east-1.amazonaws.com') throw new Error(c.apiBase);
"); then
  pass "the uploaded settings file is the stack's, and the page accepts it"
else
  fail "the uploaded settings file is wrong: $(cat "$work/synced/frontend/deploy-configs/dev.json")"
fi

for expect in \
  "--include \*\.js --content-type text/javascript; charset=utf-8" \
  "--include \*\.json --content-type application/json" \
  "--include \*\.html --content-type text/html; charset=utf-8" \
  "--include \*\.md --include \*/LICENSE --include \*/LICENSE-\* --content-type text/plain; charset=utf-8"; do
  grep -q -- "^s3 sync .* s3://timeline-page-dev-123456789012 --delete --cache-control no-cache --exclude \* $expect --only-show-errors$" "$work/argv.log" \
    && pass "syncs with --delete, no-cache and its type: ${expect#--include }" \
    || fail "no sync for: $expect; got: $(grep '^s3' "$work/argv.log")"
done
[ "$(grep -c '^s3 sync' "$work/argv.log")" = 4 ] && pass "syncs four groups, no more" \
  || fail "sync count: $(grep -c '^s3 sync' "$work/argv.log")"
grep -q -- "^cloudfront create-invalidation --distribution-id E2QWRUHAPOMQZL --paths /\*$" "$work/argv.log" \
  && pass "clears the distribution's copies" \
  || fail "wrong or missing invalidation: $(grep '^cloudfront' "$work/argv.log")"
[ "$(tail -1 "$work/argv.log" | cut -d' ' -f1)" = cloudfront ] \
  && pass "clears the copies only after every upload" || fail "the last AWS call was: $(tail -1 "$work/argv.log")"

# The refusals that need a different repository: a copy with one change each.
fake_repo() {
  rm -rf "$work/repo" "$work/synced"
  mkdir -p "$work/repo/scripts" "$work/repo/frontend"
  cp "$REPO_ROOT/scripts/publish-page.sh" "$REPO_ROOT/scripts/write-deploy-config.sh" "$work/repo/scripts/"
  cp -r "$REPO_ROOT/timeline.html" "$REPO_ROOT/vendor" "$work/repo/"
  cp "$REPO_ROOT/frontend/main.js" "$work/repo/frontend/"
  : > "$work/argv.log"
}
fake_repo
touch "$work/repo/vendor/style.css"
if "$work/repo/scripts/publish-page.sh" dev > "$work/out.txt" 2>&1; then
  fail "published a file it has no content type for"
else
  grep -q "vendor/style.css" "$work/out.txt" && pass "refuses a file it has no content type for, naming it" \
    || fail "did not name the untyped file: $(cat "$work/out.txt")"
fi
grep -q "^s3 " "$work/argv.log" && fail "uploaded before refusing the untyped file" \
  || pass "uploads nothing when a file has no content type"

fake_repo
sed -i 's|<meta charset="UTF-8">|&\n<meta name="timeline-deploy" content="prod">|' "$work/repo/timeline.html"
if "$work/repo/scripts/publish-page.sh" dev > "$work/out.txt" 2>&1; then
  fail "published a timeline.html that already names a deployment"
else
  grep -q "already has a timeline-deploy tag" "$work/out.txt" && pass "refuses a timeline.html that already names a deployment" \
    || fail "wrong refusal: $(cat "$work/out.txt")"
fi

fake_repo
sed -i 's|<meta charset="UTF-8">|<meta charset="utf-8">|' "$work/repo/timeline.html"
if "$work/repo/scripts/publish-page.sh" dev > "$work/out.txt" 2>&1; then
  fail "published a timeline.html with no charset line to put the tag after"
else
  grep -q 'no single <meta charset="UTF-8"> line' "$work/out.txt" && pass "refuses a timeline.html with no charset line" \
    || fail "wrong refusal: $(cat "$work/out.txt")"
fi

echo
if [ "$failures" -eq 0 ]; then echo "All passed."; else echo "$failures failed." >&2; exit 1; fi
