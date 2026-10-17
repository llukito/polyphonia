import Foundation

/// Isolated secret storage: tests never read or write the user's Keychain.
final class MemorySecretVault: SecretVault, @unchecked Sendable {
  private let lock = NSLock()
  private var values: [String: Data] = [:]
  func load(account: String) -> Data? { lock.withLock { values[account] } }
  func save(_ data: Data, account: String) { lock.withLock { values[account] = data } }
}

private enum CryptoCheckError: Error { case failed(String) }

struct CryptoFlowChecks {
  static func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw CryptoCheckError.failed(message) }
  }

  static func run() throws {
    for algorithm in CryptoAlgorithm.secureAlgorithms {
      let senderVault = MemorySecretVault()
      let receiverVault = MemorySecretVault()
      let sender = PolyphoniaService(vault: senderVault)
      let receiver = PolyphoniaService(vault: receiverVault)
      let identity = try receiver.identity(algorithm: algorithm)
      let reloaded = try PolyphoniaService(vault: receiverVault).identity(algorithm: algorithm)
      try require(identity.publicKey == reloaded.publicKey, "Identity changed on reload")
      let contactFile = FileManager.default.temporaryDirectory.appendingPathComponent(
        UUID().uuidString + ".pub")
      defer { try? FileManager.default.removeItem(at: contactFile) }
      try Contact.writePublicKey(identity.publicKey, algorithm: algorithm, to: contactFile)
      let contact = try Contact.read(from: contactFile)
      try require(
        contact.algorithm == algorithm && contact.publicKey == identity.publicKey,
        "Public key import lost its algorithm")
      try expectFailure("Malformed key accepted") {
        try validatePublicKey(publicKey: Data([0]), algorithm: algorithm.core)
      }
      try expectFailure("Compact message sent before full message") {
        _ = try sender.seal(
          message: "text", recipientPublicKey: identity.publicKey,
          decoy: nil, passphrase: nil, algorithm: algorithm, compact: true, carrier: .rawDebug)
      }
      // Exercise both real algorithms through actual PNG and WAV carriers and service persistence.
      for carrier in [CarrierKind.stealthPng, .musicWav] {
        let real = "Private meeting at 17:00."
        let decoy = "Lunch tomorrow at noon."
        let full = try sender.seal(
          message: real, recipientPublicKey: identity.publicKey,
          decoy: decoy, passphrase: "agreed test phrase", algorithm: algorithm,
          carrier: algorithm == .mlkem ? .stealthPng : carrier)
        defer { try? FileManager.default.removeItem(at: full.fileURL) }
        let fullBytes = try Data(contentsOf: full.fileURL)
        try require(
          try inspect(file: fullBytes).algorithm == algorithm.core, "Fake algorithm still used")
        let stranger = PolyphoniaService(vault: MemorySecretVault())
        do {
          _ = try stranger.open(file: fullBytes, passphrase: "agreed test phrase")
          throw CryptoCheckError.failed("Wrong recipient opened a real message")
        } catch PolyError.WrongRecipient {}

        // Both full and compact paths must preserve decoy behavior without disclosing the slot.
        let compact = try PolyphoniaService(vault: senderVault).seal(
          message: real,
          recipientPublicKey: identity.publicKey, decoy: decoy, passphrase: "agreed test phrase",
          algorithm: algorithm, compact: true, carrier: carrier)
        defer { try? FileManager.default.removeItem(at: compact.fileURL) }
        let compactBytes = try Data(contentsOf: compact.fileURL)
        try require(try inspect(file: compactBytes).isSession, "Compact mode omitted SESSION flag")
        // A device with the identity but without the full message has no session key yet.
        let freshVault = MemorySecretVault()
        if let bytes = receiverVault.load(account: "identity-\(algorithm.rawValue)-v1") {
          freshVault.save(bytes, account: "identity-\(algorithm.rawValue)-v1")
        }
        do {
          _ = try PolyphoniaService(vault: freshVault).open(file: compactBytes, passphrase: nil)
          throw CryptoCheckError.failed("Unknown session opened")
        } catch PolyError.UnknownSession {}
        for file in [fullBytes, compactBytes] {
          let reopened = PolyphoniaService(vault: receiverVault)
          let without = try reopened.open(file: file, passphrase: nil)
          let wrong = try reopened.open(file: file, passphrase: "wrong phrase")
          let correct = try reopened.open(file: file, passphrase: "agreed test phrase")
          try require(
            without.text == decoy && wrong.text == decoy && correct.text == real,
            "Real crypto did not gate the hidden message")
          try require(without.summary == correct.summary, "Interface disclosed which slot opened")
        }
      }
      // Compact envelopes also fit the two carriers most affected by KEM overhead.
      for carrier in [CarrierKind.robustPng, .musicMidi] {
        let file = try sender.seal(
          message: "ordinary message", recipientPublicKey: identity.publicKey,
          decoy: nil, passphrase: nil, algorithm: algorithm, compact: true, carrier: carrier)
        defer { try? FileManager.default.removeItem(at: file.fileURL) }
        let opened = try receiver.open(file: Data(contentsOf: file.fileURL), passphrase: nil)
        try require(opened.text == "ordinary message", "Compact small carrier failed")
      }
      // User-supplied WAVs have capacity based on duration and can hold a full ML-KEM message.
      let covered = try sender.seal(
        message: "custom music", recipientPublicKey: identity.publicKey,
        decoy: nil, passphrase: nil, algorithm: algorithm, carrier: .musicWav,
        coverAudio: silentWav())
      defer { try? FileManager.default.removeItem(at: covered.fileURL) }
      let coveredBytes = try Data(contentsOf: covered.fileURL)
      try require(
        try !inspect(file: coveredBytes).isSession, "Full custom WAV unexpectedly used a session")
      try require(
        try receiver.open(file: coveredBytes, passphrase: nil).text == "custom music",
        "Full real encryption in a custom WAV failed")
      // Open above stored the newest full message's session. Expiration never rotates silently.
      let compact = try sender.seal(
        message: "later", recipientPublicKey: identity.publicKey,
        decoy: nil, passphrase: nil, algorithm: algorithm, compact: true, carrier: .rawDebug)
      defer { try? FileManager.default.removeItem(at: compact.fileURL) }
      try expireSessions(in: senderVault)
      try expectFailure("Expired sending session was reused") {
        _ = try sender.seal(
          message: "later", recipientPublicKey: identity.publicKey,
          decoy: nil, passphrase: nil, algorithm: algorithm, compact: true, carrier: .rawDebug)
      }
      try expireSessions(in: receiverVault)
      do {
        _ = try receiver.open(file: Data(contentsOf: compact.fileURL), passphrase: nil)
        throw CryptoCheckError.failed("Expired receiving session was reused")
      } catch PolyError.UnknownSession {}
      // Reopening the original full file restores the session even after expiry.
      _ = try receiver.open(file: coveredBytes, passphrase: nil)
      try require(
        try receiver.open(file: Data(contentsOf: compact.fileURL), passphrase: nil).text == "later",
        "Reopening the full message did not restore the expired session")
      print(
        "PASS \(algorithm.title): keys, contacts, all carriers, custom WAV, decoys, wrong recipient, sessions, reload and expiry"
      )
    }
    try checkLegacy()
  }

  private static func expireSessions(in vault: MemorySecretVault) throws {
    guard let data = vault.load(account: "sessions-v1"),
      var sessions = try PropertyListSerialization.propertyList(from: data, format: nil)
        as? [[String: Any]]
    else { throw CryptoCheckError.failed("Missing stored sessions") }
    for index in sessions.indices { sessions[index]["created"] = Date.distantPast }
    vault.save(
      try PropertyListSerialization.data(fromPropertyList: sessions, format: .binary, options: 0),
      account: "sessions-v1")
  }

  private static func silentWav() -> Data {
    // One second of mono 44.1 kHz, 16-bit PCM, matching the supported custom WAV format.
    let sampleRate: UInt32 = 44_100
    let bitsPerSample: UInt16 = 16
    let blockAlign = bitsPerSample / 8
    let byteCount = sampleRate * UInt32(blockAlign)
    var bytes = Data()
    func ascii(_ text: String) { bytes.append(contentsOf: text.utf8) }
    func little<T: FixedWidthInteger>(_ number: T) {
      var value = number.littleEndian
      withUnsafeBytes(of: &value) { bytes.append(contentsOf: $0) }
    }
    // RIFF payload header is 36 bytes, fmt PCM body is 16 bytes (WAVE format).
    ascii("RIFF")
    little(byteCount + 36)
    ascii("WAVEfmt ")
    little(UInt32(16))
    little(UInt16(1))
    little(UInt16(1))
    little(sampleRate)
    little(sampleRate * UInt32(blockAlign))
    little(blockAlign)
    little(bitsPerSample)
    ascii("data")
    little(byteCount)
    bytes.append(Data(count: Int(byteCount)))
    return bytes
  }

  private static func expectFailure(_ message: String, operation: () throws -> Void) throws {
    do { try operation() } catch { return }
    throw CryptoCheckError.failed(message)
  }

  private static func checkLegacy() throws {
    let vault = MemorySecretVault()
    let service = PolyphoniaService(vault: vault)
    let identity = try service.identity(algorithm: .fake)
    let oldContact = try JSONSerialization.data(withJSONObject: [
      "id": UUID().uuidString, "name": "Old contact",
      "publicKey": identity.publicKey.base64EncodedString(),
    ])
    let contact = try JSONDecoder().decode(Contact.self, from: oldContact)
    try require(contact.algorithm == .fake, "Old contact was promoted to a secure algorithm")
    try expectFailure("Fake contact allowed for new messages") {
      _ = try service.seal(
        message: "text", recipientPublicKey: identity.publicKey,
        decoy: nil, passphrase: nil, algorithm: .fake, carrier: .rawDebug)
    }
    let legacy = try encrypt(
      request: EncryptRequest(
        message: "legacy text",
        recipientPublicKey: identity.publicKey, algorithm: .fake, carrier: .rawDebug,
        options: CarrierOptions()))
    try require(
      try service.open(file: legacy.bytes, passphrase: nil).text == "legacy text",
      "Old Fake message no longer readable")
    print("PASS legacy contacts remain Fake; old files open; new Fake sealing is blocked")
  }
}
