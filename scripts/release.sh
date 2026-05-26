#!/usr/bin/env bash
# scripts/release.sh — pqls release automation
#
# Modes (set via MODE env var):
#   full        — full release: preflight, bump, check, commit, tag, push, publish
#   dry-run     — preview only: preflight, bump, check, then print what would happen
#   bump-only   — bump Cargo.toml, print new version to stdout, exit
#   publish-only — cargo publish only (rerun after a failed step 8)
#   resume      — push + publish (assumes local commit + tag from a failed push)
#
# Variables:
#   BUMP  — patch | minor | major  (default: minor)
#   MODE  — see above              (default: full)

set -euo pipefail

BUMP="${BUMP:-minor}"
MODE="${MODE:-full}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# All informational output goes to stderr so stdout stays clean for data.
die()  { printf '\nERROR: %s\n' "$*" >&2; exit 1; }
step() { printf '\n==> %s\n' "$*" >&2; }
info() { printf '    %s\n' "$*" >&2; }

# ── helpers ───────────────────────────────────────────────────────────────────

current_version() {
    grep '^version = ' Cargo.toml | head -1 | sed 's/^version = "\(.*\)"$/\1/'
}

compute_new_version() {
    local cur="$1" bump="$2"
    local major minor patch
    IFS='.' read -r major minor patch <<< "$cur"
    case "$bump" in
        patch) printf '%s.%s.%s' "$major" "$minor" "$((patch + 1))" ;;
        minor) printf '%s.%s.0'  "$major" "$((minor + 1))" ;;
        major) printf '%s.0.0'   "$((major + 1))" ;;
        *) die "BUMP must be patch, minor, or major (got: $bump)" ;;
    esac
}

check_tag_absent() {
    local tag="$1"
    if git tag -l "$tag" | grep -q "^${tag}$"; then
        die "Tag $tag already exists locally.\n  To delete: git tag -d $tag"
    fi
    if git ls-remote --tags origin "refs/tags/$tag" 2>/dev/null | grep -q "refs/tags/$tag"; then
        die "Tag $tag already exists on origin.\n  If this is a retry after a failed push, use: make release-resume"
    fi
}

# ── steps ─────────────────────────────────────────────────────────────────────

do_preflight() {
    local skip_creds="${1:-0}" skip_sync="${2:-0}" skip_branch="${3:-0}" skip_dirty="${4:-0}"
    step "Pre-flight checks"

    command -v git   >/dev/null 2>&1 || die "'git' not found in PATH"
    command -v cargo >/dev/null 2>&1 || die "'cargo' not found in PATH"
    info "git and cargo available ✓"

    local branch
    branch=$(git symbolic-ref --short HEAD 2>/dev/null) \
        || die "Not on a branch (detached HEAD)"
    if [[ "$skip_branch" != "1" ]]; then
        [[ "$branch" == "main" ]] \
            || die "Must be on 'main' branch (currently on '$branch')"
        info "Branch: $branch ✓"
    else
        info "Branch check skipped (dry-run) — currently on: $branch"
    fi

    if [[ "$skip_dirty" != "1" ]]; then
        # Allow only Cargo.toml and Cargo.lock to be dirty.
        local dirty
        dirty=$(git status --porcelain | grep -vE '^.. (Cargo\.toml|Cargo\.lock)$' || true)
        [[ -z "$dirty" ]] \
            || die "Working tree has uncommitted changes outside Cargo.toml/Cargo.lock:\n$dirty"
        info "Working tree clean ✓"
    else
        info "Dirty-tree check skipped (dry-run)"
    fi

    if [[ "$skip_sync" != "1" ]]; then
        git fetch origin --quiet
        local local_sha origin_sha
        local_sha=$(git rev-parse HEAD)
        origin_sha=$(git rev-parse origin/main)
        [[ "$local_sha" == "$origin_sha" ]] \
            || die "Local main is not in sync with origin/main\n  local:  $local_sha\n  origin: $origin_sha\n  Run: git pull --rebase origin main"
        info "In sync with origin/main ✓"
    else
        info "Origin sync check skipped (dry-run)"
    fi

    if [[ "$skip_creds" != "1" ]]; then
        if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]] && [[ ! -f "${HOME}/.cargo/credentials.toml" ]]; then
            die "No crates.io credentials found.\n  Run 'cargo login' or set CARGO_REGISTRY_TOKEN"
        fi
        info "crates.io credentials found ✓"
    else
        info "Credentials check skipped (dry-run)"
    fi
}

do_bump() {
    local cur="$1" new_version="$2"
    step "Bumping version: $cur → $new_version"
    sed -i "s/^version = \"$cur\"/version = \"$new_version\"/" Cargo.toml
    info "Cargo.toml updated"
}

do_cargo_check() {
    step "Running cargo check --all-targets"
    cargo check --all-targets 2>&1 | sed 's/^/    /' >&2
    info "cargo check passed ✓"
}

do_commit_and_tag() {
    local new_version="$1" tag="v$new_version"
    step "Committing and tagging $tag"
    git add Cargo.toml Cargo.lock
    git commit -m "release: $tag"
    git tag -a "$tag" -m "Release $tag"
    info "Committed: release: $tag"
    info "Tagged:    $tag (annotated)"
}

do_push() {
    local tag="$1"
    step "Pushing main and $tag to origin"
    git push origin main
    git push origin "$tag"
    info "Pushed. GitHub Actions will build release binaries (~3 min)."
}

do_publish() {
    step "Publishing to crates.io"
    cargo publish
    info "Published ✓"
}

