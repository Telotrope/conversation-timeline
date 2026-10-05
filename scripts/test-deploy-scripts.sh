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
# $work/answer-<command>-<subcommand> if that exists, else
# $work/answer-<command>. A describe-stacks or describe-certificate asking
# only for the status answers from $work/answer-<command>-status, and exits
# with an error while $work/fail-<command>-<subcommand> exists.
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
if [ "$1" = "cognito-idp" ] && [[ "$*" == *--cli-input-json* ]]; then
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
if [ -e "$STUB_DIR/fail-$1-$2" ]; then
  echo "aws: [ERROR]: $(cat "$STUB_DIR/fail-$1-$2")" >&2
  exit 255
fi
if [[ "$*" == *"StackStatus"* || "$*" == *"Certificate.Status"* ]] && [ -e "$STUB_DIR/answer-$1-status" ]; then
  cat "$STUB_DIR/answer-$1-status"
elif [ -e "$STUB_DIR/answer-$1-$2" ]; then
  cat "$STUB_DIR/answer-$1-$2"
else
  cat "$STUB_DIR/answer-$1"
fi
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
  find frontend \( -name '*.js' -o -name '*.css' \) -not -path 'frontend/tests/*' -not -path '*/node_modules/*'; find vendor -type f; }) | sort)"
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
  "--include \*\.css --content-type text/css; charset=utf-8" \
  "--include \*\.json --content-type application/json" \
  "--include \*\.html --content-type text/html; charset=utf-8" \
  "--include \*\.md --include \*/LICENSE --include \*/LICENSE-\* --content-type text/plain; charset=utf-8"; do
  grep -q -- "^s3 sync .* s3://timeline-page-dev-123456789012 --delete --cache-control no-cache --exclude \* $expect --only-show-errors$" "$work/argv.log" \
    && pass "syncs with --delete, no-cache and its type: ${expect#--include }" \
    || fail "no sync for: $expect; got: $(grep '^s3' "$work/argv.log")"
done
[ "$(grep -c '^s3 sync' "$work/argv.log")" = 5 ] && pass "syncs five groups, no more" \
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
touch "$work/repo/vendor/picture.webp"
if "$work/repo/scripts/publish-page.sh" dev > "$work/out.txt" 2>&1; then
  fail "published a file it has no content type for"
else
  grep -q "vendor/picture.webp" "$work/out.txt" && pass "refuses a file it has no content type for, naming it" \
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

echo "request-certificate.sh"
: > "$work/argv.log"
cert_arn="arn:aws:acm:us-east-1:123456789012:certificate/33130ff2-3a02-473a-9f54-f21c606f7b37"
echo "None" > "$work/answer-acm-list-certificates"
echo "$cert_arn" > "$work/answer-acm-request-certificate"
printf '_7545642f.dev.howangryami.telotrope.ai.\t_69f98dd3.wzccmgtwzk.acm-validations.aws.\n' > "$work/answer-acm-describe-certificate"
echo "PENDING_VALIDATION" > "$work/answer-acm-status"
: > "$work/answer-acm-wait"
if REQUEST_CERTIFICATE_POLL=0 "$REPO_ROOT/scripts/request-certificate.sh" dev.howangryami.telotrope.ai > "$work/out.txt" 2>&1; then
  pass "requests a certificate for a new domain"
else
  fail "failed for a new domain: $(cat "$work/out.txt")"
fi
grep -q "^acm request-certificate --domain-name dev.howangryami.telotrope.ai --validation-method DNS .*--region us-east-1$" "$work/argv.log" \
  && pass "asks us-east-1 for a DNS-validated certificate" || fail "request: $(grep request "$work/argv.log")"
grep -q "Host:   _7545642f.dev.howangryami$" "$work/out.txt" && grep -q "Answer: _69f98dd3.wzccmgtwzk.acm-validations.aws$" "$work/out.txt" \
  && pass "prints the Porkbun record, host without .telotrope.ai" || fail "record: $(cat "$work/out.txt")"
