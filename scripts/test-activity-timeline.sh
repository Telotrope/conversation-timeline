#!/usr/bin/env bash
#
# Tests activity-timeline.sh against a stand-in for the `aws` command, which
# records what it was asked and answers `logs filter-log-events` with canned
# output per log group. Run manually:
#
#   scripts/test-activity-timeline.sh
#
# The canned log lines follow the activity-recording contract and the plan's
# formats (docs/plans/2026-10-02-activity-instrumentation.md §2-§5); the
# answer shape follows the AWS CLI's documented filter-log-events output,
# with `NextToken` as the CLI's pagination token. None were captured from a
# real deployment; the plan's end-to-end check (§8, "On AWS") does that.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
pass() { echo "  PASS: $1"; }
fail() { echo "  FAIL: $1" >&2; failures=$((failures + 1)); }

# The stand-in `aws`: logs its arguments, then answers from
# $work/answer<group with / as _>[-<starting token>].json. A group with a
# $work/missing<group> file is answered the way aws answers a group that
# does not exist; while $work/deny exists every call is refused.
mkdir -p "$work/bin"
cat > "$work/bin/aws" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$STUB_DIR/argv.log"
group=""; token=""; prev=""
for arg in "$@"; do
  [ "$prev" = "--log-group-name" ] && group="$arg"
  [ "$prev" = "--starting-token" ] && token="$arg"
  prev="$arg"
done
name="${group//\//_}"
if [ -e "$STUB_DIR/deny" ]; then
  echo "An error occurred (AccessDeniedException) when calling the FilterLogEvents operation: not allowed" >&2
  exit 254
fi
if [ -e "$STUB_DIR/missing$name" ]; then
  echo "An error occurred (ResourceNotFoundException) when calling the FilterLogEvents operation: The specified log group does not exist." >&2
  exit 254
fi
cat "$STUB_DIR/answer$name${token:+-$token}.json"
STUB
chmod +x "$work/bin/aws"
export STUB_DIR="$work" PATH="$work/bin:$PATH"

