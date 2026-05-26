BUMP ?= minor

.PHONY: release release-dry-run bump-version publish release-resume

# Full end-to-end release: preflight → bump → check → commit → tag → push → publish.
# Operator runs this from their host machine.
release:
	BUMP=$(BUMP) MODE=full bash scripts/release.sh

# Preview: runs preflight + bump + cargo check, prints what the real release would do.
# Safe to run anywhere (including CI / container). Leaves no permanent changes.
release-dry-run:
	BUMP=$(BUMP) MODE=dry-run bash scripts/release.sh

# Bump Cargo.toml only; prints the new version on stdout.
# Rejects if the resulting tag already exists locally or on origin.
bump-version:
	@BUMP=$(BUMP) MODE=bump-only bash scripts/release.sh

# Re-run cargo publish after a failed step 8 (tag already pushed).
publish:
	MODE=publish-only bash scripts/release.sh

# Push + publish when the local commit + tag exist but push previously failed.
release-resume:
	MODE=resume bash scripts/release.sh
