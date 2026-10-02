#!/usr/bin/env bash
# Prints an access token for a user of the deployed dev stack, for checking
# the API with curl (migration plan §V2e, E7). Works only on the dev stage,
# whose login client allows username-and-password sign-in (plan C27).
#
# Usage: scripts/aws-dev-token.sh <email>
#   then: curl -H "Authorization: Bearer $TOKEN" <ApiUrl>/conversations
# Asks for the password without showing it, and never puts it on a command
# line, where every user on this machine could read it while the command
# runs (ps, /proc/<pid>/cmdline).
#
# The password reaches aws in a temporary file only you can read (mktemp
# creates it with permissions 600), deleted when this script exits, on
# success, on an error or on Ctrl-C; it is left behind only if the script is
# killed outright (kill -9) or the machine stops. Not a pipe: aws-cli 2.37.8
# appears to open its --cli-input-json file twice, and a pipe is empty the
# second time (docs/plans/2026-10-02-dev-token-script-fix.md).
set -euo pipefail

email="${1:?usage: scripts/aws-dev-token.sh <email>}"
client_id="$(aws cloudformation describe-stacks --stack-name timeline-dev \
  --query "Stacks[0].Outputs[?OutputKey=='UserPoolClientId'].OutputValue" --output text)"
request="$(mktemp)"
trap 'rm -f "$request"' EXIT
read -r -s -p "Password for $email: " password
echo >&2

EMAIL="$email" PASSWORD="$password" CLIENT_ID="$client_id" python3 -c '
import json, os
print(json.dumps({
    "AuthFlow": "USER_PASSWORD_AUTH",
    "ClientId": os.environ["CLIENT_ID"],
    "AuthParameters": {"USERNAME": os.environ["EMAIL"], "PASSWORD": os.environ["PASSWORD"]},
}))' > "$request"
aws cognito-idp initiate-auth --cli-input-json "file://$request" \
  --query 'AuthenticationResult.AccessToken' --output text