# Times: T = 1790000000000 ms (2026-09-21 14:13:20 UTC). Session S1 is
# 11111111-…, S2 is 22222222-…. The page's clock runs 6 s ahead of AWS's for
# its view of request reqA, which must still land next to the server's line.
cat > "$work/answer_timeline_dev_api-access.json" <<'JSON'
{"events": [
  {"logStreamName": "s", "timestamp": 1790000003000, "eventId": "1",
   "message": "{\"requestId\":\"reqA\",\"requestTime\":\"21/Sep/2026:14:13:23 +0000\",\"httpMethod\":\"POST\",\"routeKey\":\"POST /detect\",\"path\":\"/detect\",\"status\":\"200\",\"responseLatency\":\"3300\",\"integrationLatency\":\"3210\",\"authorizerError\":\"-\",\"errorMessage\":\"-\"}"},
  {"logStreamName": "s", "timestamp": 1790000000500, "eventId": "2",
   "message": "{\"requestId\":\"reqR\",\"requestTime\":\"21/Sep/2026:14:13:20 +0000\",\"httpMethod\":\"GET\",\"routeKey\":\"GET /conversations\",\"path\":\"/conversations\",\"status\":\"401\",\"responseLatency\":\"4\",\"integrationLatency\":\"-\",\"authorizerError\":\"Unauthorized\",\"errorMessage\":\"Unauthorized\"}"}
]}
JSON
cat > "$work/answer_timeline_dev_api.json" <<'JSON'
{"events": [
  {"logStreamName": "s", "timestamp": 1790000003200, "eventId": "3",
   "message": "{\"kind\":\"api_request\",\"request_id\":\"reqA\",\"method\":\"POST\",\"route\":\"/detect\",\"status\":200,\"ms\":3210,\"user\":\"sub-1\",\"session\":\"11111111-1111-4111-8111-111111111111\",\"facts\":{\"offset\":0,\"limit\":32,\"conversations_processed\":32,\"messages_detected\":7},\"aws_calls\":{\"DynamoDB.PutItem\":410},\"aws_retries\":2,\"aws_failures\":{\"DynamoDB.PutItem\":1},\"aws_errors\":[\"DynamoDB.PutItem: throttled\"]}\n"},
  {"logStreamName": "s", "timestamp": 1790000001000, "eventId": "4",
   "message": "{\"kind\":\"api_request\",\"request_id\":\"reqU\",\"method\":\"POST\",\"route\":\"/uploads\",\"status\":200,\"ms\":41,\"user\":\"sub-1\",\"session\":\"11111111-1111-4111-8111-111111111111\",\"facts\":{\"upload_id\":\"aaaaaaaa-0000-4000-8000-000000000001\"},\"aws_calls\":{\"DynamoDB.PutItem\":1},\"aws_retries\":0}"},
  {"logStreamName": "s", "timestamp": 1790000000100, "eventId": "5",
   "message": "{\"kind\":\"api_request\",\"request_id\":\"reqB\",\"method\":\"POST\",\"route\":\"/uploads\",\"status\":200,\"ms\":40,\"user\":\"sub-2\",\"session\":\"22222222-2222-4222-8222-222222222222\",\"facts\":{\"upload_id\":\"bbbbbbbb-0000-4000-8000-000000000002\"},\"aws_calls\":{},\"aws_retries\":0}"},
  {"logStreamName": "s", "timestamp": 1790000003201, "eventId": "6",
   "message": "START RequestId: 9f0e Version: $LATEST\n"},
  {"logStreamName": "s", "timestamp": 1790000003202, "eventId": "7",
   "message": "REPORT RequestId: 9f0e\tDuration: 3212.40 ms\tBilled Duration: 3300 ms\tMemory Size: 256 MB\tMax Memory Used: 61 MB\tInit Duration: 88.10 ms\t\n"},
  {"logStreamName": "s", "timestamp": 1790000004000, "eventId": "8",
   "message": "storage error: \u001b[31mthrottled\u0007 again"},
  {"logStreamName": "s", "timestamp": 1790000004100, "eventId": "9",
   "message": "{not json at all"}
]}
JSON
cat > "$work/answer_timeline_dev_activity.json" <<'JSON'
{"events": [
  {"logStreamName": "s", "timestamp": 1790000020000, "eventId": "10",
   "message": "{\"kind\":\"page_event\",\"request_id\":\"reqP\",\"user\":\"sub-1\",\"session\":\"11111111-1111-4111-8111-111111111111\",\"event\":{\"kind\":\"click\",\"t\":1790000000000,\"tab\":\"load\",\"target\":{\"tag\":\"button\",\"id\":\"loadBtn\"}}}"},
  {"logStreamName": "s", "timestamp": 1790000020000, "eventId": "10b",
   "message": "{\"kind\":\"page_event\",\"request_id\":\"reqP\",\"user\":\"sub-1\",\"session\":\"11111111-1111-4111-8111-111111111111\",\"event\":{\"kind\":\"shown\",\"t\":1790000000050,\"tab\":\"review\",\"where\":\"saveStatus\",\"message\":\"save.stale_page\",\"is_error\":true,\"status\":403,\"error_kind\":\"stale_page\",\"page_version\":\"a1b2c3d\"}}"},
  {"logStreamName": "s", "timestamp": 1790000020001, "eventId": "11",
   "message": "{\"kind\":\"page_event\",\"request_id\":\"reqP\",\"user\":\"sub-1\",\"session\":\"11111111-1111-4111-8111-111111111111\",\"event\":{\"kind\":\"request\",\"t\":1790000009000,\"tab\":\"review\",\"method\":\"POST\",\"route\":\"/detect\",\"status\":200,\"ms\":3350,\"request_id\":\"reqA\",\"offset\":0,\"limit\":32}}"},
  {"logStreamName": "s", "timestamp": 1790000020002, "eventId": "12",
   "message": "{\"kind\":\"page_event\",\"request_id\":\"reqQ\",\"user\":\"sub-2\",\"session\":\"22222222-2222-4222-8222-222222222222\",\"event\":{\"kind\":\"submit\",\"t\":1790000000200,\"tab\":\"\",\"target\":{\"tag\":\"input\",\"id\":\"convSearch\"},\"text\":\"evil‮txt.exe\"}}"}
], "NextToken": "tok1"}
JSON
cat > "$work/answer_timeline_dev_activity-tok1.json" <<'JSON'
{"events": [
  {"logStreamName": "s", "timestamp": 1790000020003, "eventId": "13",
   "message": "{\"kind\":\"page_event\",\"request_id\":\"reqP\",\"user\":\"sub-1\",\"session\":\"11111111-1111-4111-8111-111111111111\",\"event\":{\"kind\":\"view\",\"t\":1790000005000,\"tab\":\"review\",\"view\":\"review\",\"via\":\"hashchange\"}}"}
]}
JSON
cat > "$work/answer_timeline_dev_process-upload.json" <<'JSON'
{"events": [
  {"logStreamName": "s", "timestamp": 1790000002000, "eventId": "14",
   "message": "{\"kind\":\"processing_run\",\"key\":\"raw/sub-1/aaaaaaaa-0000-4000-8000-000000000001.json\",\"user\":\"sub-1\",\"upload_id\":\"aaaaaaaa-0000-4000-8000-000000000001\",\"outcome\":\"ready\",\"ms\":6100,\"facts\":{\"bytes\":60600000,\"conversations\":812,\"reviews\":0},\"aws_calls\":{\"DynamoDB.PutItem\":1624},\"aws_retries\":0}"},
  {"logStreamName": "s", "timestamp": 1790000002500, "eventId": "15",
   "message": "{\"kind\":\"processing_run\",\"key\":\"raw/sub-2/bbbbbbbb-0000-4000-8000-000000000002.json\",\"user\":\"sub-2\",\"upload_id\":\"bbbbbbbb-0000-4000-8000-000000000002\",\"outcome\":\"unusable\",\"error\":\"not a JSON array\",\"ms\":90,\"facts\":{\"bytes\":10,\"conversations\":0,\"reviews\":0},\"aws_calls\":{},\"aws_retries\":0}"}
]}
JSON
touch "$work/missing_timeline_dev_record-failed-upload"

