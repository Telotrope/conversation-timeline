#!/usr/bin/env python3
"""Prints one timeline of a stage's recorded activity (plan
docs/plans/2026-10-02-activity-instrumentation.md, section 7).

Reads the stage's five CloudWatch log groups through the `aws` command,
turns each log line into one timeline entry, joins API Gateway's access-log
lines to our API's own `api_request` lines by API Gateway's request ID, sorts
by time and prints one line per entry:

    19:11:24.112 page    click     button#loadBtn "Load"
    19:11:36.400 api     request   POST /detect offset 0 limit 32 -> 200 3.2 s ...

Line formats follow the activity-recording contract (one JSON object per
line). Standard library only, so it needs nothing beyond Python 3 and the
`aws` command. Run it through scripts/activity-timeline.sh.
"""

import argparse
import datetime
import json
import re
import subprocess
import sys
import time
import unicodedata
from dataclasses import dataclass, field

# Which source column a log group's lines get, by the group's last part.
GROUPS = ("api-access", "api", "activity", "process-upload", "record-failed-upload")
# Lines longer than this (plain text, unknown JSON) are cut, so one huge
# line cannot flood the terminal.
RAW_LIMIT = 300
SINCE_UNITS = {"m": 60, "h": 3600, "d": 86400}
# Sort order of entries with the same time: the gateway's line, then our
# API's, then the page's view of the same request.
SOURCE_ORDER = {"gateway": 0, "api": 1, "process": 1, "lambda": 1, "page": 2}


class UsageError(Exception):
    """A bad command-line value; the message names it."""


class AwsError(Exception):
    """`aws` failed for a reason other than a missing log group."""


@dataclass
class Entry:
    """One timeline line, before printing."""

    time_ms: int
    source: str  # page, api, gateway, lambda or process
    kind: str
    details: str
    request_id: str | None = None
    session: str | None = None
    upload_id: str | None = None
    # Gateway access-log details folded into an `api` entry by the join.
    gateway: str | None = None
    order: int = field(default=0)


def parse_since(text):
    """'30m', '2h' or '1d' -> seconds. Anything else is a UsageError."""
    match = re.fullmatch(r"([1-9][0-9]{0,5})([mhd])", text)
    if not match:
        raise UsageError(f"--since {text!r} is not a number followed by m, h or d (e.g. 30m, 2h, 1d)")
    return int(match.group(1)) * SINCE_UNITS[match.group(2)]


def terminal_safe(text):
    """Removes control and invisible formatting characters (Unicode
    categories Cc and Cf: escape sequences, NUL, zero-width and
    right-to-left overrides), which could rewrite the terminal or disguise
    what a line says. Destination: a terminal."""
    return "".join(ch for ch in text if unicodedata.category(ch) not in ("Cc", "Cf"))


def cut(text, limit=RAW_LIMIT):
    text = str(text)
    return text if len(text) <= limit else text[:limit] + "…"


def fetch_group(group, start_ms):
    """All events of one log group since start_ms, following the AWS CLI's
    pagination token. Returns None when the group does not exist."""
    events, token = [], None
    while True:
        cmd = ["aws", "logs", "filter-log-events", "--log-group-name", group,
               "--start-time", str(start_ms), "--max-items", "10000", "--output", "json"]
        if token:
            cmd += ["--starting-token", token]
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            if "ResourceNotFoundException" in result.stderr:
                return None
            raise AwsError(result.stderr.strip() or f"aws exited with status {result.returncode}")
        try:
            page = json.loads(result.stdout)
        except json.JSONDecodeError as err:
            raise AwsError(f"aws answered with something other than JSON for {group}: {err}") from err
        events.extend(page.get("events", []))
        token = page.get("NextToken")
        if not token:
            return events


def duration(ms):
    """41 -> '41 ms'; 3210 -> '3.2 s'."""
    if not isinstance(ms, (int, float)):
        return f"{ms} ms"
    return f"{ms:.0f} ms" if ms < 1000 else f"{ms / 1000:.1f} s"


def size(n):
    if not isinstance(n, (int, float)):
        return str(n)
    if n >= 1_000_000:
        return f"{n / 1_000_000:.1f} MB"
    return f"{n / 1000:.1f} kB" if n >= 1000 else f"{n} B"


def short(value):
    """First 8 characters of an ID, enough to tell sessions apart."""
    return f"{str(value)[:8]}…" if value else "-"


def pairs(mapping):
    if not isinstance(mapping, dict):
        return str(mapping)
    return ", ".join(f"{k} {v}" for k, v in mapping.items())


def describe_target(target):
    if not isinstance(target, dict):
        return str(target)
    text = str(target.get("tag", "?"))
    if target.get("id"):
        text += f"#{target['id']}"
    if target.get("message_id"):
        text += f" message {target['message_id']}"
    if target.get("column"):
        text += f" column {target['column']}"
    for key in ("name", "tab", "analysis"):
        if target.get(key):
            text += f" {key} {target[key]}"
    return text


# The live values a `shown` record may carry (page-messages.js's `record`).
SHOWN_VALUES = ("count", "attempt", "max_attempts", "status", "error_kind")


