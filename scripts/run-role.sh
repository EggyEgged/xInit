#!/usr/bin/env bash
set -euo pipefail
# run-role.sh <dir> <prompt-file> <output-file> <allowed-tools> [holdout-dir]
#
# Launch one autopilot role as a fresh headless Claude Code session
# (docs/build-prompts.md, section 4). It cds into <dir> and runs
#     claude -p "<prompt>" --model claude-opus-5-5 --permission-mode acceptEdits
#            --allowedTools "<allowed-tools>" --output-format json > <output-file>
#
# <allowed-tools> is one argument, comma separated, for example
#     "Read,Edit,Write,Bash(cargo:*),Bash(git:*)"
#
# Blind roles (test author, hardening) pass [holdout-dir]. The script then
#   - exports XINIT_HOLDOUT_DIR=<holdout-dir>,
#   - adds --add-dir <holdout-dir> /tmp/xinit-scratch,
#   - disallows Read, Edit, Write, Glob and Grep on the main repository path,
#   - refuses to run if <dir> is inside the main repository.
# The prompt and output files may live anywhere; paths are resolved before cd.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="claude-opus-5-5"
SCRATCH="/tmp/xinit-scratch"
fail() { echo "run-role: FAILED: $*" >&2; exit 1; }

[[ $# -eq 4 || $# -eq 5 ]] || fail "usage: scripts/run-role.sh <dir> <prompt-file> <output-file> <allowed-tools> [holdout-dir]"
DIR="$(realpath -e "$1")" || fail "dir $1 does not exist"
PROMPT_FILE="$(realpath -e "$2")" || fail "prompt file $2 does not exist"
OUT="$(realpath -m "$3")"
TOOLS="$4"
HOLDOUT="${5:-}"

[[ -s "$PROMPT_FILE" ]] || fail "prompt file $PROMPT_FILE is empty"
if grep -q '{{' "$PROMPT_FILE"; then fail "prompt file $PROMPT_FILE still contains a {{placeholder}}"; fi
[[ -n "$TOOLS" ]] || fail "allowed-tools is empty"
mkdir -p "$(dirname "$OUT")"

args=(-p "$(cat "$PROMPT_FILE")"
      --model "$MODEL"
      --permission-mode acceptEdits
      --allowedTools "$TOOLS"
      --output-format json)

if [[ -n "$HOLDOUT" ]]; then
    case "$DIR/" in "$ROOT"/*) fail "blind role dir $DIR is inside the repository $ROOT" ;; esac
    HOLDOUT="$(realpath -e "$HOLDOUT")" || fail "holdout dir $5 does not exist"
    case "$HOLDOUT/" in "$ROOT"/*|"$DIR"/*) fail "holdout dir $HOLDOUT must be outside the repository and the workspace" ;; esac
    mkdir -p "$SCRATCH"
    export XINIT_HOLDOUT_DIR="$HOLDOUT"
    # Permission rules take absolute paths with a leading "//".
    deny="Read(/$ROOT/**),Edit(/$ROOT/**),Write(/$ROOT/**),Glob(/$ROOT/**),Grep(/$ROOT/**)"
    args+=(--add-dir "$HOLDOUT" "$SCRATCH" --disallowedTools "$deny")
fi

echo "run-role: dir=$DIR prompt=$PROMPT_FILE out=$OUT model=$MODEL holdout=${HOLDOUT:-none}" >&2
cd "$DIR"
status=0
claude "${args[@]}" > "$OUT" || status=$?
echo "run-role: claude exited $status" >&2
exit "$status"
