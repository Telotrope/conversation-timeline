#!/usr/bin/env bash
# Deploys one remote environment end to end
# (docs/plans/2026-10-02-deployment-operating-guide.md §2):
#
#   scripts/deploy.sh dev                     # remote development
#   scripts/deploy.sh public                  # the public site
#   scripts/deploy.sh dev FailProcessing=on   # with settings changed for this deploy
#
# Builds the Lambdas, checks the template, makes a change set, checks it
# (nothing replaced or removed; the API's CORS as AWS will apply it), applies
# it, publishes the page and checks the live site. `dev` applies by itself
# when nothing is replaced or removed; `public` always asks once.
#
# Settings come from infra/samconfig.toml's [<env>] sections. KEY=VALUE
# arguments change single settings for this deploy only; the next plain
# deploy restores samconfig.toml's.
set -euo pipefail

env="${1:-}"
if [[ "$env" != "dev" && "$env" != "public" ]]; then
  echo "usage: scripts/deploy.sh <dev|public> [Setting=value ...]" >&2
  exit 2
fi
shift
for setting in "$@"; do
  if [[ ! "$setting" =~ ^[A-Za-z]+=[A-Za-z0-9._:/-]*$ ]]; then
    echo "\"$setting\" is not Setting=value." >&2
    exit 2
  fi
done

repo="$(cd "$(dirname "$0")/.." && pwd)"
export AWS_PROFILE="${AWS_PROFILE:-timeline}" AWS_REGION="${AWS_REGION:-us-east-1}" SAM_CLI_TELEMETRY=0
# Where this machine keeps cargo, zig and sam, after your own PATH.
export PATH="$PATH:$HOME/.local/opt/zig:$HOME/.cargo/bin:$HOME/.local/bin"
stack="timeline-$env"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
step() { echo; echo "== $*"; }
die() { echo "deploy.sh: $*" >&2; exit 1; }

step "1. Before anything"
aws sts get-caller-identity --query Account --output text > /dev/null 2> "$work/sts.txt" \
  || die "AWS sign-in isn't valid ($(tail -1 "$work/sts.txt")). Run: aws login --profile timeline --remote"
free="$(df --output=avail -B1 "$repo" | tail -1)"
[ "$free" -ge $((3 * 1024 * 1024 * 1024)) ] \
  || die "only $((free / 1024 / 1024)) MB free on the disk; the build needs 3 GB"
if [ "$env" = public ]; then
  [ "$(git -C "$repo" rev-parse --abbrev-ref HEAD)" = main ] || die "public deploys only from main"
  [ -z "$(git -C "$repo" status --porcelain)" ] || die "public deploys only with no local changes (git status)"
  git -C "$repo" fetch -q origin main || die "couldn't fetch origin/main to check for unpushed commits"
  [ "$(git -C "$repo" rev-list --count origin/main..HEAD)" = 0 ] \
    || die "public deploys only what's pushed: push main first"
fi
echo "signed in; $((free / 1024 / 1024 / 1024)) GB free"

step "2. Build and check the template"
(cd "$repo/backend" && cargo lambda build --release --arm64 -p timeline-api) || die "the Lambda build failed"
"$repo/scripts/check-template.sh" || die "the template check failed"

step "3. Change set"
overrides="$(python3 - "$repo/infra/samconfig.toml" "$env" "$@" <<'PY'
import sys, tomllib, shlex
path, env, *changes = sys.argv[1:]
saved = tomllib.load(open(path, "rb"))[env]["deploy"]["parameters"]["parameter_overrides"]
settings = dict(item.split("=", 1) for item in shlex.split(saved))
settings.update(item.split("=", 1) for item in changes)
print(" ".join(f'{k}="{v}"' for k, v in settings.items()))
PY
)"
set +e
(cd "$repo/infra" && sam deploy --config-env "$env" --no-execute-changeset \
  --parameter-overrides "$overrides") > "$work/sam.txt" 2>&1
sam_status=$?
set -e
cat "$work/sam.txt"
if grep -q "No changes to deploy" "$work/sam.txt"; then
  echo "Nothing to apply; the stack already matches."
  change_set=""
else
  [ "$sam_status" = 0 ] || die "making the change set failed (above)"
  change_set="$(grep -o 'arn:aws:cloudformation:[^ ]*changeSet/[^ ]*' "$work/sam.txt" | tail -1)"
  [ -n "$change_set" ] || die "sam deploy didn't print a change set"
fi

