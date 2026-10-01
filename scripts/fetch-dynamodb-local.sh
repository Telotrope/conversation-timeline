#!/usr/bin/env bash
#
# Downloads Amazon's DynamoDB Local into backend/.tools/dynamodb-local/,
# where timeline-storage's DynamoDB tests look for it.
#
#   scripts/fetch-dynamodb-local.sh
#
# Does nothing if the pinned version is already there. AWS's download
# address always serves the newest release, so the file is checked against
# a checksum pinned here, not against the one AWS publishes next to it
# (that one changes with every release). When AWS ships a new version this
# script stops instead of silently switching the tests to a version nobody
# chose. See docs/plans/2026-09-09-rust-aws-backend-migration.md, §V2b.
#
# DynamoDB Local is under AWS's own license, not an open-source one (see the
# same plan's C15). It may only be used on machines the AWS account holder
# owns or controls, and must never be committed or redistributed --
# backend/.tools/ is ignored by git for that reason.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${REPO_ROOT}/backend/.tools/dynamodb-local"
URL="https://d1ni2b6xgvw0s0.cloudfront.net/v2.x/dynamodb_local_latest.tar.gz"
# Version 3.3.1 (release notes dated 2026-05-28), downloaded 2026-09-30.
PINNED_SHA256="f80bcec477f85f57e2c77f8d54aa6b672a8403fceff0c450560aee1cf6c21163"
MARKER="${DEST}/.sha256"

if [[ -f "${MARKER}" && "$(cat "${MARKER}")" == "${PINNED_SHA256}" && -f "${DEST}/DynamoDBLocal.jar" ]]; then
  echo "DynamoDB Local (pinned version) is already in ${DEST}"
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

echo "Downloading ${URL}"
curl -sSfL -o "${TMP}/dynamodb_local.tar.gz" "${URL}"

ACTUAL_SHA256="$(sha256sum "${TMP}/dynamodb_local.tar.gz" | cut -d' ' -f1)"
if [[ "${ACTUAL_SHA256}" != "${PINNED_SHA256}" ]]; then
  echo "AWS is now serving a different DynamoDB Local than the pinned one." >&2
  echo "  pinned:     ${PINNED_SHA256}" >&2
  echo "  downloaded: ${ACTUAL_SHA256}" >&2
  echo "Read the new version's LICENSE.txt and release notes, then update" >&2
  echo "PINNED_SHA256 in this script if the new version is acceptable." >&2
  exit 1
fi

rm -rf "${DEST}"
mkdir -p "${DEST}"
tar -xzf "${TMP}/dynamodb_local.tar.gz" -C "${DEST}"
echo "${PINNED_SHA256}" > "${MARKER}"
echo "DynamoDB Local unpacked into ${DEST}"
