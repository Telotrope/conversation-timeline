#!/usr/bin/env bash
# Gets a certificate for a custom domain, once per domain
# (docs/plans/2026-10-02-deployment-operating-guide.md §3): requests it from
# Certificate Manager in us-east-1 (the only region CloudFront accepts),
# prints the DNS record to add at Porkbun, waits until AWS has checked it,
# and prints the identifier to put in infra/samconfig.toml as
# PageCertificateArn.
#
# A certificate already requested or issued for the domain is reused, so
# running this again never makes a second one.
#
# Usage: scripts/request-certificate.sh <domain>   (AWS_PROFILE, e.g. timeline)
set -euo pipefail

domain="${1:?usage: scripts/request-certificate.sh <domain>}"
if [[ ! "$domain" =~ ^([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,}$ ]]; then
  echo "\"$domain\" is not a lowercase domain name." >&2
  exit 1
fi
acm() { aws acm "$@" --region us-east-1; }

arn="$(acm list-certificates --certificate-statuses PENDING_VALIDATION ISSUED \
  --query "CertificateSummaryList[?DomainName=='$domain'] | [0].CertificateArn" --output text)"
if [ -z "$arn" ] || [ "$arn" = "None" ]; then
  arn="$(acm request-certificate --domain-name "$domain" --validation-method DNS \
    --query CertificateArn --output text)"
  echo "Requested a certificate for $domain."
else
  echo "Reusing the certificate already requested for $domain."
fi

# AWS takes a few seconds to choose the validation record.
record=""
for _ in $(seq 1 30); do
  record="$(acm describe-certificate --certificate-arn "$arn" \
    --query 'Certificate.DomainValidationOptions[0].ResourceRecord.[Name,Value]' --output text)"
  [ -n "$record" ] && [ "$record" != "None" ] && break
  sleep "${REQUEST_CERTIFICATE_POLL:-2}"
done
if [ -z "$record" ] || [ "$record" = "None" ]; then
  echo "AWS didn't provide the validation record for $arn within a minute; run this again." >&2
  exit 1
fi
read -r name value <<< "$record"

status="$(acm describe-certificate --certificate-arn "$arn" --query Certificate.Status --output text)"
if [ "$status" != "ISSUED" ]; then
  cat <<EOF

Add this record at Porkbun (Domain Management -> DNS), and keep it: AWS
uses it again to renew the certificate.
  Type:   CNAME
  Host:   ${name%.telotrope.ai.}
  Answer: ${value%.}
(Host is the part before .telotrope.ai; for another domain, everything before it.)

Waiting for AWS to see it (checks every minute, up to 40 minutes)...
EOF
  if ! acm wait certificate-validated --certificate-arn "$arn"; then
    echo "Not validated yet. Once the record is in, run this again to keep waiting." >&2
    exit 1
  fi
fi
echo "Issued. In infra/samconfig.toml, for the deployment at $domain:"
echo "  PageCertificateArn=\"$arn\""
