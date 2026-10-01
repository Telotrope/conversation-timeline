#!/usr/bin/env bash
#
# Tests write-deploy-config.sh and aws-dev-token.sh against a stand-in for
# the `aws` command, which records what it was asked and answers with
# canned output. Run manually:
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

# The stand-in `aws`: logs its arguments and standard input, then answers
# from $work/answer-<subcommand>.
mkdir -p "$work/bin"
cat > "$work/bin/aws" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$STUB_DIR/argv.log"
if [ "$1" = "cognito-idp" ]; then cat > "$STUB_DIR/stdin.log"; fi
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
token="$(printf '%s\n' "$secret" | "$REPO_ROOT/scripts/aws-dev-token.sh" alice@example.com 2>/dev/null)"
[ "$token" = "eyJ.test.token" ] && pass "prints the token" || fail "printed: $token"
grep -qF "$secret" "$work/argv.log" && fail "the password appeared on a command line" \
  || pass "the password never appears on a command line"
python3 - "$work/stdin.log" "$secret" <<'PY' && pass "sends a password sign-in for the client and user" || fail "wrong request: $(cat "$work/stdin.log")"
import json, sys
r = json.load(open(sys.argv[1]))
assert r["AuthFlow"] == "USER_PASSWORD_AUTH", r
assert r["ClientId"] == "4hj2k3l4m5n6o7p8q9r0s1t2u3", r
assert r["AuthParameters"] == {"USERNAME": "alice@example.com", "PASSWORD": sys.argv[2]}, r
PY

echo
if [ "$failures" -eq 0 ]; then echo "All passed."; else echo "$failures failed." >&2; exit 1; fi