grep -q "acm wait certificate-validated --certificate-arn $cert_arn --region us-east-1" "$work/argv.log" \
  && pass "waits for validation" || fail "did not wait: $(cat "$work/argv.log")"
grep -q "PageCertificateArn=\"$cert_arn\"" "$work/out.txt" && pass "prints the identifier for samconfig.toml" \
  || fail "no identifier: $(cat "$work/out.txt")"

: > "$work/argv.log"
echo "$cert_arn" > "$work/answer-acm-list-certificates"
echo "ISSUED" > "$work/answer-acm-status"
REQUEST_CERTIFICATE_POLL=0 "$REPO_ROOT/scripts/request-certificate.sh" dev.howangryami.telotrope.ai > "$work/out.txt" 2>&1 || true
! grep -q "request-certificate" "$work/argv.log" && grep -q "Reusing" "$work/out.txt" \
  && pass "reuses a certificate already requested for the domain" || fail "made another: $(cat "$work/argv.log")"
! grep -q "wait certificate-validated" "$work/argv.log" && ! grep -q "Add this record" "$work/out.txt" \
  && pass "an issued certificate needs no record and no wait" || fail "issued: $(cat "$work/out.txt")"

echo "PENDING_VALIDATION" > "$work/answer-acm-status"
echo "Waiter CertificateValidated failed: Max attempts exceeded" > "$work/fail-acm-wait"
if REQUEST_CERTIFICATE_POLL=0 "$REPO_ROOT/scripts/request-certificate.sh" dev.howangryami.telotrope.ai > "$work/out.txt" 2>&1; then
  fail "succeeded although validation never happened"
else
  grep -q "run this again to keep waiting" "$work/out.txt" && pass "says to run again when validation takes too long" \
    || fail "wrong message: $(cat "$work/out.txt")"
fi
rm "$work/fail-acm-wait"
if "$REPO_ROOT/scripts/request-certificate.sh" "Not_A_Domain" > "$work/out.txt" 2>&1; then
  fail "accepted a malformed domain"
else
  pass "refuses a malformed domain"
fi

echo "deploy.sh"
# A throwaway repository holding what deploy.sh uses, with its own "origin",
# so the public-only rules (main, no local changes, pushed) can be tested.
deploy_repo() {
  rm -rf "$work/drepo" "$work/origin.git" "$work/synced"
  mkdir -p "$work/drepo/scripts" "$work/drepo/infra" "$work/drepo/frontend" "$work/drepo/backend"
  cp "$REPO_ROOT"/scripts/{deploy.sh,deploy_checks.py,publish-page.sh,write-deploy-config.sh,check-template.sh} "$work/drepo/scripts/"
  cp "$REPO_ROOT/infra/samconfig.toml" "$REPO_ROOT/infra/template.yaml" "$work/drepo/infra/"
  cp -r "$REPO_ROOT/timeline.html" "$REPO_ROOT/vendor" "$work/drepo/"
  cp "$REPO_ROOT/frontend/main.js" "$work/drepo/frontend/"
  (cd "$work/drepo" && git init -q -b main && git add -A && git -c user.name=t -c user.email=t@t commit -qm init \
    && git init -q --bare "$work/origin.git" && git remote add origin "$work/origin.git" && git push -q origin main)
  : > "$work/argv.log"
}
# Stand-ins for sam, cargo, curl, getent and df, logging to the same file.
cat > "$work/bin/sam" <<'STUB'
#!/usr/bin/env bash
echo "sam $*" >> "$STUB_DIR/argv.log"
if [ "$1" = validate ]; then echo "template.yaml is a valid SAM Template"; exit 0; fi
if [ -e "$STUB_DIR/sam-no-changes" ]; then echo "Error: No changes to deploy. Stack timeline-dev is up to date"; exit 1; fi
echo "Changeset created successfully. arn:aws:cloudformation:us-east-1:123456789012:changeSet/samcli-deploy1/abc"
STUB
cat > "$work/bin/cargo" <<'STUB'
#!/usr/bin/env bash
echo "cargo $*" >> "$STUB_DIR/argv.log"
[ ! -e "$STUB_DIR/fail-cargo" ]
STUB
cat > "$work/bin/curl" <<'STUB'
#!/usr/bin/env bash
echo "curl $*" >> "$STUB_DIR/argv.log"
if [[ "$*" == *"-X OPTIONS"* ]]; then printf 'HTTP/2 204\r\naccess-control-allow-origin: %s\r\n' "$(cat "$STUB_DIR/curl-origin")"
elif [[ "$*" == *".json"* ]]; then cat "$STUB_DIR/curl-settings"
else cat "$STUB_DIR/curl-page"; fi
STUB
printf '#!/bin/sh\necho "18.160.0.1      STREAM d111111abcdef8.cloudfront.net"\n' > "$work/bin/getent"
cat > "$work/bin/df" <<'STUB'
#!/bin/sh
echo Avail; cat "$STUB_DIR/df-avail"
STUB
chmod +x "$work/bin/"{sam,cargo,curl,getent,df}