def describe_page_event(event):
    kind = event.get("kind")
    if kind == "request":
        text = f"{event.get('method', '?')} {event.get('route', '?')}"
        for key in ("offset", "limit"):
            if key in event:
                text += f" {key} {event[key]}"
        if "scan" in event:
            text += f" scan {'on' if event['scan'] else 'off'}"
        status = event.get("status")
        text += f" -> {status if status is not None else 'no answer'} {duration(event.get('ms'))}"
        if "bytes" in event:
            text += f" {size(event['bytes'])}"
        if event.get("error_kind"):
            text += f" error: {event['error_kind']}"
        return text
    if kind == "view":
        return f"{event.get('view', '?')} (via {event.get('via', '?')})"
    if kind == "shown":
        # The page records which message it showed, not the wording; the
        # wording is in frontend/ui/widgets/page-messages.js at page_version.
        flag = " (error)" if event.get("is_error") else ""
        values = {k: event[k] for k in SHOWN_VALUES if k in event}
        text = f'{event.get("where", "?")}: {event.get("message", "?")}{flag}'
        if values:
            text += f" {pairs(values)}"
        return text + f"  [page {event.get('page_version', '?')}]"
    text = describe_target(event.get("target"))
    if kind == "change":
        if "file_size" in event or "file_ext" in event:
            text += f" -> file {size(event.get('file_size'))} .{event.get('file_ext', '?')}"
        else:
            text += f" -> {json.dumps(event.get('value'))}"
    elif kind == "submit":
        text += f' text "{event.get("text", "")}"'
    if event.get("tab"):
        text += f"  (tab {event['tab']})"
    return text


def from_json(record, ts):
    """An entry for a contract-format JSON line, or None if `record` is not one."""
    kind = record.get("kind")
    if kind == "api_request":
        facts = record.get("facts") or {}
        text = f"{record.get('method', '?')} {record.get('route', '?')}"
        if facts:
            text += f" {pairs(facts)}"
        text += f" -> {record.get('status', '?')} {duration(record.get('ms'))}"
        if record.get("aws_calls"):
            text += f", {pairs(record['aws_calls'])}"
        if record.get("aws_retries"):
            text += f", retries {record['aws_retries']}"
        if record.get("aws_failures"):
            text += f", FAILED {pairs(record['aws_failures'])}"
        if record.get("aws_errors"):
            text += f" ({'; '.join(record['aws_errors'])})"
        text += f" (session {short(record.get('session'))}, user {short(record.get('user'))})"
        upload = facts.get("upload_id") if isinstance(facts, dict) else None
        return Entry(ts, "api", "request", text, record.get("request_id"), record.get("session"), upload)
    if kind == "page_event":
        event = record.get("event")
        if not isinstance(event, dict):
            return None
        t = event.get("t")
        when = t if isinstance(t, int) else ts
        return Entry(when, "page", str(event.get("kind", "?")), describe_page_event(event),
                     event.get("request_id"), record.get("session"))
    if kind == "processing_run":
        text = f"upload {short(record.get('upload_id'))} -> {record.get('outcome', '?')}"
        if record.get("error"):
            text += f" ({record['error']})"
        if record.get("facts"):
            text += f", {pairs(record['facts'])}"
        text += f", {duration(record.get('ms'))}"
        if record.get("aws_calls"):
            text += f", {pairs(record['aws_calls'])}"
        if record.get("aws_retries"):
            text += f", retries {record['aws_retries']}"
        if record.get("aws_failures"):
            text += f", FAILED {pairs(record['aws_failures'])}"
        if record.get("aws_errors"):
            text += f" ({'; '.join(record['aws_errors'])})"
        return Entry(ts, "process", "run", text, upload_id=record.get("upload_id"))
    if kind == "upload_marked_failed":
        text = (f"upload {short(record.get('upload_id'))} marked failed: {record.get('reason', '?')}"
                f" -> {record.get('outcome', '?')}")
        if record.get("error"):
            text += f" ({record['error']})"
        return Entry(ts, "process", "failed", text, upload_id=record.get("upload_id"))
    if "requestId" in record and "routeKey" in record:
        return Entry(ts, "gateway", "request", describe_access(record), record.get("requestId"))
    return None


def describe_access(record):
    def present(key):
        value = record.get(key)
        return value if value not in (None, "", "-") else None

    text = f"{record.get('httpMethod', '?')} {record.get('path', '?')} -> {record.get('status', '?')}"
    if present("responseLatency"):
        text += f" {record['responseLatency']} ms"
    if present("integrationLatency"):
        text += f" (our code {record['integrationLatency']} ms)"
    for key, label in (("authorizerError", "sign-in refused"), ("errorMessage", "error")):
        if present(key):
            text += f", {label}: {record[key]}"
    return text


REPORT_FIELDS = (("Duration", "took"), ("Max Memory Used", "memory"), ("Memory Size", "of"),
                 ("Init Duration", "start-up"))


