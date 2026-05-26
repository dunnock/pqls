# Release Workflow Audit

## Trigger

`.github/workflows/release.yml` fires on `push` to any tag matching `v*`.

## Jobs (before this change)

| Job | Runner | Target | Method | Output |
|-----|--------|--------|--------|--------|
| `build-x86_64` | ubuntu-latest | x86_64-unknown-linux-gnu | `cargo build --release` (native) | bare binary `pqls-linux-x86_64` |
| `build-aarch64` | ubuntu-latest | aarch64-unknown-linux-gnu | `cross build --release` (cross) | bare binary `pqls-linux-aarch64` |
| `release` | ubuntu-latest | — | `softprops/action-gh-release@v2` | creates GitHub release |

## Artifacts on GitHub Release Page (before this change)

- `pqls-linux-x86_64` — bare ELF binary, Linux x86_64
- `pqls-linux-aarch64` — bare ELF binary, Linux aarch64
- `install.sh` — curl-pipe installer (Linux only)

## What Was Missing

| Gap | Impact |
|-----|--------|
| No macOS builds (`x86_64-apple-darwin`, `aarch64-apple-darwin`) | macOS users cannot use the install script or download a pre-built binary |
| No Windows build (`x86_64-pc-windows-msvc`) | Windows users must build from source |
| Binaries not packed (no `.tar.gz` / `.zip`) | Harder to distribute; no checksum-friendly single asset |
| No `cargo publish` step | crates.io publish is a manual operator step; forgettable |
| `install.sh` explicitly rejected macOS | Darwin users always saw an error even when macOS binaries exist |

## Changes Made

### Workflow (`release.yml`)

Added three new build jobs:
- `build-macos-x86_64` — `macos-13` runner (Intel), native `cargo build`, packs as `.tar.gz`
- `build-macos-aarch64` — `macos-latest` runner (Apple Silicon), native `cargo build`, packs as `.tar.gz`
- `build-windows-x86_64` — `windows-latest` runner, native `cargo build`, packs as `.zip`

Existing Linux jobs updated to pack binaries as `.tar.gz` instead of bare binaries.

Added `publish` job:
- Runs after all five build jobs succeed
- Reads `CRATES_IO_TOKEN` from repository secrets
- Calls `cargo publish`

### `install.sh`

- Added macOS (`darwin`) support — removes the explicit rejection
- Updated to download `.tar.gz` archives and extract them

### README

Added "Releases" section documenting pre-built platforms, `cargo install`, and the `CRATES_IO_TOKEN` secret requirement.

## Operator Action Required

Set the `CRATES_IO_TOKEN` repository secret on GitHub before the next release tag push, or the `publish` job will fail. See README "Releases" section.