dev_outputs='[
  {"OutputKey": "ApiUrl", "OutputValue": "https://abc123.execute-api.us-east-1.amazonaws.com"},
  {"OutputKey": "UserPoolId", "OutputValue": "us-east-1_AbCdEf123"},
  {"OutputKey": "UserPoolClientId", "OutputValue": "4hj2k3l4m5n6o7p8q9r0s1t2u3"},
  {"OutputKey": "CognitoDomain", "OutputValue": "https://timeline-dev-123456789012.auth.us-east-1.amazoncognito.com"},
  {"OutputKey": "RawUploadsBucketName", "OutputValue": "timeline-uploads-dev-123456789012"},
  {"OutputKey": "PageUrl", "OutputValue": "https://dev.howangryami.telotrope.ai/"},
  {"OutputKey": "PageBucketName", "OutputValue": "timeline-dev-pagebucket-abc"},
  {"OutputKey": "PageDistributionId", "OutputValue": "E2QWRUHAPOMQZL"},
  {"OutputKey": "PageDnsTarget", "OutputValue": "d111111abcdef8.cloudfront.net"}
]'
deploy_answers() {
  local env="$1"
  # The public site is at howangryami.telotrope.ai, dev at dev.howangryami.telotrope.ai.
  if [ "$env" = public ]; then
    echo "$dev_outputs" | sed 's/dev\.howangryami/howangryami/; s/timeline-dev/timeline-public/g; s/-dev-/-public-/g' > "$work/answer-cloudformation"
  else
    echo "$dev_outputs" > "$work/answer-cloudformation"
  fi
  echo "123456789012" > "$work/answer-sts"
  echo '{"Changes": [{"ResourceChange": {"Action": "Modify", "LogicalResourceId": "HttpApi", "ResourceType": "AWS::ApiGatewayV2::Api", "Replacement": "False"}}]}' \
    > "$work/answer-cloudformation-describe-change-set"
  cp "$REPO_ROOT/scripts/fixtures/processed-httpapi-2026-10-02-fixed-cors.json" "$work/answer-cloudformation-get-template"
  : > "$work/answer-cloudformation-execute-change-set"
  : > "$work/answer-cloudformation-delete-change-set"
  echo "UPDATE_COMPLETE" > "$work/answer-cloudformation-status"
  local host; host="$(python3 -c 'import json,sys; print({o["OutputKey"]: o["OutputValue"] for o in json.load(open(sys.argv[1]))}["PageUrl"])' "$work/answer-cloudformation")"
  printf '%s\t%s\n' "$host" "http://localhost:8000/timeline.html" > "$work/answer-cognito-idp"
  printf '<meta name="timeline-deploy" content="%s">\n' "$env" > "$work/curl-page"
  printf '{\n  "apiBase": "https://abc123.execute-api.us-east-1.amazonaws.com",\n  "clientId": "x"\n}\n' > "$work/curl-settings"
  host="${host#https://}"; echo "https://${host%/}" > "$work/curl-origin"
  echo $((50 * 1024 * 1024 * 1024)) > "$work/df-avail"
  rm -f "$work/sam-no-changes" "$work/fail-cargo" "$work"/fail-cloudformation-* "$work"/fail-sts-*
}
run_deploy() { (cd "$work/drepo" && DEPLOY_POLL=0 scripts/deploy.sh "$@") > "$work/out.txt" 2>&1; }
# The line number of the first log line matching $1 (0 if none).
at() { grep -n -m1 -- "$1" "$work/argv.log" | cut -d: -f1 || echo 0; }

