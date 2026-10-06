#!/usr/bin/env bash
# Renders folio's app icon from brand/icon.svg (the lsuite icon template) into every format the
# packages need, and writes them to the repository (run it after changing the SVG, then commit):
#   brand/icon.png                                 1024 px
#   crates/folio-desktop/resources/folio.icns      macOS (iconutil on a Mac, Pillow elsewhere)
#   crates/folio-desktop/resources/folio.ico       Windows (16–256 px, PNG-compressed)
#   crates/folio-desktop/resources/folio.png       Linux (512 px; .desktop, AppImage, .deb)
# Needs resvg (`cargo install resvg`) or rsvg-convert (`brew install librsvg`), and python3
# (with Pillow when iconutil isn't there; set PYTHON to pick another interpreter).
set -euo pipefail
cd "$(dirname "$0")/.."
svg=brand/icon.svg
out=crates/folio-desktop/resources
python=${PYTHON:-python3}
mkdir -p "$out"

render() { # size output
  if command -v resvg > /dev/null; then
    resvg -w "$1" -h "$1" "$svg" "$2"
  elif command -v rsvg-convert > /dev/null; then
    rsvg-convert -w "$1" -h "$1" "$svg" -o "$2"
  else
    echo "Install resvg (cargo install resvg) or rsvg-convert to render the icon." >&2
    exit 1
  fi
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

render 1024 brand/icon.png
render 512 "$out/folio.png"

# Each size is rendered from the SVG, not scaled down, so small sizes stay sharp.
set_dir="$work/folio.iconset"
mkdir -p "$set_dir"
for size in 16 32 128 256 512; do
  render "$size" "$set_dir/icon_${size}x${size}.png"
  render $((size * 2)) "$set_dir/icon_${size}x${size}@2x.png"
done
if command -v iconutil > /dev/null; then
  iconutil -c icns "$set_dir" -o "$out/folio.icns"
elif "$python" -c "import PIL" 2> /dev/null; then
  "$python" - "$out/folio.icns" "$set_dir" <<'PY'
# Pillow writes an .icns from PNGs of each size (16 to 1024 px).
import os, sys
from PIL import Image
out, d = sys.argv[1], sys.argv[2]
png = lambda n: Image.open(os.path.join(d, n)).convert("RGBA")
sizes = {16: "icon_16x16.png", 32: "icon_32x32.png", 64: "icon_32x32@2x.png",
         128: "icon_128x128.png", 256: "icon_256x256.png", 512: "icon_512x512.png",
         1024: "icon_512x512@2x.png"}
images = [png(n) for n in sizes.values()]
images[-1].save(out, format="ICNS", append_images=images[:-1])
PY
else
  echo "Neither iconutil nor Pillow found: kept the existing $out/folio.icns" >&2
fi

sizes=(16 24 32 48 64 128 256)
for size in "${sizes[@]}"; do render "$size" "$work/ico-$size.png"; done
"$python" - "$out/folio.ico" "${sizes[@]/#/$work/ico-}" <<'PY'
# An .ico whose entries are PNGs (Windows Vista and later), largest last.
import struct, sys
out, files = sys.argv[1], [f + ".png" for f in sys.argv[2:]]
images = [open(f, "rb").read() for f in files]
header = struct.pack("<HHH", 0, 1, len(images))
offset = 6 + 16 * len(images)
entries = b""
for f, data in zip(files, images):
    w, h = struct.unpack(">II", data[16:24])
    entries += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
open(out, "wb").write(header + entries + b"".join(images))
PY
ls -l brand/icon.png "$out"/folio.*
