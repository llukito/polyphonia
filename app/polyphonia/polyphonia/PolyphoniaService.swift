import Foundation
import Security

struct AppIdentity: Sendable {
  let publicKey: Data
  let secretKey: Data
}

struct SealedCarrier: Sendable {
  let fileURL: URL
  let summary: String
}

struct OpenedMessage: Sendable {
  let text: String
  let summary: String
}

private func carrierSummary(_ carrier: CarrierKind, algorithm: Algorithm, byteCount: Int) -> String
{
  let name: String
  switch carrier {
  case .stealthPng: name = "Stealth PNG"
  case .robustPng: name = "Robust PNG"
  case .musicWav: name = "Music WAV"
  case .musicMidi: name = "Music MIDI"
  case .rawDebug: name = "Raw debug"
  }
  let size = ByteCountFormatter.string(fromByteCount: Int64(byteCount), countStyle: .file)
  return "\(name) · \(size) · \(CryptoAlgorithm(algorithm).title)"
}

/// Small boundary between SwiftUI and the generated Rust API.
protocol PolyphoniaBackend: Sendable {
  func detectAiServers() -> [AiServer]
  func shorten(message: String, server: AiServer, model: String) throws -> AiShortenSuggestion
  func generateDecoy(message: String, server: AiServer, model: String) throws -> AiDecoySuggestion
  func identity(algorithm: CryptoAlgorithm) throws -> AppIdentity
  func seal(
    message: String, recipientPublicKey: Data, decoy: String?, passphrase: String?,
    algorithm: CryptoAlgorithm, compact: Bool,
    carrier: CarrierKind, coverImage: Data?, coverAudio: Data?
  ) throws -> SealedCarrier
  func open(file: Data, passphrase: String?) throws -> OpenedMessage
}

final class PolyphoniaService: PolyphoniaBackend, @unchecked Sendable {
  private let vault: any SecretVault
  // Serializes read/modify/write of identity and session Keychain items.
  private let lock = NSLock()

  init(vault: any SecretVault = KeychainSecretVault()) { self.vault = vault }

  private func synchronized<T>(_ operation: () throws -> T) rethrows -> T {
    lock.lock()
    defer { lock.unlock() }
    return try operation()
  }

  func detectAiServers() -> [AiServer] { aiDetectServers() }

  func shorten(message: String, server: AiServer, model: String) throws -> AiShortenSuggestion {
    try aiShorten(server: server, model: model, text: message)
  }

  func generateDecoy(message: String, server: AiServer, model: String) throws
    -> AiDecoySuggestion
  {
    try aiGenerateDecoy(
      server: server, model: model,
      targetTextLenBytes: UInt64(message.utf8.count),
      script: aiDetectScript(text: message), tone: nil)
  }

  func identity(algorithm: CryptoAlgorithm) throws -> AppIdentity {
    try synchronized { try loadIdentity(algorithm) }
  }

  private func loadIdentity(_ algorithm: CryptoAlgorithm) throws -> AppIdentity {
    // Preserve the old Fake account for reading existing demo files.
    let account = algorithm == .fake ? "fake-v1" : "identity-\(algorithm.rawValue)-v1"
    if let data = try vault.load(account: account) {
      return try PropertyListDecoder().decode(StoredIdentity.self, from: data).identity
    }
    let generated = try generateIdentity(algorithm: algorithm.core)
    let saved = AppIdentity(publicKey: generated.publicKey, secretKey: generated.secretKey)
    try vault.save(
      PropertyListEncoder().encode(StoredIdentity(identity: saved)), account: account)
    return saved
  }

  func seal(
    message: String, recipientPublicKey: Data, decoy: String?, passphrase: String?,
    algorithm: CryptoAlgorithm, compact: Bool = false,
    carrier: CarrierKind, coverImage: Data? = nil, coverAudio: Data? = nil
  ) throws
    -> SealedCarrier
  {
    try synchronized {
      guard algorithm != .fake else { throw CryptoSetupError.legacyContact }
      var sessions = try loadSessions()
      let recipientID = contactId(publicKey: recipientPublicKey)
      let outgoing = sessions.last {
        $0.outgoing && $0.recipientID == recipientID && $0.algorithm == algorithm
          && Date().timeIntervalSince($0.created) < StoredSession.outgoingLifetime
      }
      if compact && outgoing == nil { throw CryptoSetupError.fullMessageRequired }
      let result = try encrypt(
        request: EncryptRequest(
          message: message,
          decoy: decoy,
          passphrase: passphrase,
          recipientPublicKey: recipientPublicKey,
          algorithm: algorithm.core,
          carrier: carrier,
          options: CarrierOptions(
            coverImage: carrier == .stealthPng ? coverImage : nil,
            coverAudio: carrier == .musicWav ? coverAudio : nil),
          sessionKey: compact ? outgoing?.key : nil
        ))
      let fileURL = FileManager.default.temporaryDirectory
        .appendingPathComponent("Polyphonia-\(UUID().uuidString)")
        .appendingPathExtension(result.`extension`)
      if let key = result.sessionKey {
        sessions.removeAll {
          $0.outgoing && $0.recipientID == recipientID && $0.algorithm == algorithm
        }
        sessions.append(
          StoredSession(
            key: key, recipientID: recipientID, algorithm: algorithm, outgoing: true,
            created: Date()))
        try saveSessions(sessions)
      }
      try result.bytes.write(to: fileURL, options: .atomic)
      return SealedCarrier(
        fileURL: fileURL,
        summary: carrierSummary(
          result.carrier, algorithm: result.algorithm, byteCount: result.bytes.count))
    }
  }