deploy_repo; deploy_answers dev
if run_deploy dev < /dev/null; then pass "dev: deploys with nothing to ask"; else fail "dev failed: $(tail -20 "$work/out.txt")"; fi
order=(
  "cargo lambda build --release --arm64 -p timeline-api"
  "sam validate"
  "sam deploy --config-env dev --no-execute-changeset"
  "cloudformation describe-change-set"
  "cloudformation get-template --stack-name timeline-dev"
  "cloudformation execute-change-set"
  "s3 sync"
  "cloudfront create-invalidation"
  "cognito-idp describe-user-pool-client"
)
prev=0; in_order=yes
for marker in "${order[@]}"; do
  n="$(at "$marker")"
  if [ "$n" = 0 ] || [ "$n" -le "$prev" ]; then in_order="no (at: $marker)"; break; fi
  prev="$n"
done
[ "$in_order" = yes ] && pass "dev: build, check, change set, checks, apply, publish, live checks, in order" \
  || fail "dev: order $in_order: $(cat "$work/argv.log")"
grep -q "ok: the page loads and names deployment dev" "$work/out.txt" && grep -q "ok: Cognito returns sign-ins to https://dev.howangryami.telotrope.ai/" "$work/out.txt" \
  && pass "dev: the live checks pass and are named" || fail "dev: live checks: $(tail -12 "$work/out.txt")"
grep -q "curl .*--resolve dev.howangryami.telotrope.ai:443:18.160.0.1" "$work/argv.log" \
  && pass "dev: asks the distribution itself, whatever DNS says" || fail "dev: no --resolve: $(grep curl "$work/argv.log")"
grep -q "CNAME dev.howangryami -> d111111abcdef8.cloudfront.net" "$work/out.txt" \
  && pass "dev: names the Porkbun record while the domain doesn't point here" || fail "dev: no DNS hint: $(tail -3 "$work/out.txt")"

deploy_repo; deploy_answers dev
run_deploy dev FailProcessing=on < /dev/null || true
grep -q 'sam deploy .*--parameter-overrides .*FailProcessing="on"' "$work/argv.log" \
  && grep -q 'sam deploy .*PageDomain="dev.howangryami.telotrope.ai"' "$work/argv.log" \
  && grep -q 'sam deploy .*AlsoAllowLocalPage="on"' "$work/argv.log" \
  && pass "a Setting=value changes that setting and keeps samconfig.toml's others" \
  || fail "overrides: $(grep 'sam deploy' "$work/argv.log")"

deploy_repo; deploy_answers dev
cp "$REPO_ROOT/scripts/fixtures/processed-httpapi-2026-10-02-broken-cors.json" "$work/answer-cloudformation-get-template"
if run_deploy dev < /dev/null; then fail "applied a change set with the 2026-10-02 CORS shape"; else
  grep -q "CORS is a list, not a CORS object" "$work/out.txt" && [ "$(at "execute-change-set")" = 0 ] \
    && pass "the 2026-10-02 CORS shape (AWS's real processed template) stops the deploy before applying" \
    || fail "broken CORS: $(tail -8 "$work/out.txt")"
fi

