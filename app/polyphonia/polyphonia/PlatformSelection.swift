import AppKit
import Combine

/// A fresh selection for each invocation. Destination and mode are never persisted.
@MainActor
final class PlatformSelection: ObservableObject {
  @Published private(set) var platformID = PlatformProfiles.otherID
  @Published private(set) var imageMode: ImageMode?
  @Published private(set) var wasDetected = false
  let profiles: PlatformProfiles?
  let loadError: String?

  init(bundle: Bundle = .main) {
    do {
      profiles = try PlatformProfiles.load(bundle: bundle)
      loadError = nil
    } catch {
      profiles = nil
      loadError =
        "Platform profiles could not be loaded. Rebuild Polyphonia to include platforms.json."
    }
  }

  var platform: PlatformProfile? { profiles?.profile(id: platformID) }
  var platformName: String { platform?.name ?? "Other" }

  func beginSession(source: NSRunningApplication?) {
    let detected = PlatformDetector.platformID(for: source)
    selectPlatform(detected ?? PlatformProfiles.otherID)
    wasDetected = platform != nil && detected != nil
  }

  func selectPlatform(_ id: String) {
    platformID = profiles?.profile(id: id)?.id ?? PlatformProfiles.otherID
    imageMode = platform?.recommendedMode
    wasDetected = false
  }

  func selectMode(_ mode: ImageMode) { imageMode = mode }

  func guide(for mode: ImageMode) -> SendingGuide? {
    guard let profiles else { return nil }
    return SendingGuide(
      platformName: platformName, mode: mode,
      steps: platform?.steps[mode] ?? [profiles.other.explain[mode]],
      verification: platform?.verified, notes: platform?.notes,
      sources: platform?.sources ?? [])
  }
}