  func open(file: Data, passphrase: String?) throws -> OpenedMessage {
    try synchronized {
      let header = try inspect(file: file)
      let algorithm = CryptoAlgorithm(header.algorithm)
      let identity = try loadIdentity(algorithm)
      var sessions = try loadSessions()
      let result = try decrypt(
        request: DecryptRequest(
          file: file,
          mySecretKey: identity.secretKey,
          myPublicKey: identity.publicKey,
          passphrase: passphrase,
          sessionKeys: sessions.filter { !$0.outgoing && $0.algorithm == algorithm }.map(\.key)
        ))
      if let key = result.sessionKey {
        sessions.removeAll { !$0.outgoing && $0.key == key }
        sessions.append(
          StoredSession(
            key: key, recipientID: header.contactId, algorithm: algorithm,
            outgoing: false, created: Date()))
        try saveSessions(sessions)
      }
      // Slot is intentionally not returned to the interface.
      return OpenedMessage(
        text: result.text,
        summary: carrierSummary(result.carrier, algorithm: result.algorithm, byteCount: file.count))
    }
  }

  private func loadSessions() throws -> [StoredSession] {
    guard let data = try vault.load(account: "sessions-v1") else { return [] }
    let sessions = try PropertyListDecoder().decode([StoredSession].self, from: data)
    let current = sessions.filter { Date().timeIntervalSince($0.created) < StoredSession.retention }
    if current.count != sessions.count { try saveSessions(current) }
    return current
  }

  private func saveSessions(_ sessions: [StoredSession]) throws {
    try vault.save(PropertyListEncoder().encode(sessions), account: "sessions-v1")
  }
}

/// Only this boundary persists private material. Tests inject an in-memory vault.
protocol SecretVault: Sendable {
  func load(account: String) throws -> Data?
  func save(_ data: Data, account: String) throws
}

final class KeychainSecretVault: SecretVault, @unchecked Sendable {
  // Keep the original service so existing Fake identities remain readable.
  private let serviceName = "me.polyphonia.demo-identity"

  func load(account: String) throws -> Data? {
    var query = baseQuery(account)
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    if status == errSecItemNotFound { return nil }
    guard status == errSecSuccess, let data = result as? Data else {
      throw KeychainError.operationFailed(status)
    }
    return data
  }

  func save(_ data: Data, account: String) throws {
    let query = baseQuery(account)
    let update = [kSecValueData as String: data]
    var status = SecItemUpdate(query as CFDictionary, update as CFDictionary)
    if status == errSecItemNotFound {
      var item = query
      item[kSecValueData as String] = data
      item[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
      status = SecItemAdd(item as CFDictionary, nil)
    }
    guard status == errSecSuccess else { throw KeychainError.operationFailed(status) }
  }

  private func baseQuery(_ account: String) -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: serviceName, kSecAttrAccount as String: account,
    ]
  }
}

private struct StoredSession: Codable {
  // Rotate sending sessions after a week; retain receiving keys for 30 days for delayed files.
  static let outgoingLifetime: TimeInterval = 7 * 24 * 60 * 60
  static let retention: TimeInterval = 30 * 24 * 60 * 60
  let key: Data
  let recipientID: Data
  let algorithm: CryptoAlgorithm
  let outgoing: Bool
  let created: Date
}

enum CryptoSetupError: LocalizedError {
  case legacyContact, fullMessageRequired

  var errorDescription: String? {
    switch self {
    case .legacyContact:
      "This contact uses an insecure demo key. Ask them to export a new ML-KEM or RSA public key."
    case .fullMessageRequired:
      "Send a full message first to start a session. Turn off compact messages and try again."
    }
  }
}

private struct StoredIdentity: Codable {
  let publicKey: Data
  let secretKey: Data

  init(identity: AppIdentity) {
    publicKey = identity.publicKey
    secretKey = identity.secretKey
  }

  var identity: AppIdentity { AppIdentity(publicKey: publicKey, secretKey: secretKey) }
}

private enum KeychainError: LocalizedError {
  case operationFailed(OSStatus)

  var errorDescription: String? {
    switch self {
    case .operationFailed:
      return "Polyphonia could not access its identity in Keychain."
    }
  }
}