deploy_repo; deploy_answers dev
echo '{"Changes": [{"ResourceChange": {"Action": "Modify", "LogicalResourceId": "ConversationsTable", "ResourceType": "AWS::DynamoDB::Table", "Replacement": "True"}}]}' \
  > "$work/answer-cloudformation-describe-change-set"
if echo n | run_deploy dev; then fail "dev: applied a replacement after 'n'"; else
  grep -q "replaces (True) ConversationsTable" "$work/out.txt" && [ "$(at "execute-change-set")" = 0 ] && [ "$(at "delete-change-set")" != 0 ] \
    && pass "dev: a replacement asks first; 'n' applies nothing and deletes the change set" \
    || fail "dev replacement, n: $(tail -8 "$work/out.txt")"
fi
deploy_repo; deploy_answers dev
echo '{"Changes": [{"ResourceChange": {"Action": "Remove", "LogicalResourceId": "PageBucket", "ResourceType": "AWS::S3::Bucket"}}]}' \
  > "$work/answer-cloudformation-describe-change-set"
echo y | run_deploy dev && [ "$(at "execute-change-set")" != 0 ] && grep -q "removes PageBucket" "$work/out.txt" \
  && pass "dev: a removal asks first; 'y' applies" || fail "dev removal, y: $(tail -8 "$work/out.txt")"

deploy_repo; deploy_answers public
if echo n | run_deploy public; then fail "public: applied after 'n'"; else
  grep -q "Apply to timeline-public?" "$work/out.txt" && [ "$(at "execute-change-set")" = 0 ] \
    && pass "public: always asks; 'n' applies nothing" || fail "public, n: $(tail -8 "$work/out.txt")"
fi
deploy_repo; deploy_answers public
echo y | run_deploy public && [ "$(at "execute-change-set")" != 0 ] && grep -q "ok: the page loads and names deployment public" "$work/out.txt" \
  && grep -q "sam deploy --config-env public" "$work/argv.log" \
  && pass "public: 'y' deploys stack timeline-public" || fail "public, y: $(tail -12 "$work/out.txt")"

deploy_repo; deploy_answers public
(cd "$work/drepo" && git checkout -qb feature)
run_deploy public < /dev/null && fail "public: deployed from a branch" || \
  { grep -q "public deploys only from main" "$work/out.txt" && [ "$(at "cargo")" = 0 ] && pass "public: refuses off main, before building" \
    || fail "off main: $(cat "$work/out.txt")"; }
deploy_repo; deploy_answers public
echo change >> "$work/drepo/timeline.html"
run_deploy public < /dev/null && fail "public: deployed with local changes" || \
  { grep -q "no local changes" "$work/out.txt" && pass "public: refuses with local changes" || fail "dirty: $(cat "$work/out.txt")"; }
deploy_repo; deploy_answers public
(cd "$work/drepo" && git -c user.name=t -c user.email=t@t commit -qm local --allow-empty)
run_deploy public < /dev/null && fail "public: deployed an unpushed commit" || \
  { grep -q "push main first" "$work/out.txt" && pass "public: refuses unpushed commits" || fail "unpushed: $(cat "$work/out.txt")"; }
deploy_repo; deploy_answers dev
(cd "$work/drepo" && git checkout -qb feature && echo change >> timeline.html)
run_deploy dev < /dev/null && pass "dev: deploys from any branch, with local changes" || fail "dev on a branch: $(tail -5 "$work/out.txt")"

deploy_repo; deploy_answers dev
echo "An error occurred (ExpiredToken): The security token included in the request is expired" > "$work/fail-sts-get-caller-identity"
run_deploy dev < /dev/null && fail "deployed with an expired sign-in" || \
  { grep -q "Run: aws login --profile timeline --remote" "$work/out.txt" && pass "an expired sign-in names the command to run" \
    || fail "expired: $(cat "$work/out.txt")"; }
deploy_repo; deploy_answers dev
echo $((1024 * 1024 * 1024)) > "$work/df-avail"
run_deploy dev < /dev/null && fail "deployed with 1 GB free" || \
  { grep -q "the build needs 3 GB" "$work/out.txt" && pass "too little disk space stops it before building" || fail "disk: $(cat "$work/out.txt")"; }