def from_text(message, ts, group):
    """An entry for a non-JSON line: Lambda's REPORT and INIT_START lines
    briefly, START and END left out (REPORT covers the same invocation),
    anything else as-is. None means "leave out"."""
    if message.startswith(("START RequestId:", "END RequestId:")):
        return None
    if message.startswith("REPORT RequestId:"):
        parts = dict(p.split(": ", 1) for p in message.split("\t") if ": " in p)
        text = ", ".join(f"{label} {parts[key].strip()}" for key, label in REPORT_FIELDS if key in parts)
        return Entry(ts, "lambda", "report", f"[{group}] {text}")
    if message.startswith("INIT_START"):
        return Entry(ts, "lambda", "start-up", f"[{group}] {cut(message)}")
    return Entry(ts, "lambda", "text", f"[{group}] {cut(message)}")


def parse_event(event, group):
    message = str(event.get("message", "")).rstrip("\n")
    ts = event.get("timestamp", 0)
    if message.lstrip().startswith("{"):
        try:
            record = json.loads(message)
        except json.JSONDecodeError:
            record = None  # not valid JSON: shown as raw text below, never dropped
        if isinstance(record, dict):
            entry = from_json(record, ts)
            if entry is not None:
                return entry
    return from_text(message, ts, group)


def join_and_filter(entries, session):
    """Folds each gateway line into the `api` line with the same request ID,
    places the page's view of a request at the server's time for that
    request, and, given a session, keeps only that session's entries.
    Returns (kept entries, how many were left out for lacking a session)."""
    api_by_id = {e.request_id: e for e in entries if e.source == "api" and e.request_id}
    gateway_by_id = {e.request_id: e for e in entries if e.source == "gateway" and e.request_id}
    joined = []
    for e in entries:
        if e.source == "gateway" and e.request_id in api_by_id:
            api_by_id[e.request_id].gateway = e.details
            continue
        if e.source == "page" and e.kind == "request" and e.request_id:
            server = api_by_id.get(e.request_id) or gateway_by_id.get(e.request_id)
            if server is not None:
                e.time_ms = server.time_ms
        joined.append(e)
    if session is None:
        return joined, 0
    ids = {e.request_id for e in joined if e.session == session and e.request_id}
    uploads = {e.upload_id for e in joined if e.session == session and e.upload_id}
    kept = [e for e in joined
            if e.session == session
            or (e.source == "gateway" and e.request_id in ids)
            or (e.source == "process" and e.upload_id in uploads)]
    kept_ids = {id(e) for e in kept}
    lacking = sum(1 for e in joined if e.session is None and id(e) not in kept_ids)
    return kept, lacking


def filter_upload(entries, upload):
    """Only one upload's entries: the API requests and processing lines naming
    it, and the gateway lines joined to those requests (used by
    scripts/diagnose-upload.sh; deployment guide plan §5)."""
    return [e for e in entries if e.upload_id == upload]


def render(entry):
    clock = datetime.datetime.fromtimestamp(entry.time_ms / 1000, datetime.timezone.utc)
    details = entry.details + (f"  [gateway: {entry.gateway}]" if entry.gateway else "")
    line = f"{clock:%H:%M:%S}.{entry.time_ms % 1000:03d} {entry.source:<7} {entry.kind:<9} {details}"
    return terminal_safe(line)


def main(argv):
    parser = argparse.ArgumentParser(prog="scripts/activity-timeline.sh",
                                     description="Print one timeline of a stage's recorded activity.")
    parser.add_argument("stage")
    parser.add_argument("--since", default="30m", help="how far back: e.g. 30m, 2h, 1d (default 30m)")
    parser.add_argument("--session", help="only this page session's entries")
    parser.add_argument("--upload", help="only this upload's requests and processing lines")
    args = parser.parse_args(argv)
    try:
        if not re.fullmatch(r"[A-Za-z0-9-]+", args.stage):
            raise UsageError(f"stage {args.stage!r} must be letters, digits and dashes only")
        start_ms = int((time.time() - parse_since(args.since)) * 1000)
        entries = []
        for name in GROUPS:
            group = f"/timeline/{args.stage}/{name}"
            events = fetch_group(group, start_ms)
            if events is None:
                print(f"log group {group} not found; continuing without it", file=sys.stderr)
                continue
            entries.extend(e for e in (parse_event(ev, name) for ev in events) if e is not None)
    except UsageError as err:
        print(f"activity-timeline: {err}", file=sys.stderr)
        return 2
    except AwsError as err:
        print(f"activity-timeline: aws failed: {terminal_safe(str(err))}", file=sys.stderr)
        return 1
    entries, lacking = join_and_filter(entries, args.session)
    if args.upload:
        entries = filter_upload(entries, args.upload)
    entries.sort(key=lambda e: (e.time_ms, SOURCE_ORDER.get(e.source, 1)))
    day = None
    for entry in entries:
        this_day = datetime.datetime.fromtimestamp(entry.time_ms / 1000, datetime.timezone.utc).date()
        if this_day != day:
            print(f"-- {this_day.isoformat()} (UTC) --")
            day = this_day
        print(render(entry))
    if lacking:
        print(f"{lacking} lines without a session ID (Lambda reports, plain text) left out by --session",
              file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
