#!/usr/bin/env bash
# Tag the latest merged main and push just that tag to start a draft GitHub release.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <version-tag> (for example: v0.1.0)" >&2
    exit 2
fi
tag=$1

fail() {
    echo "release: $*" >&2
    exit 1
}

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
[[ $(git branch --show-current) == main ]] || fail "Run this from the main branch."
[[ -z $(git status --porcelain --untracked-files=all) ]] || fail "A clean checkout is required; commit or stash changes first."
[[ $tag == v* ]] || fail "Use a version tag starting with v, such as v0.1.0."
git check-ref-format "refs/tags/$tag"

git pull --ff-only --no-tags origin main
[[ $(git rev-parse HEAD) == "$(git rev-parse refs/remotes/origin/main)" ]] || fail "main must match origin/main; publish any local commits first."

# Cargo.toml remains the sole version setting; this also checks every crate and Cargo.lock.
python3 tools/release/release.py metadata --tag "$tag"
if git show-ref --verify --quiet "refs/tags/$tag"; then
    fail "Tag $tag already exists locally."
fi
remote_tag=$(git ls-remote --tags origin "refs/tags/$tag" "refs/tags/$tag^{}")
[[ -z $remote_tag ]] || fail "Tag $tag already exists on origin."

git tag -a "$tag" -m "Megawatt Mogul ${tag#v}"
# Keep normal Git hooks enabled. A failed push leaves the annotated tag available for retry.
if ! git push origin "refs/tags/$tag:refs/tags/$tag"; then
    fail "The tag remains local. After resolving the push failure, retry: git push origin refs/tags/$tag"
fi
echo "Pushed $tag. Wait for Linux release, then review and publish its draft on GitHub."