print_summary() {
    local new_version="$1"
    printf '\n' >&2
    printf '================================================\n' >&2
    printf ' Release v%s complete\n' "$new_version" >&2
    printf '================================================\n' >&2
    printf '\n' >&2
    printf '  GitHub release : https://github.com/dunnock/pqls/releases/tag/v%s\n' "$new_version" >&2
    printf '  crates.io      : https://crates.io/crates/pqls/%s\n' "$new_version" >&2
    printf '\n' >&2
    printf '  Workflow dispatched — binaries appear in ~3 min.\n' >&2
}

# ── modes ─────────────────────────────────────────────────────────────────────

case "$MODE" in

full)
    do_preflight 0 0

    cur=$(current_version)
    new_version=$(compute_new_version "$cur" "$BUMP")
    tag="v$new_version"
    step "Version: $cur → $new_version  (tag: $tag)"
    check_tag_absent "$tag"

    do_bump "$cur" "$new_version"
    do_cargo_check
    do_commit_and_tag "$new_version"

    printf '\nLocal commit and tag created. Proceeding to push + publish...\n' >&2
    printf '(If push fails below, run: make release-resume)\n' >&2

    do_push "$tag" || {
        printf '\nERROR: Push failed. Local commit and tag are intact.\n' >&2
        printf 'Recovery: make release-resume\n' >&2
        exit 1
    }

    do_publish || {
        printf '\nERROR: cargo publish failed. Tag %s is already on GitHub.\n' "$tag" >&2
        printf 'Recovery: make publish\n' >&2
        exit 1
    }

    print_summary "$new_version"
    ;;

dry-run)
    printf '=== DRY RUN: no push, no publish, no permanent changes ===\n' >&2
    do_preflight 1 1 1 1

    cur=$(current_version)
    new_version=$(compute_new_version "$cur" "$BUMP")
    tag="v$new_version"
    step "Version plan: $cur → $new_version  (tag: $tag)"

    # Only check local tags in dry-run (no network fetch required)
    if git tag -l "$tag" | grep -q "^${tag}$"; then
        die "Tag $tag already exists locally. Cannot proceed."
    fi
    info "Tag $tag does not exist locally ✓"

    # Save originals for cleanup
    CARGO_TOML_BAK=$(mktemp)
    CARGO_LOCK_BAK=$(mktemp)
    cp Cargo.toml "$CARGO_TOML_BAK"
    cp Cargo.lock "$CARGO_LOCK_BAK"

    cleanup() {
        cp "$CARGO_TOML_BAK" Cargo.toml
        cp "$CARGO_LOCK_BAK" Cargo.lock
        rm -f "$CARGO_TOML_BAK" "$CARGO_LOCK_BAK"
        printf '\n(dry-run cleanup: Cargo.toml and Cargo.lock restored)\n' >&2
    }
    trap cleanup EXIT

    do_bump "$cur" "$new_version"
    do_cargo_check

    # Print preview to stderr (informational, not data)
    printf '\n=== Would execute (skipped in dry-run): ===\n' >&2
    printf '    git add Cargo.toml Cargo.lock\n' >&2
    printf '    git commit -m "release: %s"\n' "$tag" >&2
    printf '    git tag -a %s -m "Release %s"\n' "$tag" "$tag" >&2
    printf '    git push origin main\n' >&2
    printf '    git push origin %s    # triggers .github/workflows/release.yml\n' "$tag" >&2
    printf '    cargo publish\n' >&2
    printf '\n=== Release URLs (after push): ===\n' >&2
    printf '    https://github.com/dunnock/pqls/releases/tag/%s\n' "$tag" >&2
    printf '    https://crates.io/crates/pqls/%s\n' "$new_version" >&2
    printf '\n=== DRY RUN COMPLETE — no changes pushed or published ===\n' >&2
    ;;

bump-only)
    cur=$(current_version)
    new_version=$(compute_new_version "$cur" "$BUMP")
    tag="v$new_version"

    # Reject if tag already exists
    if git tag -l "$tag" | grep -q "^${tag}$"; then
        die "Tag $tag already exists locally. Cannot bump to this version."
    fi
    if git ls-remote --tags origin "refs/tags/$tag" 2>/dev/null | grep -q "refs/tags/$tag"; then
        die "Tag $tag already exists on origin. Cannot bump to this version."
    fi

    sed -i "s/^version = \"$cur\"/version = \"$new_version\"/" Cargo.toml
    # Print new version to stdout (only stdout output in this mode)
    echo "$new_version"
    ;;

publish-only)
    step "Publishing to crates.io (publish-only mode)"
    cargo publish
    info "Published ✓"
    ;;

resume)
    # Assumes local commit + tag exist (steps 1-6 already done)
    tag=$(git describe --tags --abbrev=0 2>/dev/null) \
        || die "No tags found. Has the release been committed and tagged?"
    [[ "$tag" =~ ^v[0-9] ]] \
        || die "Latest tag '$tag' doesn't look like a version tag"
    new_version="${tag#v}"
    step "Resuming release $tag (push + publish)"
    do_push "$tag" || {
        printf '\nERROR: Push failed.\n' >&2
        printf 'Recovery: make release-resume\n' >&2
        exit 1
    }
    do_publish || {
        printf '\nERROR: cargo publish failed. Tag %s is already on GitHub.\n' "$tag" >&2
        printf 'Recovery: make publish\n' >&2
        exit 1
    }
    print_summary "$new_version"
    ;;

*)
    die "Unknown MODE: '$MODE'. Valid values: full, dry-run, bump-only, publish-only, resume"
    ;;
esac
