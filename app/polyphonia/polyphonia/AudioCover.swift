import Foundation

/// Keep a snapshot so moving or editing the original file cannot change a pending seal.
struct AudioCover: Sendable {
  let name: String
  let bytes: Data

  static func read(from url: URL) throws -> AudioCover {
    let hasAccess = url.startAccessingSecurityScopedResource()
    defer { if hasAccess { url.stopAccessingSecurityScopedResource() } }
    return AudioCover(name: url.lastPathComponent, bytes: try Data(contentsOf: url))
  }
}
