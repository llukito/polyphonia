import Foundation

/// Stable names match SPEC §9 public-key headers and Keychain account names.
enum CryptoAlgorithm: String, Codable, CaseIterable, Sendable {
  case mlkem, rsa, fake

  static let secureAlgorithms: [Self] = [.mlkem, .rsa]

  var core: Algorithm {
    switch self {
    case .mlkem: .mlKem768
    case .rsa: .rsa2048
    case .fake: .fake
    }
  }

  init(_ algorithm: Algorithm) {
    switch algorithm {
    case .mlKem768: self = .mlkem
    case .rsa2048: self = .rsa
    case .fake: self = .fake
    }
  }

  var title: String {
    switch self {
    case .mlkem: "ML-KEM-768"
    case .rsa: "RSA-2048 · comparison"
    case .fake: "Legacy Fake · not secure"
    }
  }
}