if [ -n "$change_set" ]; then
  step "4. Check the change set"
  aws cloudformation describe-change-set --change-set-name "$change_set" --output json > "$work/changes.json"
  risky=0
  python3 "$repo/scripts/deploy_checks.py" changes "$work/changes.json" > "$work/risky.txt" || risky=$?
  [ "$risky" = 0 ] || [ "$risky" = 2 ] || die "couldn't read the change set"
  aws cloudformation get-template --stack-name "$stack" --change-set-name "$change_set" --template-stage Processed \
    --query TemplateBody --output json > "$work/processed.json"
  if ! python3 "$repo/scripts/deploy_checks.py" cors "$work/processed.json" > "$work/cors.txt"; then
    cat "$work/cors.txt" >&2
    die "the API's CORS as AWS would apply it is wrong (above); nothing applied"
  fi
  echo "the API's CORS block is complete"

  step "5. Apply"
  ask=no
  if [ "$env" = public ]; then ask=yes; fi
  if [ "$risky" = 2 ]; then
    echo "This change set replaces or removes, which loses what those resources hold:"
    cat "$work/risky.txt"
    ask=yes
  fi
  if [ "$ask" = yes ]; then
    # Printed, not `read -p`, which shows nothing when the answer is piped in.
    printf 'Apply to %s? [y/N] ' "$stack"
    read -r answer || answer=""
    if [[ "$answer" != [yY] ]]; then
      aws cloudformation delete-change-set --change-set-name "$change_set"
      die "not applied; the change set was deleted"
    fi
  fi
  aws cloudformation execute-change-set --change-set-name "$change_set"
  while :; do
    status="$(aws cloudformation describe-stacks --stack-name "$stack" --query 'Stacks[0].StackStatus' --output text)"
    case "$status" in
      *_IN_PROGRESS) sleep "${DEPLOY_POLL:-15}" ;;
      CREATE_COMPLETE|UPDATE_COMPLETE) echo "$stack: $status"; break ;;
      *) die "$stack ended in $status; see its events in the CloudFormation console" ;;
    esac
  done
fi

step "6. Publish the page"
"$repo/scripts/publish-page.sh" "$env"

step "7. Check the live site"
aws cloudformation describe-stacks --stack-name "$stack" --query 'Stacks[0].Outputs' --output json > "$work/outputs.json"
output() { python3 -c 'import json,sys; print({o["OutputKey"]: o["OutputValue"] for o in json.load(open(sys.argv[1]))}[sys.argv[2]])' "$work/outputs.json" "$1"; }
page_url="$(output PageUrl)"; api="$(output ApiUrl)"; target="$(output PageDnsTarget)"
host="${page_url#https://}"; host="${host%/}"; origin="https://$host"
# Asked of the distribution itself, so the checks work before the domain's
# DNS record points at it.
ip="$(getent ahostsv4 "$target" | awk 'NR==1 {print $1}')"
[ -n "$ip" ] || die "couldn't look up $target"
fetch() { curl -sS --max-time 20 --resolve "$host:443:$ip" "$@"; }
failed=0
check() { if "$@"; then echo "  ok: $label"; else echo "  FAILED: $label" >&2; failed=1; fi; }

label="the page loads and names deployment $env"
check grep -q "<meta name=\"timeline-deploy\" content=\"$env\">" <(fetch "$page_url")
label="the settings file names this stack's API"
check grep -q "\"apiBase\": \"$api\"" <(fetch "${page_url}frontend/deploy-configs/$env.json")
label="the API accepts the page's address (CORS)"
check grep -qi "^access-control-allow-origin: $origin" <(curl -sS --max-time 20 -o /dev/null -D - -X OPTIONS "$api/uploads" \
  -H "Origin: $origin" -H "Access-Control-Request-Method: POST" \
  -H "Access-Control-Request-Headers: authorization,content-type,x-timeline-session")
label="the upload bucket accepts the page's address (CORS)"
check grep -qi "^access-control-allow-origin: $origin" <(curl -sS --max-time 20 -o /dev/null -D - -X OPTIONS \
  "https://$(output RawUploadsBucketName).s3.$AWS_REGION.amazonaws.com/check" \
  -H "Origin: $origin" -H "Access-Control-Request-Method: PUT" -H "Access-Control-Request-Headers: content-type")
label="Cognito returns sign-ins to $page_url"
check grep -qx "$page_url" <(aws cognito-idp describe-user-pool-client --user-pool-id "$(output UserPoolId)" \
  --client-id "$(output UserPoolClientId)" --query 'UserPoolClient.CallbackURLs' --output text | tr '\t' '\n')
[ "$failed" = 0 ] || die "the live site failed a check (above)"

echo
echo "Deployed: $page_url"
# The name the domain is an alias of (its CNAME), or nothing if it has none.
alias_of="$(python3 -c 'import socket,sys
try: print(socket.gethostbyname_ex(sys.argv[1])[0])
except OSError as e: print(f"(no DNS answer: {e})", file=sys.stderr)' "$host" 2>/dev/null || true)"
if [ "$alias_of" != "$target" ]; then
  echo "$host doesn't point at this deployment yet. At Porkbun: CNAME ${host%.telotrope.ai} -> $target"
fi
