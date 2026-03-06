#!/usr/bin/env bash
# yoyo project analyzer — run locally to reproduce benchmark reports
#
# Usage:
#   ./reports/analyze.sh                          # analyze all pinned projects
#   ./reports/analyze.sh --project ripgrep        # single project
#   ./reports/analyze.sh --output /tmp/my-report  # custom output dir
#   CLONE_DIR=/tmp ./reports/analyze.sh           # reuse existing clones
#
# Dependencies: git, yoyo (in PATH), python3
# Bash 3.2+ compatible (macOS default shell)

set -euo pipefail

YOYO=${YOYO_BIN:-yoyo}
CLONE_DIR=${CLONE_DIR:-/tmp/yoyo-bench}
OUTPUT_DIR=${OUTPUT_DIR:-"$(dirname "$0")/runs/$(date +%Y-%m-%d)"}
ONLY_PROJECT=""

# ── parse args ────────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case $1 in
    --project)   ONLY_PROJECT="$2"; shift 2 ;;
    --output)    OUTPUT_DIR="$2";   shift 2 ;;
    --clone-dir) CLONE_DIR="$2";    shift 2 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

# ── project registry ──────────────────────────────────────────────────────────
PROJECTS="ripgrep flask gin express clap"

project_url() {
  case $1 in
    ripgrep) echo "https://github.com/BurntSushi/ripgrep" ;;
    flask)   echo "https://github.com/pallets/flask" ;;
    gin)     echo "https://github.com/gin-gonic/gin" ;;
    express) echo "https://github.com/expressjs/express" ;;
    clap)    echo "https://github.com/clap-rs/clap" ;;
    *) echo ""; return 1 ;;
  esac
}

project_commit() {
  case $1 in
    ripgrep) echo "4519153e5e461527f4bca45b042fff45c4ec6fb9" ;;
    flask)   echo "3a9d54f3da1de540adfdf6f1e2dea6fc0006e15d" ;;
    gin)     echo "3e44fdc4d1636a2b1599c6688a76e13216a413dd" ;;
    express) echo "6c4249feec8ab40631817c8e7001baf2ed022224" ;;
    clap)    echo "1536fb6c6c9715ecd0d1560c4865d10be3d7b7df" ;;
    *) echo ""; return 1 ;;
  esac
}

# ── helpers ───────────────────────────────────────────────────────────────────
log() { echo "[analyze] $*"; }
die() { echo "[analyze] ERROR: $*" >&2; exit 1; }

require_cmd() { command -v "$1" &>/dev/null || die "$1 not found in PATH"; }
require_cmd git
require_cmd python3
require_cmd "$YOYO"

mkdir -p "$OUTPUT_DIR" "$CLONE_DIR"

clone_or_update() {
  local name=$1
  local url; url=$(project_url "$name")
  local commit; commit=$(project_commit "$name")
  local dir="$CLONE_DIR/$name"

  if [[ -d "$dir/.git" ]]; then
    log "$name: already cloned at $dir — resetting"
    git -C "$dir" checkout -q . 2>/dev/null || true
  else
    log "$name: cloning $url ..."
    git clone --depth=1 "$url" "$dir"
  fi
}

bake_project() {
  local name=$1
  local dir="$CLONE_DIR/$name"
  log "$name: baking index ..."
  "$YOYO" bake --path "$dir" > /dev/null
}

analyze_project() {
  local name=$1
  local dir="$CLONE_DIR/$name"
  local out="$OUTPUT_DIR/$name"
  mkdir -p "$out"

  log "$name: shake ..."
  "$YOYO" shake --path "$dir" > "$out/shake.json"

  log "$name: health ..."
  "$YOYO" health --path "$dir" > "$out/health.json"

  log "$name: architecture-map ..."
  "$YOYO" architecture-map --path "$dir" > "$out/architecture.json"

  log "$name: README ..."
  local readme=""
  for f in README.md readme.md README.rst README; do
    if [[ -f "$dir/$f" ]]; then readme="$dir/$f"; break; fi
  done
  if [[ -n "$readme" ]]; then
    head -20 "$readme" > "$out/readme_head.txt"
  else
    echo "(no README found)" > "$out/readme_head.txt"
  fi

  log "$name: done → $out/"
}

summarize() {
  local report="$OUTPUT_DIR/summary.md"
  local yoyo_version
  yoyo_version=$("$YOYO" --version 2>/dev/null | head -1 || echo "unknown")

  {
    echo "# yoyo benchmark summary"
    echo "**Date:** $(date +%Y-%m-%d)"
    echo "**yoyo:** $yoyo_version"
    echo ""
    echo "| Project | Files | Languages | Dead code | God fns | Max complexity |"
    echo "|---|---|---|---|---|---|"
  } > "$report"

  for name in $PROJECTS; do
    [[ -n "$ONLY_PROJECT" && "$name" != "$ONLY_PROJECT" ]] && continue
    local pdir="$OUTPUT_DIR/$name"
    [[ -d "$pdir" ]] || continue

    python3 - "$name" "$pdir" >> "$report" <<'PYEOF'
import sys, json, os
name, d = sys.argv[1], sys.argv[2]

def load(f):
    path = os.path.join(d, f)
    return json.load(open(path)) if os.path.exists(path) else {}

shake  = load("shake.json")
health = load("health.json")

files  = shake.get("files_indexed", "?")
langs  = ", ".join(shake.get("languages", []))
dead   = len(health.get("dead_code", []))
gods   = len(health.get("god_functions", []))
top    = shake.get("top_functions", [])
max_cx = top[0].get("complexity", "?") if top else "?"

print(f"| {name} | {files} | {langs} | {dead} | {gods} | {max_cx} |")
PYEOF
  done

  {
    echo ""
    echo "Raw JSON in \`$OUTPUT_DIR/<project>/\`"
    echo ""
    echo "Re-run: \`CLONE_DIR=$CLONE_DIR ./reports/analyze.sh --output $OUTPUT_DIR\`"
  } >> "$report"

  echo ""
  log "Summary → $report"
  echo "---"
  cat "$report"
}

# ── main ──────────────────────────────────────────────────────────────────────
run_list=$PROJECTS
if [[ -n "$ONLY_PROJECT" ]]; then
  project_url "$ONLY_PROJECT" > /dev/null || die "Unknown project: $ONLY_PROJECT. Valid: $PROJECTS"
  run_list=$ONLY_PROJECT
fi

log "Output:   $OUTPUT_DIR"
log "Projects: $run_list"
echo ""

for name in $run_list; do
  clone_or_update "$name"
  bake_project "$name"
  analyze_project "$name"
  echo ""
done

summarize
