#!/usr/bin/env bash
# Publishes a folio build to ludovic111/lsuite-builds as the release folio-v<version>, where the
# lsuite app and folio's updater get it through lsuite.xyz (lsuite's DISTRIBUTION.md).
#
#   scripts/publish-build.sh <version> <run-id>   from the artifacts of a run of kimchi's
#                                                 suite-build.yml with app=folio (the signed builds)
#   scripts/publish-build.sh <version>            from folio's own draft release v<version>
#                                                 (made by .github/workflows/release.yml), which is
#                                                 deleted once the copy is published
#
# The files are copied as built and signed (the platform files and their .sig); the signatures
# are checked against folio's update key, latest.json is written by folio-release (its URLs point
# at the lsuite-builds release; the server rewrites them to its own file route) and SHA256SUMS is
# made from the files. Nothing is re-signed.
#
# Needs: gh, signed in with access to the source repository and to lsuite-builds; cargo.
# Environment: LSUITE_BUILDS_REPO (default ludovic111/lsuite-builds), SUITE_BUILD_REPO (default
# ludovic111/kimchi), DRY_RUN=1 to stop before creating the release (the files stay in ./dist-publish).
set -euo pipefail

usage() {
  sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
}

[ $# -ge 1 ] && [ $# -le 2 ] || usage
version=${1#v}
run=${2:-}
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || { echo "error: $1 isn't a version like 0.2.0" >&2; exit 2; }
[ -z "$run" ] || [[ "$run" =~ ^[0-9]+$ ]] || { echo "error: $run isn't a workflow run id" >&2; exit 2; }

root=$(cd "$(dirname "$0")/.." && pwd)
builds=${LSUITE_BUILDS_REPO:-ludovic111/lsuite-builds}
suite=${SUITE_BUILD_REPO:-ludovic111/kimchi}
tag="folio-v$version"
command -v gh > /dev/null || { echo "error: gh (GitHub CLI) is needed" >&2; exit 1; }

if gh release view "$tag" -R "$builds" > /dev/null 2>&1; then
  echo "error: $builds already has $tag; a published build is never replaced." >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
dist="$work/dist"
mkdir -p "$dist" "$work/in"

if [ -n "$run" ]; then
  info=$(gh run view "$run" -R "$suite" --json workflowName,conclusion,status,displayTitle)
  echo "run $run: $info"
  [ "$(jq -r .status <<< "$info")" = completed ] || { echo "error: run $run hasn't finished" >&2; exit 1; }
  [ "$(jq -r .conclusion <<< "$info")" = success ] || { echo "error: run $run didn't succeed" >&2; exit 1; }
  gh run download "$run" -R "$suite" -p 'folio-*' -D "$work/in"
else
  draft=$(gh release view "v$version" -R ludovic111/folio --json isDraft -q .isDraft)
  [ "$draft" = true ] || { echo "error: v$version of ludovic111/folio isn't a draft release" >&2; exit 1; }
  gh release download "v$version" -R ludovic111/folio -D "$work/in"
fi

# The platform files and their signatures, flattened (artifacts come one folder per target).
find "$work/in" -type f -name 'folio-*' -exec cp {} "$dist/" \;
ls "$dist"/folio-* > /dev/null 2>&1 || { echo "error: no folio-* files found" >&2; exit 1; }

# What changed: CHANGELOG.md's section for this version.
awk -v v="$version" '/^## /{p = ($2 == v); next} p' "$root/CHANGELOG.md" | sed -e '/./,$!d' > "$work/notes.md"
[ -s "$work/notes.md" ] || echo "folio $version: documents, sheets and slides in one file." > "$work/notes.md"

# Checks every signature against folio's update key and the version, and writes latest.json.
cargo run --quiet --release --locked --manifest-path "$root/Cargo.toml" -p folio-release -- \
  manifest "$dist" --version "$version" --base-url "https://github.com/$builds/releases/download/$tag" \
  --notes-file "$work/notes.md" --out "$dist/latest.json"

if command -v sha256sum > /dev/null; then
  (cd "$dist" && sha256sum folio-* > SHA256SUMS)
else
  (cd "$dist" && shasum -a 256 folio-* > SHA256SUMS)
fi
ls -l "$dist"

if [ "${DRY_RUN:-}" = 1 ]; then
  rm -rf "$root/dist-publish"
  cp -R "$dist" "$root/dist-publish"
  echo "DRY_RUN: not publishing; the files are in $root/dist-publish"
  exit 0
fi

gh release create "$tag" -R "$builds" "$dist"/* --title "folio $version" --notes-file "$work/notes.md"
echo "published $tag in $builds"

if [ -z "$run" ]; then
  gh release delete "v$version" -R ludovic111/folio --yes
  echo "deleted the draft v$version of ludovic111/folio"
fi
