import Foundation

enum ImageMode: String, Codable, CaseIterable, Identifiable, Sendable {
  case stealth, robust

  var id: String { rawValue }
  var title: String { self == .stealth ? "Stealth" : "Robust" }
  var carrier: CarrierKind { self == .stealth ? .stealthPng : .robustPng }
}

struct PlatformProfiles: Decodable, Sendable {
  let version: Int
  let platforms: [PlatformProfile]
  let other: OtherPlatform

  // Version 1 and the special Other entry are defined in docs/platform-tests.md.
  static let supportedVersion = 1
  static let otherID = "other"

  static func load(bundle: Bundle = .main) throws -> PlatformProfiles {
    guard let url = bundle.url(forResource: "platforms", withExtension: "json") else {
      throw ProfileError.missingResource
    }
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    let profiles = try decoder.decode(Self.self, from: Data(contentsOf: url))
    guard profiles.version == supportedVersion,
      profiles.other.defaultMode == nil,
      Set(profiles.platforms.map(\.id)).count == profiles.platforms.count,
      profiles.platforms.allSatisfy({ profile in
        profile.id != otherID && !profile.id.isEmpty
          && profile.defaultMode == (profile.losslessFileMode ? .stealth : .robust)
          && !profile.steps.stealth.isEmpty && !profile.steps.robust.isEmpty
      })
    else { throw ProfileError.invalidFormat }
    return profiles
  }

  func profile(id: String) -> PlatformProfile? { platforms.first { $0.id == id } }
}

struct PlatformProfile: Decodable, Identifiable, Sendable {
  let id: String
  let name: String
  let losslessFileMode: Bool
  let defaultMode: ImageMode
  let steps: ModeText<[String]>
  let notes: String
  let sources: [URL]
  let verified: Verification

  var recommendedMode: ImageMode { verified.status == .failed ? .robust : defaultMode }

  struct Verification: Decodable, Sendable {
    enum Status: String, Decodable, Sendable { case research, verified, failed }
    let status: Status
    let date: String?

    var caption: String {
      switch status {
      case .research: "Not yet tested"
      case .verified: date.map { "Profile tested · \($0)" } ?? "Profile tested"
      case .failed: "Previous transfer test failed"
      }
    }
  }
}

struct OtherPlatform: Decodable, Sendable {
  let defaultMode: ImageMode?
  let explain: ModeText<String>
}

struct ModeText<Value: Decodable & Sendable>: Decodable, Sendable {
  let stealth: Value
  let robust: Value

  subscript(mode: ImageMode) -> Value { mode == .stealth ? stealth : robust }
}

/// Capture the instructions with the file so later selector changes cannot change its sending steps.
struct SendingGuide: Identifiable, Sendable {
  let id = UUID()
  let platformName: String
  let mode: ImageMode
  let steps: [String]
  let verification: PlatformProfile.Verification?
  let notes: String?
  let sources: [URL]
}

private enum ProfileError: Error {
  case missingResource, invalidFormat
}
