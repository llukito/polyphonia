import AppKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

private enum KeyImageCheckError: Error { case failed(String) }

@MainActor
enum PublicKeyImageChecks {
  private static func require(_ condition: Bool, _ reason: String) throws {
    if !condition { throw KeyImageCheckError.failed(reason) }
  }

  private static func wait(_ condition: @escaping @MainActor () -> Bool) async throws {
    let deadline = Date().addingTimeInterval(15)
    while !condition() {
      guard Date() < deadline else {
        throw KeyImageCheckError.failed("Key-image operation timed out")
      }
      try await Task.sleep(for: .milliseconds(20))
    }
  }

  private static func picture(side: Int) throws -> Data {
    let rep = NSBitmapImageRep(
      bitmapDataPlanes: nil, pixelsWide: side, pixelsHigh: side, bitsPerSample: 8,
      samplesPerPixel: 3, hasAlpha: false, isPlanar: false, colorSpaceName: .deviceRGB,
      bytesPerRow: side * 3, bitsPerPixel: 24)!
    for y in 0..<side {
      for x in 0..<side {
        let index = y * rep.bytesPerRow + x * 3
        rep.bitmapData![index] = UInt8(x % 256)
        rep.bitmapData![index + 1] = UInt8(y % 256)
        rep.bitmapData![index + 2] = 170
      }
    }
    return rep.representation(using: .png, properties: [:])!
  }