# Line number of the first output line containing $2 in file $1 (0 if none).
line_of() { grep -n -F -- "$2" "$1" | head -1 | cut -d: -f1 || true; }

echo "activity-timeline.sh, whole stage"
if "$REPO_ROOT/scripts/activity-timeline.sh" dev --since 2h > "$work/out.txt" 2> "$work/err.txt"; then
  pass "succeeds although one log group is missing"
else
  fail "failed: $(cat "$work/err.txt")"
fi
cat "$work/out.txt"
grep -qF "/timeline/dev/record-failed-upload not found" "$work/err.txt" && pass "names the missing log group" \
  || fail "did not name the missing log group: $(cat "$work/err.txt")"
before=$failures
for name in api-access api activity process-upload record-failed-upload; do
  grep -qF -- "--log-group-name /timeline/dev/$name " "$work/argv.log" || fail "never read /timeline/dev/$name"
done
[ "$failures" -eq "$before" ] && pass "asks for all five log groups"

detect="$(grep -F "api     request   POST /detect" "$work/out.txt" || true)"
[[ "$detect" == *"[gateway: POST /detect -> 200 3300 ms (our code 3210 ms)]"* ]] \
  && pass "joins the gateway's line to the API's by request ID" || fail "no joined /detect line: $detect"
[ "$(grep -c "gateway" "$work/out.txt")" -eq 2 ] && pass "a joined gateway line is not printed again on its own" \
  || fail "gateway lines: $(grep gateway "$work/out.txt")"
grep -qF "gateway request   GET /conversations -> 401 4 ms, sign-in refused: Unauthorized" "$work/out.txt" \
  && pass "prints a refused request that never reached our code" || fail "no refused-request line"

click="$(line_of "$work/out.txt" 'page    click     button#loadBtn')"
refused="$(line_of "$work/out.txt" "GET /conversations -> 401")"
uploads="$(line_of "$work/out.txt" "POST /uploads upload_id aaaaaaaa")"
run="$(line_of "$work/out.txt" "process run       upload aaaaaaaa")"
api_detect="$(line_of "$work/out.txt" "api     request   POST /detect")"
page_detect="$(line_of "$work/out.txt" "page    request   POST /detect offset 0 limit 32 -> 200 3.4 s")"
view="$(line_of "$work/out.txt" "page    view      review (via hashchange)")"
if [ "$click" -gt 0 ] && [ "$click" -lt "$refused" ] && [ "$refused" -lt "$uploads" ] \
   && [ "$uploads" -lt "$run" ] && [ "$run" -lt "$api_detect" ] && [ "$api_detect" -lt "$view" ]; then
  pass "sorts by time (page events by the page's own clock)"
else
  fail "out of order: click $click, 401 $refused, uploads $uploads, run $run, detect $api_detect, view $view"
fi
[ "$page_detect" -eq $((api_detect + 1)) ] \
  && pass "places the page's view of a request right after the server's line for it" \
  || fail "page's /detect at line $page_detect, server's at $api_detect"
grep -qF "14:13:23.200 api" "$work/out.txt" && pass "prints UTC times with milliseconds" \
  || fail "no 14:13:23.200 line"
grep -qF "lambda  report    [api] took 3212.40 ms, memory 61 MB, of 256 MB, start-up 88.10 ms" "$work/out.txt" \
  && pass "summarises Lambda's REPORT line" || fail "no REPORT summary"