deploy_repo; deploy_answers dev
touch "$work/fail-cargo"
run_deploy dev < /dev/null && fail "deployed after a failed build" || \
  { grep -q "the Lambda build failed" "$work/out.txt" && [ "$(at "sam deploy")" = 0 ] && pass "a failed build stops it" || fail "build: $(cat "$work/out.txt")"; }

deploy_repo; deploy_answers dev
touch "$work/sam-no-changes"
run_deploy dev < /dev/null && [ "$(at "execute-change-set")" = 0 ] && [ "$(at "s3 sync")" != 0 ] && grep -q "Nothing to apply" "$work/out.txt" \
  && pass "no changes: applies nothing, still publishes and checks" || fail "no changes: $(tail -8 "$work/out.txt")"
deploy_repo; deploy_answers dev
echo "UPDATE_ROLLBACK_COMPLETE" > "$work/answer-cloudformation-status"
run_deploy dev < /dev/null && fail "succeeded after a rollback" || \
  { grep -q "ended in UPDATE_ROLLBACK_COMPLETE" "$work/out.txt" && [ "$(at "s3 sync")" = 0 ] && pass "a rolled-back stack stops it before publishing" \
    || fail "rollback: $(tail -5 "$work/out.txt")"; }
deploy_repo; deploy_answers dev
echo "https://example.com" > "$work/curl-origin"
run_deploy dev < /dev/null && fail "succeeded with CORS refusing the page" || \
  { grep -q "FAILED: the API accepts the page's address (CORS)" "$work/out.txt" && pass "a failed live check stops it and is named" \
    || fail "live check: $(tail -8 "$work/out.txt")"; }
deploy_repo; deploy_answers dev
for bad in "staging" "dev Bad Setting" "dev Setting=a;b"; do
  # shellcheck disable=SC2086
  run_deploy $bad < /dev/null && fail "accepted: $bad" || true
done
[ "$(at "sts")" = 0 ] && pass "refuses an unknown environment or malformed settings before anything runs" \
  || fail "bad arguments ran something: $(cat "$work/argv.log")"

echo "diagnose-upload.sh"
up="aaaaaaaa-0000-4000-8000-000000000001"
diagnose_answers() {
  rm -f "$work"/answer-* "$work"/fail-*
  echo "123456789012" > "$work/answer-sts"
  echo "timeline-uploads-dev-123456789012" > "$work/answer-cloudformation-describe-stacks"
  printf 'raw/sub-1/%s.json\t2026-10-02T21:30:00+00:00\t60600000\n' "$up" > "$work/answer-s3api-list-objects-v2"
  cat > "$work/answer-dynamodb-query" <<JSON
{"Items": [
  {"pk": {"S": "sub-1"}, "sk": {"S": "PROGRESS#$up"}, "attempts": {"N": "3"}, "last_error": {"S": "processing failed: throttled"}},
  {"pk": {"S": "sub-1"}, "sk": {"S": "UPLOAD#$up"}, "status": {"S": "failed"}, "error": {"S": "the server couldn't process the file after 3 attempts"}}
], "Count": 2}
JSON
  echo '{"Items": [], "Count": 0}' > "$work/answer-dynamodb-scan"
  cat > "$work/answer-logs-filter-log-events" <<JSON
{"events": [{"timestamp": 1790000002500, "message": "{\"kind\":\"processing_run\",\"upload_id\":\"$up\",\"outcome\":\"failed\",\"error\":\"throttled\",\"ms\":900}"}]}
JSON
  : > "$work/argv.log"
}
diagnose_answers
if "$REPO_ROOT/scripts/diagnose-upload.sh" dev "$up" > "$work/out.txt" 2>&1; then
  grep -q "raw/sub-1/$up.json landed 2026-10-02T21:30:00+00:00, 60600000 bytes" "$work/out.txt" \
    && pass "shows when and how big the file landed" || fail "S3: $(cat "$work/out.txt")"
  grep -q "attempts: attempts 3, last_error processing failed: throttled" "$work/out.txt" \
    && grep -q "outcome: error the server couldn't process the file after 3 attempts, status failed" "$work/out.txt" \
    && pass "shows the stored outcome, attempt count and last error" || fail "DynamoDB: $(cat "$work/out.txt")"
  grep -q "dynamodb query --table-name timeline-conversations-dev --consistent-read --key-condition-expression pk = :u" "$work/argv.log" \
    && pass "reads the user's rows directly once the file names the user" || fail "query: $(grep dynamodb "$work/argv.log")"
  grep -q "upload aaaaaaaa… -> failed (throttled)" "$work/out.txt" \
    && pass "shows the processing attempts from the logs" || fail "logs: $(cat "$work/out.txt")"
