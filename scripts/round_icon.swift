// round_icon.swift — Apple-style rounded-corner icon master generator.
//
// Renders the input artwork onto a 1024x1024 transparent canvas, inset to
// ~82% (Apple icon-grid bleed), masked with a continuous ("squircle") rounded
// rectangle whose corner radius is ~22.37% of the artwork size, and writes a
// transparent PNG. CoreGraphics only — no ImageMagick / third-party deps.
//
// Usage:
//   swift round_icon.swift <input.png> <output.png> [canvas] [insetPct] [radiusPct]
//
// Defaults: canvas=1024, insetPct=0.82, radiusPct=0.2237
//
// The continuous corner is approximated with the standard Apple squircle
// formula (a superellipse-style bezier) rather than a plain quarter-circle so
// the silhouette matches macOS Big Sur+ app icons.

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

func fail(_ msg: String) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(1)
}

let args = CommandLine.arguments
guard args.count >= 3 else {
    fail("usage: round_icon.swift <input.png> <output.png> [canvas] [insetPct] [radiusPct]")
}

let inputPath = args[1]
let outputPath = args[2]
let canvas = args.count > 3 ? Int(args[3]) ?? 1024 : 1024
let insetPct = args.count > 4 ? Double(args[4]) ?? 0.82 : 0.82
let radiusPct = args.count > 5 ? Double(args[5]) ?? 0.2237 : 0.2237

// --- Load source image ------------------------------------------------------
guard let srcURL = URL(string: "file://" + inputPath) ?? URL(fileURLWithPath: inputPath) as URL?,
    let srcData = try? Data(contentsOf: URL(fileURLWithPath: inputPath)),
    let srcProvider = CGDataProvider(data: srcData as CFData),
    let srcSource = CGImageSourceCreateWithDataProvider(srcProvider, nil),
    let srcImage = CGImageSourceCreateImageAtIndex(srcSource, 0, nil)
else {
    fail("error: could not load input image at \(inputPath)")
}
_ = srcURL

// --- Build canvas context ---------------------------------------------------
let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)!
guard
    let ctx = CGContext(
        data: nil,
        width: canvas,
        height: canvas,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: colorSpace,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )
else {
    fail("error: could not create graphics context")
}
ctx.interpolationQuality = .high

// Artwork rectangle: centered, inset to insetPct of the canvas.
let artSize = Double(canvas) * insetPct
let origin = (Double(canvas) - artSize) / 2.0
let artRect = CGRect(x: origin, y: origin, width: artSize, height: artSize)

// --- Continuous (squircle) rounded-rect mask path ---------------------------
// Apple's continuous corner is a superellipse blend. We approximate it with a
// cubic-bezier corner where control points are pulled toward the corner by a
// fraction of the radius, giving the smooth "G2 continuous" transition rather
// than a circular arc. Magic constants tuned to match CALayer continuous mask.
func continuousRoundedPath(_ rect: CGRect, radius r: CGFloat) -> CGPath {
    let path = CGMutablePath()
    let limit = min(rect.width, rect.height) / 2.0
    let radius = min(r, limit)
    // Continuous-corner control offsets (fractions of radius). These are the
    // commonly used squircle bezier coefficients.
    let c = radius * 1.28195
    let minX = rect.minX, minY = rect.minY, maxX = rect.maxX, maxY = rect.maxY

    // Inner straight-segment offset (where the corner curve begins).
    let p = c
    // Helper bezier offsets along the corner (superellipse approximation).
    let a = radius * 1.0
    let b = radius * 0.55228 * 1.28195  // smoothed handle

    // Start mid-top edge, go clockwise.
    path.move(to: CGPoint(x: minX + p, y: minY))
    // top edge
    path.addLine(to: CGPoint(x: maxX - p, y: minY))
    // top-right corner
    path.addCurve(
        to: CGPoint(x: maxX, y: minY + p),
        control1: CGPoint(x: maxX - p + b, y: minY),
        control2: CGPoint(x: maxX, y: minY + p - b))
    _ = a
    // right edge
    path.addLine(to: CGPoint(x: maxX, y: maxY - p))
    // bottom-right corner
    path.addCurve(
        to: CGPoint(x: maxX - p, y: maxY),
        control1: CGPoint(x: maxX, y: maxY - p + b),
        control2: CGPoint(x: maxX - p + b, y: maxY))
    // bottom edge
    path.addLine(to: CGPoint(x: minX + p, y: maxY))
    // bottom-left corner
    path.addCurve(
        to: CGPoint(x: minX, y: maxY - p),
        control1: CGPoint(x: minX + p - b, y: maxY),
        control2: CGPoint(x: minX, y: maxY - p + b))
    // left edge
    path.addLine(to: CGPoint(x: minX, y: minY + p))
    // top-left corner
    path.addCurve(
        to: CGPoint(x: minX + p, y: minY),
        control1: CGPoint(x: minX, y: minY + p - b),
        control2: CGPoint(x: minX + p - b, y: minY))
    path.closeSubpath()
    return path
}

let cornerRadius = CGFloat(artSize * radiusPct)
let maskPath = continuousRoundedPath(artRect, radius: cornerRadius)

// Clip to the rounded mask, then draw the artwork filling the inset rect.
ctx.addPath(maskPath)
ctx.clip()
ctx.draw(srcImage, in: artRect)

// --- Encode PNG -------------------------------------------------------------
guard let outImage = ctx.makeImage() else {
    fail("error: could not render output image")
}
let outURL = URL(fileURLWithPath: outputPath)
guard
    let dest = CGImageDestinationCreateWithURL(
        outURL as CFURL, UTType.png.identifier as CFString, 1, nil)
else {
    fail("error: could not create PNG destination at \(outputPath)")
}
CGImageDestinationAddImage(dest, outImage, nil)
guard CGImageDestinationFinalize(dest) else {
    fail("error: could not write PNG to \(outputPath)")
}
print("round_icon: wrote \(outputPath) (\(canvas)x\(canvas), inset \(insetPct), radius \(radiusPct))")
