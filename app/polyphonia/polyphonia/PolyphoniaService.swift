import Foundation
import Security

struct DemoIdentity: Sendable {
  let publicKey: Data
  let secretKey: Data
}

struct SealedCarrier: Sendable {
  let bytes: Data
  let fileExtension: String
}

/// Small boundary between SwiftUI and the generated Rust API.
final class PolyphoniaService: @unchecked Sendable {
  private let identityStore = KeychainIdentityStore()

  func demoIdentity() throws -> DemoIdentity {
    if let saved = try identityStore.load() { return saved }

    let identity = try generateIdentity(algorithm: .fake)
    let saved = DemoIdentity(publicKey: identity.publicKey, secretKey: identity.secretKey)
    try identityStore.save(saved)
    return saved
  }

  func seal(
    message: String, recipientPublicKey: Data, decoy: String?, passphrase: String?,
    carrier: CarrierKind
  ) throws
    -> SealedCarrier
  {
    let result = try encrypt(
      request: EncryptRequest(
        message: message,
        decoy: decoy,
        passphrase: passphrase,
        recipientPublicKey: recipientPublicKey,
        algorithm: .fake,
        carrier: carrier,
        options: CarrierOptions()
      ))
    return SealedCarrier(bytes: result.bytes, fileExtension: result.`extension`)
  }

  func open(file: Data, passphrase: String?) throws -> String {
    let identity = try demoIdentity()
    let result = try decrypt(
      request: DecryptRequest(
        file: file,
        mySecretKey: identity.secretKey,
        myPublicKey: identity.publicKey,
        passphrase: passphrase
      ))
    // Slot is intentionally not returned to the interface.
    return result.text
  }
}

/// Stores the demo identity as one Keychain item; secret key bytes never go to disk.
private final class KeychainIdentityStore: @unchecked Sendable {
  private let serviceName = "me.polyphonia.demo-identity"
  private let accountName = "fake-v1"

  func load() throws -> DemoIdentity? {
    var query = baseQuery
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne

    var result: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    if status == errSecItemNotFound { return nil }
    guard status == errSecSuccess, let data = result as? Data else {
      throw KeychainError.operationFailed(status)
    }
    return try PropertyListDecoder().decode(StoredIdentity.self, from: data).identity
  }

  func save(_ identity: DemoIdentity) throws {
    let data = try PropertyListEncoder().encode(StoredIdentity(identity: identity))
    var query = baseQuery
    query[kSecValueData as String] = data
    query[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    let status = SecItemAdd(query as CFDictionary, nil)
    guard status == errSecSuccess else { throw KeychainError.operationFailed(status) }
  }

  private var baseQuery: [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: serviceName,
      kSecAttrAccount as String: accountName,
    ]
  }
}

private struct StoredIdentity: Codable {
  let publicKey: Data
  let secretKey: Data

  init(identity: DemoIdentity) {
    publicKey = identity.publicKey
    secretKey = identity.secretKey
  }

  var identity: DemoIdentity { DemoIdentity(publicKey: publicKey, secretKey: secretKey) }
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
