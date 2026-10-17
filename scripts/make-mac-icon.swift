// Genera el icono de macOS a partir del de Windows (extra/logo/nebula.png):
// lienzo de 1024 con 100 px de margen transparente y baldosa de 824 con
// esquinas de radio ≈ 18 %, como el resto de apps Poxi para Mac.
//
//   swift scripts/make-mac-icon.swift [entrada.png] [salida.png]
import AppKit

let args = CommandLine.arguments
let input = args.count > 1 ? args[1] : "extra/logo/nebula.png"
let output = args.count > 2 ? args[2] : "packaging/macos/icon.png"

guard let source = NSImage(contentsOfFile: input),
      let cg = source.cgImage(forProposedRect: nil, context: nil, hints: nil)
else {
    FileHandle.standardError.write("No se pudo leer \(input)\n".data(using: .utf8)!)
    exit(1)
}

let canvas = 1024
let margin = 100.0
let tile = 824.0
// La baldosa del icono de Windows ocupa de 32 a 992 px del lienzo de 1024.
let sourceTileOrigin = 32.0 / 1024.0 * Double(cg.width)
let sourceTileSize = 960.0 / 1024.0 * Double(cg.width)

let ctx = CGContext(
    data: nil, width: canvas, height: canvas, bitsPerComponent: 8, bytesPerRow: 0,
    space: CGColorSpace(name: CGColorSpace.sRGB)!,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.interpolationQuality = .none  // pixel-art: sin suavizado

let tileRect = CGRect(x: margin, y: margin, width: tile, height: tile)
ctx.addPath(CGPath(roundedRect: tileRect, cornerWidth: tile * 0.18, cornerHeight: tile * 0.18, transform: nil))
ctx.clip()
let scale = tile / sourceTileSize
let drawSize = Double(cg.width) * scale
let drawOrigin = margin - sourceTileOrigin * scale
ctx.draw(cg, in: CGRect(x: drawOrigin, y: drawOrigin, width: drawSize, height: drawSize))

let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
try! FileManager.default.createDirectory(
    atPath: (output as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: output))
print("Icono guardado en \(output)")
