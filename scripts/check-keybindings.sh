#!/usr/bin/env bash
# Cross-references KeyCode::* patterns in src/events/ against the keybindings
# table in README.md. Exits non-zero if a keybinding appears in code but not
# in the README. Intended as a non-blocking CI check — it flags doc drift but
# should not gate merges.
#
# Usage: scripts/check-keybindings.sh
#
# Heuristic, not exhaustive. False positives are expected.

set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVENTS_DIR="${ROOT}/src/events"
README="${ROOT}/README.md"

if [[ ! -d "${EVENTS_DIR}" ]]; then
    echo "error: events dir not found: ${EVENTS_DIR}" >&2
    exit 2
fi
if [[ ! -f "${README}" ]]; then
    echo "error: README not found: ${README}" >&2
    exit 2
fi

mapfile -t code_keys < <(
    grep -rhoE "KeyCode::(Char\('[^']+'\)|[A-Z][A-Za-z]+)" "${EVENTS_DIR}" \
    | sort -u
)

missing=0
for key in "${code_keys[@]}"; do
    if [[ "${key}" =~ KeyCode::Char\(\'([^\']+)\'\) ]]; then
        display="${BASH_REMATCH[1]}"
    else
        display="${key#KeyCode::}"
    fi

    if [[ ${#display} -lt 1 ]]; then
        continue
    fi

    if ! grep -qiF "${display}" "${README}"; then
        echo "missing in README: ${key} (look for \`${display}\`)"
        missing=$((missing + 1))
    fi
done

if (( missing > 0 )); then
    echo ""
    echo "${missing} keybinding(s) in src/events/ did not appear in README.md."
    echo "Update the keybindings table or adjust this script if intentional."
    exit 1
fi

echo "OK: every KeyCode literal in src/events/ appears somewhere in README.md."
