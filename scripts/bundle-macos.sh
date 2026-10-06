#!/usr/bin/env bash
# Builds folio.app for one macOS target and packages it for release:
#   scripts/bundle-macos.sh [aarch64-apple-darwin|x86_64-apple-darwin]     (default: this Mac)
# Writes to target/dist/:
#   folio.app (in target/dist/<triple>/)   the bundle: folio, folio-cli, folio-mcp
#   folio-macos-<arch>.dmg                 what people download (arch: arm64 or x86_64, the names
#                                          lsuite.xyz/folio/download/macos-* looks for)
#   folio-macos-<arch>.app.tar.gz                the same app for in-app updaters
#
# Environment (all optional):
#   APPLE_SIGNING_IDENTITY       Developer ID identity; without it the build is ad-hoc signed
#   APPLE_API_KEY_PATH, APPLE_API_KEY, APPLE_API_ISSUER
#                                App Store Connect API key: notarize and staple (set by
#                                scripts/prepare-apple-signing.sh in the release workflow)
#   FOLIO_SKIP_BUILD=1           reuse the binaries already in target/<triple>/release
#   CARGO                        the cargo to run (default: cargo)
set -euo pipefail
cd "$(dirname "$0")/.."

triple=${1:-$(rustc -vV | sed -n 's/^host: //p')}
case "$triple" in
  aarch64-apple-darwin) arch=arm64 lipo_arch=arm64 ;;
  x86_64-apple-darwin) arch=x86_64 lipo_arch=x86_64 ;;
  *) echo "usage: $0 aarch64-apple-darwin|x86_64-apple-darwin" >&2; exit 1 ;;
esac
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ -n "$version" ] || { echo "No version in Cargo.toml [workspace.package]" >&2; exit 1; }
identity=${APPLE_SIGNING_IDENTITY:--}
cargo=${CARGO:-cargo}
resources=crates/folio-desktop/resources
dist=target/dist
app="$dist/$triple/folio.app"
bins=(folio folio-cli folio-mcp)

if [ "${FOLIO_SKIP_BUILD:-}" != 1 ]; then
  locked=()
  [ -n "${CI:-}" ] && locked=(--locked)
  packages=()
  for b in "${bins[@]}"; do packages+=(-p "$b"); done
  # GPUI needs 10.15.7; folio requires 11 like kimchi.
  MACOSX_DEPLOYMENT_TARGET=11.0 "$cargo" build --release ${locked[@]+"${locked[@]}"} --target "$triple" "${packages[@]}"
fi

echo "Assembling $app ($version, $arch)"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for b in "${bins[@]}"; do
  src="target/$triple/release/$b"
  test -x "$src" || { echo "Missing $src; build it first." >&2; exit 1; }
  lipo "$src" -verify_arch "$lipo_arch"
  cp "$src" "$app/Contents/MacOS/$b"
done
cp "$resources/folio.icns" "$app/Contents/Resources/folio.icns"
cp LICENSE "$app/Contents/Resources/LICENSE.txt"
sed "s/@VERSION@/$version/g" "$resources/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" > /dev/null
printf 'APPL????' > "$app/Contents/PkgInfo"

# Hardened runtime everywhere (also ad-hoc, so local builds behave like releases); a secure
# timestamp only with a real identity. Helpers first, the bundle (and its main binary) last.
if [ "$identity" = - ]; then
  stamp=(--timestamp=none)
  echo "No APPLE_SIGNING_IDENTITY: ad-hoc signing (fine locally; Gatekeeper will refuse it elsewhere)."
else
  stamp=(--timestamp)
fi
sign() { # path identifier
  # All three load plugins (native libraries signed by others): same entitlements.
  codesign --force --options runtime "${stamp[@]}" --entitlements "$resources/folio.entitlements" \
    --identifier "$2" --sign "$identity" "$1"
}
for b in "${bins[@]}"; do
  [ "$b" = folio ] || sign "$app/Contents/MacOS/$b" "xyz.lsuite.folio.${b#folio-}"
done
sign "$app" xyz.lsuite.folio
codesign --verify --deep --strict --verbose=2 "$app"

notarize() { # path-to-submit
  local result status id
  result=$(xcrun notarytool submit "$1" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" --wait --timeout 45m --output-format json)
  echo "$result"
  status=$(printf '%s' "$result" | plutil -extract status raw -o - - 2> /dev/null || true)
  if [ "$status" != Accepted ]; then
    id=$(printf '%s' "$result" | plutil -extract id raw -o - - 2> /dev/null || true)
    [ -n "$id" ] && xcrun notarytool log "$id" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" \
      --issuer "$APPLE_API_ISSUER" || true
    echo "Notarization of $1 was not accepted: ${status:-no status}" >&2
    exit 1
  fi
}
can_notarize=0
if [ "$identity" != - ] && [ -n "${APPLE_API_KEY_PATH:-}" ] && [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
  can_notarize=1
fi
if [ "$can_notarize" = 1 ]; then
  echo "Notarizing folio.app"
  submission="$dist/folio-$arch-notarize.zip"
  rm -f "$submission"
  ditto -c -k --keepParent "$app" "$submission"
  notarize "$submission"
  rm -f "$submission"
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  spctl --assess --type execute --verbose=2 "$app"
fi

# The download: a disk image with the app and a link to /Applications.
dmg="$dist/folio-macos-$arch.dmg"
stage="$dist/$triple/dmg"
rm -rf "$stage" "$dmg"
mkdir -p "$stage"
ditto "$app" "$stage/folio.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname folio -srcfolder "$stage" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$dmg" > /dev/null
rm -rf "$stage"
if [ "$identity" != - ]; then
  codesign --force --timestamp --sign "$identity" "$dmg"
fi
if [ "$can_notarize" = 1 ]; then
  echo "Notarizing $dmg"
  notarize "$dmg"
  xcrun stapler staple "$dmg"
fi

# The updater archive: one folio.app folder at the root, without AppleDouble files.
archive="$dist/folio-macos-$arch.app.tar.gz"
rm -f "$archive"
COPYFILE_DISABLE=1 tar --no-mac-metadata -C "$dist/$triple" -czf "$archive" folio.app

echo "Built folio $version for $triple:"
ls -lh "$dmg" "$archive"
