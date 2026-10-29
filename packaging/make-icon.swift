// Original Framecraft artwork. Rebuild with: swift packaging/make-icon.swift
// AppKit draws each resolution independently, keeping the frame legible at 16px.
import AppKit

let packaging = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let iconset = packaging.appendingPathComponent("Framecraft.iconset")
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

func render(_ pixels: Int) -> Data {
    let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
    let scale = CGFloat(pixels) / 1024
    let transform = NSAffineTransform()
    transform.scale(by: scale)
    transform.concat()

    // A restrained blue tile with a white capture boundary and image inside.
    let tile = NSBezierPath(roundedRect: NSRect(x: 72, y: 72, width: 880, height: 880),
        xRadius: 192, yRadius: 192)
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.18)
    shadow.shadowBlurRadius = 24
    shadow.shadowOffset = NSSize(width: 0, height: -10)
    shadow.set()
    NSColor(calibratedRed: 0.075, green: 0.30, blue: 0.91, alpha: 1).setFill()
    tile.fill()
    NSShadow().set()
    let gradient = NSGradient(starting: NSColor(calibratedRed: 0.16, green: 0.49, blue: 0.99, alpha: 1),
        ending: NSColor(calibratedRed: 0.04, green: 0.25, blue: 0.78, alpha: 1))!
    gradient.draw(in: tile, angle: -90)

    let picture = NSBezierPath(roundedRect: NSRect(x: 300, y: 325, width: 424, height: 374),
        xRadius: 44, yRadius: 44)
    NSColor.white.withAlphaComponent(0.18).setFill()
    picture.fill()
    NSGraphicsContext.saveGraphicsState()
    picture.addClip()
    let hill = NSBezierPath()
    hill.move(to: NSPoint(x: 285, y: 340))
    hill.line(to: NSPoint(x: 445, y: 540))
    hill.line(to: NSPoint(x: 530, y: 445))
    hill.line(to: NSPoint(x: 612, y: 536))
    hill.line(to: NSPoint(x: 740, y: 375))
    hill.line(to: NSPoint(x: 740, y: 300))
    hill.line(to: NSPoint(x: 285, y: 300))
    hill.close()
    NSColor.white.withAlphaComponent(0.92).setFill()
    hill.fill()
    NSColor.white.setFill()
    NSBezierPath(ovalIn: NSRect(x: 555, y: 578, width: 62, height: 62)).fill()
    NSGraphicsContext.restoreGraphicsState()

    let corners = NSBezierPath()
    corners.lineWidth = 40
    corners.lineCapStyle = .round
    corners.lineJoinStyle = .round
    for (x, y, dx, dy): (CGFloat, CGFloat, CGFloat, CGFloat) in [
        (242, 268, 118, 118), (782, 268, -118, 118),
        (242, 756, 118, -118), (782, 756, -118, -118)
    ] {
        corners.move(to: NSPoint(x: x + dx, y: y))
        corners.line(to: NSPoint(x: x, y: y))
        corners.line(to: NSPoint(x: x, y: y + dy))
    }
    NSColor.white.setStroke()
    corners.stroke()
    NSGraphicsContext.restoreGraphicsState()
    return bitmap.representation(using: .png, properties: [:])!
}

for size in [16, 32, 128, 256, 512] {
    try render(size).write(to: iconset.appendingPathComponent("icon_\(size)x\(size).png"))
    try render(size * 2).write(to: iconset.appendingPathComponent("icon_\(size)x\(size)@2x.png"))
}
try render(1024).write(to: packaging.appendingPathComponent("Framecraft.png"))
let process = Process()
process.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
process.arguments = ["-c", "icns", iconset.path, "-o", packaging.appendingPathComponent("Framecraft.icns").path]
try process.run()
process.waitUntilExit()
guard process.terminationStatus == 0 else {
    fputs("iconutil failed. Run the generator in a normal macOS terminal with access to native image services.\n", stderr)
    exit(EXIT_FAILURE)
}
try FileManager.default.removeItem(at: iconset)
print("Created Framecraft.png and Framecraft.icns")
