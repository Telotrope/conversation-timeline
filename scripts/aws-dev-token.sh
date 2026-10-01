#!/usr/bin/env bash
# Prints an access token for a user of the deployed dev stack, for checking
# the API with curl (migration plan §V2e, E7). Works only on the dev stage,
# whose login client allows username-and-password sign-in (plan C27).
#
# Usage: scripts/aws-dev-token.sh <email>
#   then: curl -H "Authorization: Bearer $TOKEN" <ApiUrl>/conversations
# Asks for the password without showing it, and passes it to the aws tool
# on its standard input, not its command line, where other programs on
# this machine could see it.
set -euo pipefail

email="${1:?usage: scripts/aws-dev-token.sh <email>}"
client_id="$(aws cloudformation describe-stacks --stack-name timeline-dev \
  --query "Stacks[0].Outputs[?OutputKey=='UserPoolClientId'].OutputValue" --output text)"
read -r -s -p "Password for $email: " password
echo >&2

EMAIL="$email" PASSWORD="$password" CLIENT_ID="$client_id" python3 -c '
import json, os
print(json.dumps({
    "AuthFlow": "USER_PASSWORD_AUTH",
    "ClientId": os.environ["CLIENT_ID"],
    "AuthParameters": {"USERNAME": os.environ["EMAIL"], "PASSWORD": os.environ["PASSWORD"]},
}))' | aws cognito-idp initiate-auth --cli-input-json file:///dev/stdin \
  --query 'AuthenticationResult.AccessToken' --output text
