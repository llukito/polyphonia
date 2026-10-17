import CoreGraphics
import CryptoKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

struct PublicKeyInvitation: Sendable {
  let publicKey: Data
  let algorithm: CryptoAlgorithm

  var fingerprint: String {
    let digest = Array(SHA256.hash(data: publicKey))
    // Four-byte groups make the full SHA-256 fingerprint easier to compare.
    let groupSize = 4
    return stride(from: 0, to: digest.count, by: groupSize).map { start in
      digest[start..<min(start + groupSize, digest.count)]
        .map { String(format: "%02X", $0) }.joined()
    }.joined(separator: " ")
  }
}

/// Public invitation format, independent of the encrypted POLY envelope:
/// PKEYIMG! | version (1 byte) | algorithm (1 byte) | key length (u16 BE) | key | SHA-256.
/// The checksum detects damage; it does not authenticate the person sharing the image.
/// Only public key material is embedded, one bit per RGB channel; no PNG metadata is copied.
enum PublicKeyImage {
  private static let magic = Array("PKEYIMG!".utf8)
  private static let version: UInt8 = 1
  private static let headerSize = magic.count + 1 + 1 + MemoryLayout<UInt16>.size
  private static let digestSize = SHA256.byteCount
  // Both supported public keys fit well below 4 KiB. Bound untrusted length fields before allocation.
  private static let maximumKeyBytes = 4 * 1024
  private static let bitsPerByte = 8
  private static let colorChannels = 3
  private static let pixelStride = 4
  private static let channelMaximum = 255
  // A 512px wallpaper has ample space; chosen photos are limited to 2048px on their longest side.
  private static let generatedSide = 512
  private static let maximumCoverSide = 2048
  // Bound decoding to 16 megapixels before ImageIO allocates the bitmap.
  private static let maximumDecodePixels = 16 * 1024 * 1024
  private static let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)!
  private static let bitmapInfo =
    CGBitmapInfo.byteOrder32Big.rawValue
    | CGImageAlphaInfo.noneSkipLast.rawValue

  static func encode(publicKey: Data, algorithm: CryptoAlgorithm, cover: Data? = nil) throws -> Data
  {
    guard algorithm != .fake, publicKey.count <= maximumKeyBytes else {
      throw PublicKeyImageError.invalidInvitation
    }
    try validatePublicKey(publicKey: publicKey, algorithm: algorithm.core)
    var payload = magic + [version, algorithm == .mlkem ? 1 : 2]
    let length = UInt16(publicKey.count)
    payload += [UInt8(length >> bitsPerByte), UInt8(length & UInt16(channelMaximum))]
    payload += publicKey
    payload += SHA256.hash(data: Data(payload))

    var raster: Raster
    if let cover {
      guard let source = CGImageSourceCreateWithData(cover as CFData, nil),
        let image = CGImageSourceCreateThumbnailAtIndex(
          source, 0,
          [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maximumCoverSide,
            kCGImageSourceShouldCacheImmediately: false,
          ] as CFDictionary)
      else { throw PublicKeyImageError.invalidImage }
      raster = try Raster(image: image)
    } else {
      raster = try generatedCover()
    }
    guard payload.count <= raster.capacity else { throw PublicKeyImageError.tooSmall }
    for (byteIndex, byte) in payload.enumerated() {
      for bit in 0..<bitsPerByte {
        let position = byteIndex * bitsPerByte + bit
        let channel = position / colorChannels * pixelStride + position % colorChannels
        let value = (byte >> (bitsPerByte - 1 - bit)) & 1
        raster.pixels[channel] = (raster.pixels[channel] & ~1) | value
      }
    }
    return try raster.png()
  }

  /// Returns nil for ordinary pictures and encrypted carriers, so Open can continue normal detection.
  static func decode(_ file: Data) throws -> PublicKeyInvitation? {
    guard let source = CGImageSourceCreateWithData(file as CFData, nil),
      let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
      let width = properties[kCGImagePropertyPixelWidth] as? Int,
      let height = properties[kCGImagePropertyPixelHeight] as? Int,
      width > 0, height > 0, width <= maximumDecodePixels / height,
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { return nil }
    let raster = try Raster(image: image)
    guard raster.capacity >= headerSize else { return nil }
    let header = raster.read(count: headerSize)
    guard header.starts(with: magic) else { return nil }
    guard header[magic.count] == version else { throw PublicKeyImageError.unsupportedVersion }
    let algorithm: CryptoAlgorithm
    switch header[magic.count + 1] {
    case 1: algorithm = .mlkem
    case 2: algorithm = .rsa
    default: throw PublicKeyImageError.invalidInvitation
    }
    let keyLength = Int(header[headerSize - 2]) << bitsPerByte | Int(header[headerSize - 1])
    let bodySize = headerSize + keyLength
    let total = bodySize + digestSize
    guard keyLength > 0, keyLength <= maximumKeyBytes, total <= raster.capacity else {
      throw PublicKeyImageError.damagedImage
    }
    let bytes = raster.read(count: total)
    let body = Data(bytes.prefix(bodySize))
    guard Data(SHA256.hash(data: body)) == Data(bytes.suffix(digestSize)) else {
      throw PublicKeyImageError.damagedImage
    }
    let key = Data(bytes[headerSize..<bodySize])
    do { try validatePublicKey(publicKey: key, algorithm: algorithm.core) } catch {
      throw PublicKeyImageError.invalidInvitation
    }
    return PublicKeyInvitation(publicKey: key, algorithm: algorithm)
  }

  private static func generatedCover() throws -> Raster {
    // Random pastel colors and fine grain create a plain wallpaper without logos or identifying text.
    let componentRange = 0.2...0.85
    let grainRange = -3...3
    let from = (0..<colorChannels).map { _ in Double.random(in: componentRange) }
    let to = (0..<colorChannels).map { _ in Double.random(in: componentRange) }
    var pixels = [UInt8](
      repeating: UInt8(channelMaximum), count: generatedSide * generatedSide * pixelStride)
    let span = Double(2 * (generatedSide - 1))
    for y in 0..<generatedSide {
      for x in 0..<generatedSide {
        let blend = Double(x + y) / span
        for channel in 0..<colorChannels {
          let value =
            Int((from[channel] * (1 - blend) + to[channel] * blend) * Double(channelMaximum))
            + Int.random(in: grainRange)
          pixels[(y * generatedSide + x) * pixelStride + channel] = UInt8(clamping: value)
        }
      }
    }
    return Raster(width: generatedSide, height: generatedSide, pixels: pixels)
  }

  private struct Raster {
    let width: Int
    let height: Int
    var pixels: [UInt8]
    var capacity: Int { width * height * colorChannels / bitsPerByte }

    init(width: Int, height: Int, pixels: [UInt8]) {
      self.width = width
      self.height = height
      self.pixels = pixels
    }

    init(image: CGImage) throws {
      let width = image.width
      let height = image.height
      var bytes = [UInt8](repeating: UInt8(channelMaximum), count: width * height * pixelStride)
      let rendered = bytes.withUnsafeMutableBytes { buffer -> Bool in
        guard
          let context = CGContext(
            data: buffer.baseAddress, width: width, height: height, bitsPerComponent: bitsPerByte,
            bytesPerRow: width * pixelStride, space: colorSpace, bitmapInfo: bitmapInfo)
        else { return false }
        // Flatten transparency onto white so PNG/clipboard round trips preserve the RGB LSBs.
        context.setFillColor(CGColor(gray: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return true
      }
      guard rendered else { throw PublicKeyImageError.invalidImage }
      self.width = width
      self.height = height
      pixels = bytes
    }

    func read(count: Int) -> [UInt8] {
      (0..<count).map { index in
        var byte: UInt8 = 0
        for bit in 0..<bitsPerByte {
          let position = index * bitsPerByte + bit
          let channel = position / colorChannels * pixelStride + position % colorChannels
          byte = (byte << 1) | (pixels[channel] & 1)
        }
        return byte
      }
    }

    func png() throws -> Data {
      let result = NSMutableData()
      guard let provider = CGDataProvider(data: Data(pixels) as CFData),
        let image = CGImage(
          width: width, height: height, bitsPerComponent: bitsPerByte,
          bitsPerPixel: pixelStride * bitsPerByte, bytesPerRow: width * pixelStride,
          space: colorSpace, bitmapInfo: CGBitmapInfo(rawValue: bitmapInfo), provider: provider,
          decode: nil, shouldInterpolate: false, intent: .defaultIntent),
        let destination = CGImageDestinationCreateWithData(
          result, UTType.png.identifier as CFString, 1, nil)
      else { throw PublicKeyImageError.invalidImage }
      CGImageDestinationAddImage(destination, image, nil)
      guard CGImageDestinationFinalize(destination) else { throw PublicKeyImageError.invalidImage }
      return result as Data
    }
  }
}

enum PublicKeyImageError: LocalizedError {
  case invalidImage, tooSmall, damagedImage, invalidInvitation, unsupportedVersion

  var errorDescription: String? {
    switch self {
    case .invalidImage: "Could not use that image. Choose another PNG or JPEG."
    case .tooSmall: "This photo is too small to hold your public key. Choose a larger image."
    case .damagedImage: "This key image was damaged. Ask them to resend the original PNG as a file."
    case .invalidInvitation: "This image contains an invalid public key."
    case .unsupportedVersion: "This key image needs a newer version of Polyphonia."
    }
  }
}
