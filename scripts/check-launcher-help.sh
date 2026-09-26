#!/usr/bin/env bash
#
# Guard for akko.sh: every command the launcher can run must be documented in
# `akko --help`, and every cmd_* function must actually be reachable.
#
# The help text is the only description of this wrapper, and it drifts silently
# the moment a command is added — the same way the TUI help and the device
# database did. This turns that drift into a failed check.
set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# Optional argument: the launcher to check (defaults to the one beside this
# repo), so the guard itself can be run against a deliberately broken copy.
launcher="${1:-$here/../akko.sh}"

if [[ ! -f "$launcher" ]]; then
    echo "check-launcher-help: $launcher not found" >&2
    exit 1
fi

# The help text: the heredoc inside cmd_help.
help_text="$(awk '/^cmd_help\(\)/,/^}/' "$launcher" | sed -n '/<<EOF/,/^EOF$/p')"
if [[ -z "$help_text" ]]; then
    echo "check-launcher-help: could not extract the help text from $launcher" >&2
    exit 1
fi

# Commands the dispatcher routes, and the function each one calls.
mapfile -t routes < <(
    awk '/^main\(\)/,/^}/' "$launcher" \
        | grep -oE '^[[:space:]]+[a-z][a-z|-]*\)[[:space:]]+cmd_[a-z_]+' \
        | sed -E 's/^[[:space:]]+//; s/\)[[:space:]]+/ /' \
        | tr '|' '\n' | awk 'NF == 2 { print $1, $2 }' | sort -u
)

if [[ ${#routes[@]} -eq 0 ]]; then
    echo "check-launcher-help: no commands found in main()'s dispatcher" >&2
    exit 1
fi

status=0

# 1. Every dispatched command is named in the help text.
for entry in "${routes[@]}"; do
    read -r cmd fn <<<"$entry"
    case "$cmd" in
        help | -h | --help) continue ;;   # the help itself
    esac
    if ! grep -qE "(^|[^[:alnum:]_-])${cmd}([^[:alnum:]_-]|$)" <<<"$help_text"; then
        echo "check-launcher-help: 'akko --help' does not document '$cmd'" >&2
        status=1
    fi
done

# 2. Every cmd_* function is reachable from the dispatcher (no dead commands).
mapfile -t funcs < <(grep -oE '^cmd_[a-z_]+\(\)' "$launcher" | tr -d '()' | sort -u)
for fn in "${funcs[@]}"; do
    if ! grep -qE "^[[:space:]]+[a-z][a-z|-]*\)[[:space:]]+$fn\b" <<<"$(awk '/^main\(\)/,/^}/' "$launcher")"; then
        echo "check-launcher-help: $fn is defined but never dispatched" >&2
        status=1
    fi
done

# 3. The help must not advertise a command the dispatcher does not have.
while read -r advertised; do
    [[ -z "$advertised" ]] && continue
    found=0
    for entry in "${routes[@]}"; do
        read -r cmd _ <<<"$entry"
        [[ "$cmd" == "$advertised" ]] && { found=1; break; }
    done
    if [[ "$found" -eq 0 ]]; then
        echo "check-launcher-help: help advertises '$advertised', which is not dispatched" >&2
        status=1
    fi
done < <(awk '/^cmd_help\(\)/,/^}/' "$launcher" \
         | grep -oE '^  [a-z][a-z-]+ ' | tr -d ' ' | sort -u)

if [[ "$status" -eq 0 ]]; then
    echo "check-launcher-help: ${#routes[@]} commands dispatched, all documented"
fi
exit "$status"
