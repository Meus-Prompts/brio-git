#!/usr/bin/env bash
# Prints the next release version (X.Y.Z) from the Conventional Commits made
# since the last vX.Y.Z tag (tags of other channels, like egui-vX.Y.Z, are
# ignored), or nothing when there is no commit since it.
#
#   feat:                        -> minor (1.4.2 -> 1.5.0)
#   type!: / BREAKING CHANGE:    -> major (1.4.2 -> 2.0.0)
#   anything else (fix, chore…)  -> patch (1.4.2 -> 1.4.3)
#   no tag yet                   -> 0.1.0
#
#   scripts/next-version.sh          next version for HEAD of this repository
#   scripts/next-version.sh --test   runs the self-test
set -euo pipefail

FIRST="0.1.0"
# Prefixo das tags de release; outros canais (egui-v…) não contam.
PREFIX="${TAG_PREFIX:-v}"

# bump <last version> <level: major|minor|patch>
bump() {
    local major minor patch
    IFS=. read -r major minor patch <<<"$1"
    case "$2" in
        major) echo "$((major + 1)).0.0" ;;
        minor) echo "$major.$((minor + 1)).0" ;;
        *) echo "$major.$minor.$((patch + 1))" ;;
    esac
}

# level: reads commit messages separated by NUL on stdin, prints the bump level.
level() {
    local level="patch" message subject
    while IFS= read -r -d '' message; do
        subject="${message%%$'\n'*}"
        if [[ "$subject" =~ ^[a-zA-Z]+(\([^\)]*\))?!: ]] || [[ "$message" == *"BREAKING CHANGE:"* || "$message" == *"BREAKING-CHANGE:"* ]]; then
            echo major
            return
        fi
        if [[ "$subject" =~ ^feat(\([^\)]*\))?: ]]; then
            level="minor"
        fi
    done
    echo "$level"
}

next() {
    local tag
    tag="$(git describe --tags --abbrev=0 --match "${PREFIX}[0-9]*.[0-9]*.[0-9]*" 2>/dev/null || true)"
    if [[ -z "$tag" ]]; then
        echo "$FIRST"
        return
    fi
    if [[ -z "$(git rev-list "$tag..HEAD")" ]]; then
        return # nothing new since the last release
    fi
    bump "${tag#"$PREFIX"}" "$(git log --format='%B%x00' "$tag..HEAD" | level)"
}

self_test() {
    local failed=0
    check() { # check <expected> <actual> <what>
        if [[ "$1" != "$2" ]]; then
            echo "FALHOU: $3: esperado '$1', veio '$2'" >&2
            failed=1
        fi
    }
    msgs() { printf '%s\0' "$@"; }

    check "1.4.3" "$(bump 1.4.2 patch)" "patch"
    check "1.5.0" "$(bump 1.4.2 minor)" "minor"
    check "2.0.0" "$(bump 1.4.2 major)" "major"
    check "patch" "$(msgs 'fix: corrige grafo' 'chore: deps' | level)" "fix e chore"
    check "minor" "$(msgs 'fix: x' 'feat(app): reflog' 'docs: y' | level)" "feat com escopo"
    check "major" "$(msgs 'feat!: novo formato' | level)" "bang"
    check "major" "$(msgs 'refactor(core)!: muda API' | level)" "bang com escopo"
    check "major" "$(msgs $'feat: x\n\nBREAKING CHANGE: remove y' | level)" "rodapé breaking"
    check "patch" "$(msgs 'feature: não é feat' 'Merge branch x' | level)" "não conventional"
    check "patch" "$(printf '' | level)" "sem commits"

    # End to end in a scratch repository.
    local dir
    dir="$(mktemp -d)"
    (
        cd "$dir"
        git init -q -b main
        git config user.name t && git config user.email t@t && git config commit.gpgsign false && git config tag.gpgsign false
        git commit -q --allow-empty -m "chore: início"
        check "$FIRST" "$(next)" "sem tag"
        git tag egui-v9.9.9 && git tag "${PREFIX}1.2.3"
        check "" "$(next)" "nada desde a tag"
        git commit -q --allow-empty -m "fix: algo"
        check "1.2.4" "$(next)" "fix após tag"
        git commit -q --allow-empty -m "feat: algo"
        check "1.3.0" "$(next)" "feat após tag"
        exit "$failed"
    ) || failed=1
    rm -rf -- "$dir"

    if [[ "$failed" == 0 ]]; then echo "next-version: ok"; fi
    return "$failed"
}

if [[ "${1:-}" == "--test" ]]; then
    self_test
else
    next
fi
