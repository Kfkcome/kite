#!/usr/bin/env bash
# Regenerates packaging/macos/kite.icns from packaging/macos/icon.svg with tools that ship with
# macOS (qlmanage, swift/AppKit, sips, iconutil): Quick Look renders an SVG on an opaque white
# page, so the tile's rounded-square outline is clipped to transparent afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

qlmanage -t -s 1024 -o "$work" packaging/macos/icon.svg >/dev/null 2>&1
swift - "$work/icon.svg.png" "$work/master.png" <<'SWIFT'
import AppKit

let args = CommandLine.arguments
let src = URL(fileURLWithPath: args[1])
let dst = URL(fileURLWithPath: args[2])

guard let img = NSImage(contentsOf: src),
      let rep = NSBitmapImageRep(
          bitmapDataPlanes: nil,
          pixelsWide: 1024,
          pixelsHigh: 1024,
          bitsPerSample: 8,
          samplesPerPixel: 4,
          hasAlpha: true,
          isPlanar: false,
          colorSpaceName: .deviceRGB,
          bytesPerRow: 0,
          bitsPerPixel: 0
      ) else {
    fputs("failed to load or allocate bitmap\n", stderr)
    exit(1)
}

NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSColor.clear.setFill()
NSRect(x: 0, y: 0, width: 1024, height: 1024).fill()
let clip = NSBezierPath(roundedRect: NSRect(x: 100, y: 100, width: 824, height: 824), xRadius: 185, yRadius: 185)
clip.addClip()
img.draw(in: NSRect(x: 0, y: 0, width: 1024, height: 1024))
NSGraphicsContext.restoreGraphicsState()

guard let png = rep.representation(using: .png, properties: [:]) else {
    fputs("failed to encode png\n", stderr)
    exit(1)
}
try png.write(to: dst)
SWIFT

set_dir="$work/kite.iconset"
mkdir "$set_dir"
for s in 16 32 128 256 512; do
  sips -z $s $s "$work/master.png" --out "$set_dir/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$work/master.png" --out "$set_dir/icon_${s}x${s}@2x.png" >/dev/null
done
cp "$work/master.png" "$set_dir/icon_512x512@2x.png"
iconutil -c icns "$set_dir" -o packaging/macos/kite.icns
echo "wrote packaging/macos/kite.icns"
