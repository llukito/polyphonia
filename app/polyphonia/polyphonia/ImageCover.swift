import Foundation

/// Keep a snapshot so moving or editing the original file cannot change a pending seal.
struct ImageCover: Sendable {
  let name: String
  let bytes: Data

  static func read(from url: URL) throws -> ImageCover {
    let hasAccess = url.startAccessingSecurityScopedResource()
    defer { if hasAccess { url.stopAccessingSecurityScopedResource() } }
    return ImageCover(name: url.lastPathComponent, bytes: try Data(contentsOf: url))
  }
}
