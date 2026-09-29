#!/bin/sh
# Rebuilds `obelus.ico` and `obelus.icns` from `obelus.svg`.
#
# The SVG is the icon. These two exist because neither of the things that
# wants them takes an SVG: Windows reads an icon out of the executable's own
# resources, and a macOS bundle reads one out of `Contents/Resources`. So
# they are built rather than drawn, and this is the command -- run it after
# editing the SVG, and commit what it writes.
#
# Needs `rsvg-convert` and `python3`. Not run by any build: the two files are
# committed, because a build that rasterised an SVG would put a renderer in
# the way of every compile on every machine, and the icon changes about
# never.
set -eu

cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

for size in 16 24 32 48 64 128 256 512 1024; do
    rsvg-convert -w "$size" -h "$size" obelus.svg -o "$work/$size.png"
done

python3 - "$work" <<'PY'
import pathlib
import struct
import sys

work = pathlib.Path(sys.argv[1])


def png(size):
    return (work / f'{size}.png').read_bytes()


# Windows. Every entry is a PNG, which every Windows since Vista reads and
# which is twenty-seven times smaller than the uncompressed bitmaps the
# format was first written for. A 256 is declared as a zero, because the
# field is one byte and 256 does not fit in it.
sizes = [16, 24, 32, 48, 64, 128, 256]
blobs = [(size, png(size)) for size in sizes]
header = struct.pack('<HHH', 0, 1, len(blobs))
offset = len(header) + 16 * len(blobs)
entries, data = b'', b''
for size, blob in blobs:
    side = 0 if size == 256 else size
    entries += struct.pack('<BBBBHHII', side, side, 0, 0, 1, 32, len(blob), offset)
    data += blob
    offset += len(blob)
pathlib.Path('obelus.ico').write_bytes(header + entries + data)

# macOS. The types `iconutil` emits for a modern iconset, each holding a
# PNG. A name says the slot and not the pixels: `ic11` is "16 points at 2x",
# which is 32 of them.
slots = [
    ('icp4', 16), ('icp5', 32), ('ic11', 32), ('ic12', 64),
    ('ic07', 128), ('ic13', 256), ('ic08', 256),
    ('ic14', 512), ('ic09', 512), ('ic10', 1024),
]
chunks = b''
for kind, size in slots:
    blob = png(size)
    chunks += kind.encode('ascii') + struct.pack('>I', len(blob) + 8) + blob
pathlib.Path('obelus.icns').write_bytes(b'icns' + struct.pack('>I', len(chunks) + 8) + chunks)

print('obelus.ico and obelus.icns rebuilt from obelus.svg')
PY

# And the site's favicon, which is the mark without the plate: a tab has
# its own background, and a dark plate in a light tab bar is a hole in it.
# `obelus-mark.svg` rather than this one, because a favicon is read at
# sixteen pixels and the bar in the icon above is half a pixel there --
# the plate is what carries it at that size, so a mark without one is
# drawn on its own grid. Copied and not converted, and copied here so
# that nobody has to remember to.
cp obelus-mark.svg ../../docs/obelus.svg
echo 'docs/obelus.svg copied from obelus-mark.svg'
