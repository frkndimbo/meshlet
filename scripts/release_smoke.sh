#!/usr/bin/env bash
set -Eeuo pipefail

SMOKE_DIR="/tmp/meshlet-v04-smoke"
REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
BIN_SRC="$REPO_ROOT/target/debug/meshlet"
BIN="$SMOKE_DIR/meshlet"

log() {
    printf '[release-smoke] %s\n' "$*" >&2
}

die() {
    printf '[release-smoke] ERROR: %s\n' "$*" >&2
    exit 1
}

cleanup() {
    local status=$?
    if [[ "$SMOKE_DIR" == "/tmp/meshlet-v04-smoke" ]]; then
        rm -rf -- "$SMOKE_DIR"
    fi
    exit "$status"
}

trap cleanup EXIT

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

assert_contains() {
    local needle="$1"
    shift
    grep -RF -- "$needle" "$@" >/dev/null || die "missing expected text: $needle"
}

assert_absent() {
    local needle="$1"
    shift
    if grep -RF -- "$needle" "$@" >/dev/null; then
        die "unexpected text found: $needle"
    fi
}

assert_file() {
    local path="$1"
    [[ -f "$path" ]] || die "missing expected file: $path"
}

assert_count() {
    local needle="$1"
    local expected="$2"
    local path="$3"
    local actual
    actual="$(grep -Fxc -- "$needle" "$path" || true)"
    [[ "$actual" == "$expected" ]] || die "expected $expected occurrences of $needle in $path, found $actual"
}

mcp_call() {
    local request="$1"
    printf '%s\n' "$request" | "$BIN" serve --mcp stdio --profile public-safe
}

require_cmd cargo
require_cmd grep

cd "$REPO_ROOT"

log "build"
cargo build --locked
[[ -x "$BIN_SRC" ]] || die "missing built binary: $BIN_SRC"

log "fresh workspace"
rm -rf -- "$SMOKE_DIR"
mkdir -p "$SMOKE_DIR"
cp "$BIN_SRC" "$BIN"
cd "$SMOKE_DIR"

log "adopt"
"$BIN" init --adopt > "$SMOKE_DIR/adopt.json"
assert_file "$SMOKE_DIR/.meshlet/meshlet.db"
assert_file "$SMOKE_DIR/meshlet.toml"
assert_file "$SMOKE_DIR/.meshlet-okf/index.md"
assert_file "$SMOKE_DIR/.meshlet-okf/log.md"
assert_file "$SMOKE_DIR/.gitignore"
assert_count ".meshlet/" 1 "$SMOKE_DIR/.gitignore"
assert_count "graphify-out/" 1 "$SMOKE_DIR/.gitignore"

log "codex setup"
"$BIN" agent show codex > "$SMOKE_DIR/codex-show.json"
assert_contains '"meshlet_db_present": true' "$SMOKE_DIR/codex-show.json"
assert_contains '"meshlet_toml_present": true' "$SMOKE_DIR/codex-show.json"
assert_contains '[mcp_servers.meshlet]' "$SMOKE_DIR/codex-show.json"

log "refresh okf"
"$BIN" refresh --okf > "$SMOKE_DIR/refresh-okf.json"
"$BIN" okf doctor "$SMOKE_DIR/.meshlet-okf"

log "events"
"$BIN" event append --type context.added --json '{"label":"private-needle-v04"}'
"$BIN" event append --type context.added --visibility public --profile public-safe --json '{"label":"public-needle-v04"}'
"$BIN" verify

printf '%s\n' "dummy public evidence" > evidence.txt
"$BIN" event append --type evidence.attached --visibility public --profile public-safe --json '{"path":"/tmp/meshlet-v04-smoke/evidence.txt","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}'
"$BIN" doctor public

log "exports"
"$BIN" export public --out "$SMOKE_DIR/public.json"
"$BIN" export public --format okf --out "$SMOKE_DIR/okf"
"$BIN" okf doctor "$SMOKE_DIR/okf"

assert_contains "public-needle-v04" "$SMOKE_DIR/public.json" "$SMOKE_DIR/okf"
assert_absent "private-needle-v04" "$SMOKE_DIR/public.json" "$SMOKE_DIR/okf"
assert_absent "$SMOKE_DIR/evidence.txt" "$SMOKE_DIR/public.json" "$SMOKE_DIR/okf"

log "mcp public-safe"
mcp_call '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"meshlet_get_digest","arguments":{"limit":10}}}' \
    | grep "public-needle-v04" >/dev/null

mcp_call '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"meshlet_publish_event","arguments":{"type":"context.added","visibility":"public","payload":{"label":"must-reject"}}}}' \
    | grep '"error"' >/dev/null

"$BIN" event list --mode full > "$SMOKE_DIR/events.json"
assert_absent "must-reject" "$SMOKE_DIR/events.json"

log "ok"
