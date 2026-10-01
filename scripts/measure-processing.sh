#!/usr/bin/env bash
# Measures processing one large upload, to set the processing Lambda's
# memory and time limits in infra/template.yaml (migration plan §V2e, E2).
#
# Builds a synthetic export of about 60 MB (the size of the real export this
# project was built around) with e2e/synthetic-export.js, then runs
# backend/timeline-api/examples/measure_processing.rs on it in a release
# build. Needs Node and the Rust toolchain.
#
# Usage: scripts/measure-processing.sh [target-megabytes]   (default 60)
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
target_mb="${1:-60}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

node - "$repo/e2e/synthetic-export.js" "$work/conversations.json" "$target_mb" <<'JS'
const fs = require('fs');
const [, , lib, out, targetMb] = process.argv;
const { syntheticExport } = require(lib);
// About 15 messages per conversation and 13 KB per message, close to the
// real export's shape (~4,500 messages in ~60 MB). Every fifth human
// message carries a review, so stored reviews are part of the cost.
const sentence = 'You said this was fixed but it STILL fails the same way, and I am not sure why. ';
const text = sentence.repeat(160);
const perConversation = 15;
// Each message carries its text twice (`text` and `content[0].text`).
const approxBytesPerConversation = perConversation * (2 * text.length + 600);
const count = Math.ceil((Number(targetMb) * 1024 * 1024) / approxBytesPerConversation);
const start = Date.UTC(2025, 0, 1);
const conversations = [];
for (let c = 0; c < count; c++) {
  const messages = [];
  for (let m = 0; m < perConversation; m++) {
    const human = m % 2 === 0;
    messages.push({
      sender: human ? 'human' : 'assistant',
      text,
      at: new Date(start + (c * perConversation + m) * 60000),
      review: human && m % 10 === 0 ? { angry: true } : undefined,
    });
  }
  conversations.push({ name: `Conversation ${c}`, messages });
}
fs.writeFileSync(out, syntheticExport(conversations));
JS

cd "$repo/backend"
cargo run --quiet --release -p timeline-api --example measure_processing -- "$work/conversations.json"