grep -qF "START RequestId" "$work/out.txt" && fail "printed Lambda's START line" \
  || pass "leaves out Lambda's START line"
grep -qF "lambda  text      [api] {not json at all" "$work/out.txt" && pass "prints a line that is not valid JSON as raw text" \
  || fail "dropped the line that is not JSON"
if LC_ALL=C grep -q $'[\x01-\x08\x0b-\x1f\x7f]' "$work/out.txt" || grep -qF $'‮' "$work/out.txt"; then
  fail "printed control or invisible characters"
else
  pass "strips control and invisible characters"
fi
grep -qF "storage error: [31mthrottled again" "$work/out.txt" && grep -qF 'input#convSearch text "eviltxt.exe"' "$work/out.txt" \
  && pass "keeps the text around the stripped characters" || fail "lost text around stripped characters"
grep -qF "page    shown     saveStatus: save.stale_page (error) status 403, error_kind stale_page  [page a1b2c3d]" "$work/out.txt" \
  && pass "prints a shown message by its identifier, values and page version" \
  || fail "shown line: $(grep shown "$work/out.txt")"
grep -qF "retries 2, FAILED DynamoDB.PutItem 1 (DynamoDB.PutItem: throttled)" "$work/out.txt" \
  && pass "prints retries and final AWS failures apart" \
  || fail "failures: $(grep 'POST /detect' "$work/out.txt")"
grep -qF -- "--starting-token tok1" "$work/argv.log" && [ "$view" -gt 0 ] \
  && pass "follows the pagination token and prints the next page's events" \
  || fail "did not follow the pagination token: $(cat "$work/argv.log")"

start="$(grep -o -- "--start-time [0-9]*" "$work/argv.log" | head -1 | cut -d' ' -f2)"
expected=$(( $(date +%s) * 1000 - 7200000 ))
[ $((start - expected)) -gt -60000 ] && [ $((start - expected)) -lt 60000 ] \
  && pass "--since 2h asks for events from 2 hours ago" || fail "--since 2h gave start time $start, expected about $expected"

echo "activity-timeline.sh --session"
if "$REPO_ROOT/scripts/activity-timeline.sh" dev --session 11111111-1111-4111-8111-111111111111 \
   > "$work/out.txt" 2> "$work/err.txt"; then
  before=$failures
  for want in 'button#loadBtn' "POST /detect offset 0" "upload aaaaaaaa" "[gateway: POST /detect"; do
    grep -qF -- "$want" "$work/out.txt" || fail "session filter lost: $want"
  done
  for unwanted in "bbbbbbbb" "evil" "GET /conversations" "report" "not json"; do
    if grep -qF -- "$unwanted" "$work/out.txt"; then fail "session filter kept: $unwanted"; fi
  done
  [ "$failures" -eq "$before" ] && pass "keeps the session's page, API and joined gateway entries and its upload's processing run, and nothing else"
  grep -qF "left out by --session" "$work/err.txt" && pass "says how many lines without a session it left out" \
    || fail "did not report the lines it left out: $(cat "$work/err.txt")"
else
  fail "failed with --session: $(cat "$work/err.txt")"
fi

echo "activity-timeline.sh, bad input and aws failures"
: > "$work/argv.log"
if "$REPO_ROOT/scripts/activity-timeline.sh" dev --since 5x > "$work/out.txt" 2>&1; then
  fail "accepted --since 5x"
else
  grep -qF -- "--since '5x'" "$work/out.txt" && pass "refuses a bad --since value, naming it" \
    || fail "did not name the bad value: $(cat "$work/out.txt")"
fi
[ ! -s "$work/argv.log" ] && pass "calls aws only after the arguments are valid" || fail "called aws anyway"
if "$REPO_ROOT/scripts/activity-timeline.sh" "../evil" > "$work/out.txt" 2>&1; then
  fail "accepted a stage name with a path in it"
else
  pass "refuses a stage name with a path in it"
fi
touch "$work/deny"
if "$REPO_ROOT/scripts/activity-timeline.sh" dev > "$work/out.txt" 2>&1; then
  fail "succeeded although aws refused"
else
  grep -qF "AccessDeniedException" "$work/out.txt" && pass "stops with aws's error on any other failure" \
    || fail "did not pass on aws's error: $(cat "$work/out.txt")"
fi
rm "$work/deny"

echo
if [ "$failures" -eq 0 ]; then echo "All passed."; else echo "$failures failed." >&2; exit 1; fi
