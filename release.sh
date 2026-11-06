#!/usr/bin/env bash
# Publishes a new ColorShot release on GitHub.
#
#   ./release.sh            bump the patch version (0.1.0 -> 0.1.1)
#   ./release.sh 0.2.0      release that version
#
# Bumps the version in Cargo.toml, runs the tests and builds the portable archive with
# `just release` (needs Docker). Then, after asking, commits, pushes and creates the GitHub
# release with `gh`. If anything fails before pushing, Cargo.toml and Cargo.lock are restored.
set -euo pipefail

REPO="wfpaisa/colorshot"
ASSET="dist/colorshot-x86_64-linux.tar.gz"

info() { printf '\033[1;34m::\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

cd "$(dirname "$0")"

for tool in git gh just docker cargo; do
    command -v "$tool" >/dev/null || die "Missing $tool"
done
gh auth status >/dev/null 2>&1 || die "Not logged in to GitHub: run gh auth login"

[[ "$(git branch --show-current)" == main ]] || die "Not on main"
[[ -z "$(git status --porcelain)" ]] || die "Uncommitted changes: commit or stash them first"
git fetch --quiet origin main
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] ||
    die "main and origin/main differ: pull or push first"

current="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
[[ -n "$current" ]] || die "No version in Cargo.toml"
if [[ $# -ge 1 ]]; then
    version="${1#v}"
else
    IFS=. read -r major minor patch <<<"$current"
    version="${major}.${minor}.$((patch + 1))"
fi
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "Invalid version: $version"
tag="v${version}"
[[ "$version" != "$current" ]] || die "Cargo.toml is already at $version"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && die "Tag $tag already exists"
gh release view "$tag" --repo "$REPO" >/dev/null 2>&1 && die "Release $tag already exists"

info "Releasing $tag (now $current)"
# Until the commit, a failure leaves the tree as it was.
trap 'git checkout --quiet -- Cargo.toml Cargo.lock' EXIT
sed -i "s/^version = \"$current\"/version = \"$version\"/" Cargo.toml
cargo update --workspace --offline --quiet

info "Running tests"
cargo test --workspace --offline --quiet

info "Building the portable archive (Docker)"
just release
[[ -f "$ASSET" && -f "$ASSET.sha256" ]] || die "just release did not produce $ASSET"

read -rp "Commit, push and publish $tag? [y/N] " answer
[[ "$answer" == [yY]* ]] || die "Cancelled; nothing was published"

info "Committing and pushing"
git commit --quiet -am "Release $tag"
trap - EXIT
git push --quiet origin main

info "Creating the GitHub release"
gh release create "$tag" "$ASSET" "$ASSET.sha256" \
    --repo "$REPO" --target main --title "ColorShot $tag" --generate-notes

info "Done: https://github.com/$REPO/releases/tag/$tag"