else
  fail "failed: $(cat "$work/out.txt")"
fi
diagnose_answers
echo "None" > "$work/answer-s3api-list-objects-v2"
"$REPO_ROOT/scripts/diagnose-upload.sh" dev "$up" > "$work/out.txt" 2>&1 || true
grep -q "never uploaded, or deleted since" "$work/out.txt" && grep -q "dynamodb scan" "$work/argv.log" \
  && grep -q "no rows: processing never recorded" "$work/out.txt" \
  && pass "with no file: says so, searches the whole table, says there are no rows" || fail "no file: $(cat "$work/out.txt")"
diagnose_answers
echo "ExpiredToken" > "$work/fail-sts-get-caller-identity"
"$REPO_ROOT/scripts/diagnose-upload.sh" dev "$up" > "$work/out.txt" 2>&1 && fail "ran with an expired sign-in" \
  || { grep -q "Run: aws login --profile timeline --remote" "$work/out.txt" && pass "an expired sign-in names the command to run" \
    || fail "expired: $(cat "$work/out.txt")"; }
for bad in "staging $up" "dev not-an-id" "dev"; do
  # shellcheck disable=SC2086
  "$REPO_ROOT/scripts/diagnose-upload.sh" $bad > /dev/null 2>&1 && fail "accepted: $bad" || true
done
pass "refuses an unknown environment or a malformed upload ID"

echo "deploy_checks.py"
python3 "$REPO_ROOT/scripts/deploy_checks.py" cors "$REPO_ROOT/scripts/fixtures/processed-httpapi-2026-10-02-fixed-cors.json" > "$work/out.txt" \
  && pass "accepts the fixed CORS block AWS produced on 2026-10-02" || fail "fixed CORS: $(cat "$work/out.txt")"
echo '"{\"Resources\": {\"Api\": {\"Properties\": {\"Body\": {\"x-amazon-apigateway-cors\": {\"allowMethods\": [\"GET\"]}}}}}}"' > "$work/cors.json"
status=0; python3 "$REPO_ROOT/scripts/deploy_checks.py" cors "$work/cors.json" > "$work/out.txt" || status=$?
[ "$status" = 2 ] && grep -q "Api: CORS object has no allowOrigins" "$work/out.txt" \
  && pass "rejects a CORS object without allowOrigins, from a template given as a string" || fail "no allowOrigins: $status $(cat "$work/out.txt")"
echo "not json" > "$work/bad.json"
status=0; python3 "$REPO_ROOT/scripts/deploy_checks.py" changes "$work/bad.json" > /dev/null 2> "$work/out.txt" || status=$?
[ "$status" = 1 ] && grep -q "couldn't read" "$work/out.txt" && pass "unreadable input is exit 1, not a pass" || fail "bad input: $status"
status=0; python3 "$REPO_ROOT/scripts/deploy_checks.py" > /dev/null 2>&1 || status=$?
[ "$status" = 1 ] && pass "no arguments is exit 1 with usage" || fail "no arguments: $status"

echo
if [ "$failures" -eq 0 ]; then echo "All passed."; else echo "$failures failed." >&2; exit 1; fi