  static func run() async throws {
    let cover = try picture(side: 256)
    let tiny = try picture(side: 8)
    try require(try PublicKeyImage.decode(cover) == nil, "Ordinary picture became an invitation")

    for algorithm in CryptoAlgorithm.secureAlgorithms {
      let receiver = PolyphoniaService(vault: MemorySecretVault())
      let identity = try receiver.identity(algorithm: algorithm)
      let generated = try PublicKeyImage.encode(publicKey: identity.publicKey, algorithm: algorithm)
      let custom = try PublicKeyImage.encode(
        publicKey: identity.publicKey, algorithm: algorithm, cover: cover)
      let jpegCover = NSBitmapImageRep(data: cover)!.representation(using: .jpeg, properties: [:])!
      let fromJpeg = try PublicKeyImage.encode(
        publicKey: identity.publicKey, algorithm: algorithm, cover: jpegCover)
      let repeated = try PublicKeyImage.encode(publicKey: identity.publicKey, algorithm: algorithm)
      try require(generated != repeated, "Generated covers repeat")
      for data in [generated, custom, fromJpeg] {
        let invitation = try PublicKeyImage.decode(data)
        try require(
          invitation?.publicKey == identity.publicKey && invitation?.algorithm == algorithm,
          "Key image lost its key or algorithm")
      }
      let customBitmap = NSBitmapImageRep(data: custom)!
      try require(
        customBitmap.pixelsWide == 256 && customBitmap.pixelsHigh == 256,
        "Custom photo dimensions changed unnecessarily")
      do {
        _ = try PublicKeyImage.encode(
          publicKey: identity.publicKey, algorithm: algorithm, cover: tiny)
        throw KeyImageCheckError.failed("Tiny cover was accepted")
      } catch PublicKeyImageError.tooSmall {}

      // Damage a bit in the public key, leaving the invitation header intact.
      let damaged = NSBitmapImageRep(data: custom)!
      damaged.bitmapData![60 * damaged.samplesPerPixel] ^= 1
      let damagedPNG = damaged.representation(using: .png, properties: [:])!
      do {
        _ = try PublicKeyImage.decode(damagedPNG)
        throw KeyImageCheckError.failed("Damaged invitation was accepted")
      } catch PublicKeyImageError.damagedImage {}

      // The cover can contain metadata, but the exported invitation must not retain it.
      let source = CGImageSourceCreateWithData(cover as CFData, nil)!
      let tagged = NSMutableData()
      let destination = CGImageDestinationCreateWithData(
        tagged, UTType.png.identifier as CFString, 1, nil)!
      let metadata: [CFString: Any] = [
        kCGImagePropertyPNGDictionary: [kCGImagePropertyPNGDescription: "private photo metadata"]
      ]
      CGImageDestinationAddImageFromSource(destination, source, 0, metadata as CFDictionary)
      try require(CGImageDestinationFinalize(destination), "Could not create metadata cover")
      let clean = try PublicKeyImage.encode(
        publicKey: identity.publicKey, algorithm: algorithm, cover: tagged as Data)
      let cleanSource = CGImageSourceCreateWithData(clean as CFData, nil)!
      let properties = CGImageSourceCopyPropertiesAtIndex(cleanSource, 0, nil)! as NSDictionary
      let pngProperties = properties[kCGImagePropertyPNGDictionary] as? NSDictionary
      try require(
        pngProperties?[kCGImagePropertyPNGDescription] == nil,
        "Export retained identifying photo metadata")

      let suite = "me.polyphonia.key-image-tests.\(UUID().uuidString)"
      let defaults = UserDefaults(suiteName: suite)!
      defer { defaults.removePersistentDomain(forName: suite) }
      let board = NSPasteboard.withUniqueName()
      defer { board.releaseGlobally() }
      let sender = PolyphoniaService(vault: MemorySecretVault())
      let session = MessageSession(service: sender, defaults: defaults, pasteboard: board)
      try await wait { session.selectedContact != nil }
      let originalCount = session.contacts.count

      // Copy Image: PNG and TIFF-only clipboards must both open a review, never silently import.
      board.setData(custom, forType: .png)
      session.pasteReceivedFile()
      try await wait { !session.isWorking }
      try require(session.screen == .contact, "Copied PNG did not open contact review")
      try require(session.contacts.count == originalCount, "Key imported without confirmation")
      session.cancelInvitation()
      try require(session.contacts.count == originalCount, "Cancel imported a key")
      session.open(imageData: custom)
      try await wait { !session.isWorking }
      session.open(imageData: damagedPNG)
      try await wait { !session.isWorking }
      session.confirmInvitation()
      try require(
        session.pendingInvitation == nil && session.contacts.count == originalCount,
        "A failed open left a stale key available to import")
      board.clearContents()
      board.setData(NSBitmapImageRep(data: custom)!.tiffRepresentation, forType: .tiff)
      session.pasteReceivedFile()
      try await wait { !session.isWorking }
      try require(
        session.pendingInvitation?.publicKey == identity.publicKey, "TIFF paste lost key bits")
      session.invitationName = "Receiver"
      session.confirmInvitation()
      try require(session.selectedContact?.name == "Receiver", "Confirmed contact was not selected")
      try require(
        session.selectedContact?.publicKey == identity.publicKey, "Imported the wrong key")

      let count = session.contacts.count
      let keyFile = FileManager.default.temporaryDirectory.appendingPathComponent(
        "\(UUID().uuidString).png")
      defer { try? FileManager.default.removeItem(at: keyFile) }
      try custom.write(to: keyFile)
      session.importContact(fileURL: keyFile)
      try await wait { !session.isWorking }
      try require(session.invitationName == "Receiver", "Reimport lost the local contact name")
      session.confirmInvitation()
      try require(session.contacts.count == count, "Reimport duplicated a contact")

      // A key received without any existing shared key must enable real encrypted replies.
      session.message = "Reply after adding the key image."
      session.platformSelection.selectMode(.stealth)
      session.sealAndCopy(as: .image)
      try await wait { !session.isWorking }
      guard let url = (board.readObjects(forClasses: [NSURL.self]) as? [URL])?.first else {
        throw KeyImageCheckError.failed("Reply was not copied")
      }
      defer { try? FileManager.default.removeItem(at: url) }
      let reply = try receiver.open(file: Data(contentsOf: url), passphrase: nil)
      try require(reply.text == session.message, "Received key could not encrypt a reply")

      // Both export options produce a real file on the clipboard and preserve the input photo.
      for coverURL in [nil, keyFile] {
        session.exportPublicKeyImage(algorithm: algorithm, coverURL: coverURL)
        try await wait { !session.isWorking }
        guard let exportedURL = (board.readObjects(forClasses: [NSURL.self]) as? [URL])?.first
        else {
          throw KeyImageCheckError.failed("Key image was not copied")
        }
        defer { try? FileManager.default.removeItem(at: exportedURL) }
        let exported = try PublicKeyImage.decode(Data(contentsOf: exportedURL))
        try require(
          exported?.publicKey == sender.identity(algorithm: algorithm).publicKey,
          "Export shared a different identity")
        try require(session.screen == .result, "Key export did not show Save file")
      }
      try require(try Data(contentsOf: keyFile) == custom, "Export overwrote the chosen cover")
      print(
        "PASS \(algorithm.title): key images, custom covers, metadata removal, PNG/TIFF paste, review, cancel, reimport and encrypted reply"
      )
    }
  }
}
