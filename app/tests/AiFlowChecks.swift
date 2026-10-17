import AppKit
import Foundation
import SwiftUI

private enum CheckError: Error { case failed(String) }

@MainActor
private final class FlowPreview {
  private let host: NSHostingView<RingOverlayView>
  private let window: NSWindow

  init(session: MessageSession) {
    NSApplication.shared.setActivationPolicy(.accessory)
    let presentation = RingPresentation()
    presentation.isPresented = true
    host = NSHostingView(
      rootView: RingOverlayView(
        session: session, presentation: presentation, onDismiss: {}, onQuit: {},
        onBrowse: {}, onImportContact: {}, onChooseCover: {}, onChooseAudioCover: {}))
    host.frame = NSRect(origin: .zero, size: RingLayout.canvasSize)
    window = NSWindow(
      contentRect: host.frame, styleMask: [.titled, .closable], backing: .buffered, defer: false)
    window.title = "Polyphonia controls check"
    window.backgroundColor = NSColor(white: 0.12, alpha: 1)
    window.contentView = host
    window.makeKeyAndOrderFront(nil)
    NSApplication.shared.activate(ignoringOtherApps: true)
  }

  func capture(_ name: String) async throws {
    try await processEvents(for: .milliseconds(450))
    host.layoutSubtreeIfNeeded()
    let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds)!
    host.cacheDisplay(in: host.bounds, to: bitmap)
    try bitmap.representation(using: .png, properties: [:])!
      .write(to: URL(fileURLWithPath: "/private/tmp/polyphonia-restored-\(name).png"))
  }

  private func processEvents(for duration: Duration) async throws {
    try await Task.sleep(for: duration)
  }

  func press(_ label: String) throws {
    func find(_ element: Any, depth: Int = 0) -> AnyObject? {
      guard depth < 32 else { return nil }
      let node = element as AnyObject
      if node.accessibilityRole?() == .button,
        node.accessibilityLabel?() == label || node.accessibilityTitle?() == label
      {
        return node
      }
      for child in node.accessibilityChildren?() ?? [] {
        if let match = find(child, depth: depth + 1) { return match }
      }
      return nil
    }
    guard let button = find(host), button.accessibilityPerformPress?() == true else {
      throw CheckError.failed("Could not press inline review button: \(label)")
    }
  }

  func enableDecoy() throws {
    func find(_ view: NSView) -> NSSwitch? {
      if let control = view as? NSSwitch { return control }
      return view.subviews.lazy.compactMap { find($0) }.first
    }
    guard let control = find(host) else { throw CheckError.failed("Missing Decoy switch") }
    control.performClick(nil)
  }

  func checkPanelScrolling() async throws {
    func panel(in view: NSView) -> AccessoryNativeScrollView? {
      if let match = view as? AccessoryNativeScrollView { return match }
      return view.subviews.lazy.compactMap { panel(in: $0) }.first
    }
    guard let scrollView = panel(in: host) else {
      throw CheckError.failed("Missing accessory panel")
    }
    let originalMouse = NSEvent.mouseLocation
    let desktopTop = NSScreen.screens.first!.frame.maxY
    defer {
      CGWarpMouseCursorPosition(CGPoint(x: originalMouse.x, y: desktopTop - originalMouse.y))
    }
    func move(to point: NSPoint) -> CGPoint {
      let screen = window.convertPoint(toScreen: scrollView.hostingView.convert(point, to: nil))
      let target = CGPoint(x: screen.x, y: desktopTop - screen.y)
      CGWarpMouseCursorPosition(target)
      CGEvent(
        mouseEventSource: nil, mouseType: .mouseMoved,
        mouseCursorPosition: target, mouseButton: .left)?.post(tap: .cghidEventTap)
      return target
    }
    // All probes are outside inputs: the panel gutter, the gap after its toolbar, and a heading.
    let headingY =
      RingLayout.padding + RingLayout.fieldHeight + RingLayout.gap
      + RingLayout.captionSize / 2
    let probes: [(String, NSPoint)] = [
      ("panel padding", NSPoint(x: RingLayout.padding / 2, y: headingY)),
      (
        "gap between controls",
        NSPoint(
          x: RingLayout.accessoryPanelWidth / 2,
          y: RingLayout.padding + RingLayout.fieldHeight + RingLayout.gap / 2)
      ),
      ("non-input heading", NSPoint(x: RingLayout.padding * 2, y: headingY)),
    ]
    // Let moving away from Seal settle its expansion before measuring the native viewport.
    _ = move(to: probes[0].1)
    try await processEvents(for: .milliseconds(400))
    for (name, point) in probes {
      scrollView.contentView.scroll(to: .zero)
      scrollView.reflectScrolledClipView(scrollView.contentView)
      let target = move(to: point)
      try await processEvents(for: .milliseconds(100))
      let before = scrollView.contentView.bounds.minY
      let wheel = CGEvent(
        scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1,
        wheel1: -80, wheel2: 0, wheel3: 0)!
      wheel.location = target
      wheel.post(tap: .cghidEventTap)
      try await processEvents(for: .milliseconds(500))
      guard scrollView.contentView.bounds.minY > before else {
        throw CheckError.failed("Panel did not scroll over \(name)")
      }
      print("PASS wheel scrolling over \(name)")
    }
    scrollView.contentView.scroll(to: .zero)
    scrollView.reflectScrolledClipView(scrollView.contentView)
  }

  func close() { window.orderOut(nil) }
}

/// Deterministic AI responses, with real Rust carrier encoding and decoding.
private final class FlowBackend: PolyphoniaBackend, @unchecked Sendable {
  let crypto: PolyphoniaService
  let delay: TimeInterval
  let reject: Bool
  private let lock = NSLock()
  private var outputURL: URL?
  var lastFileURL: URL? { lock.withLock { outputURL } }
  let shorter = "Meet Alex at 17:00 by the station."
  let decoy = "Lunch plans are unchanged today."
  let server = AiServer(kind: .ollama, baseUrl: "http://127.0.0.1:11434/v1", models: ["test-model"])

  init(delay: TimeInterval = 0, reject: Bool = false) throws {
    crypto = PolyphoniaService(vault: MemorySecretVault())
    self.delay = delay
    self.reject = reject
  }

  func identity(algorithm: CryptoAlgorithm) throws -> AppIdentity {
    try crypto.identity(algorithm: algorithm)
  }
  func detectAiServers() -> [AiServer] { [server] }
  func shorten(message: String, server: AiServer, model: String) throws -> AiShortenSuggestion {
    Thread.sleep(forTimeInterval: delay)
    if reject { throw AiFfiError.Rejected(reasons: ["test rejection"]) }
    return AiShortenSuggestion(
      text: shorter, originalBytes: UInt64(message.utf8.count),
      newBytes: UInt64(shorter.utf8.count), savedPercent: 30)
  }
  func generateDecoy(message: String, server: AiServer, model: String) -> AiDecoySuggestion {
    Thread.sleep(forTimeInterval: delay)
    return AiDecoySuggestion(
      text: decoy, fits: true, minBytes: UInt64(decoy.utf8.count),
      maxBytes: UInt64(decoy.utf8.count))
  }
  func seal(
    message: String, recipientPublicKey: Data, decoy: String?, passphrase: String?,
    algorithm: CryptoAlgorithm, compact: Bool,
    carrier: CarrierKind, coverImage: Data?, coverAudio: Data?
  ) throws -> SealedCarrier {
    let result = try crypto.seal(
      message: message, recipientPublicKey: recipientPublicKey, decoy: decoy,
      passphrase: passphrase, algorithm: algorithm, compact: compact, carrier: carrier,
      coverImage: coverImage, coverAudio: coverAudio)
    lock.withLock { outputURL = result.fileURL }
    return result
  }
  func open(file: Data, passphrase: String?) throws -> OpenedMessage {
    try crypto.open(file: file, passphrase: passphrase)
  }
}

@main
@MainActor
struct AiFlowChecks {
  static let source = "Please meet Alex at 17:00 by the north entrance of the station today."
  // Poll main-actor state while Rust and simulated model requests execute off the UI thread.
  static let pollInterval: Duration = .milliseconds(20)
  static let timeout: Duration = .seconds(30)

  static func require(_ condition: Bool, _ description: String) throws {
    if !condition { throw CheckError.failed(description) }
  }

  static func waitUntil(_ description: String, _ predicate: () -> Bool) async throws {
    let clock = ContinuousClock()
    let deadline = clock.now.advanced(by: timeout)
    while !predicate() {
      guard clock.now < deadline else { throw CheckError.failed(description) }
      try await Task.sleep(for: pollInterval)
    }
  }

  private static func session(_ backend: FlowBackend, defaults: UserDefaults) async throws
    -> MessageSession
  {
    let session = MessageSession(
      service: backend, defaults: defaults, pasteboard: NSPasteboard.withUniqueName())
    session.message = source
    session.platformSelection.selectMode(.stealth)
    session.discoverAiServers()
    try await waitUntil("AI discovery did not finish") {
      session.selectedAiModel != nil && session.selectedContact != nil
    }
    return session
  }

  static func main() {
    setbuf(stdout, nil)
    let application = NSApplication.shared
    application.setActivationPolicy(.prohibited)
    Task { @MainActor in
      do {
        try await Task.detached { try CryptoFlowChecks.run() }.value
        try await runChecks()
        print("PASS all AI flow checks")
        exit(0)
      } catch {
        print("FAIL", error)
        exit(1)
      }
    }
    // A real AppKit event loop is required for posted mouse and wheel events in --ui checks.
    application.run()
  }

  private static func runChecks() async throws {
    let suite = "me.polyphonia.ai-flow-tests.\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: suite)!
    defer { defaults.removePersistentDomain(forName: suite) }
    let backend = try FlowBackend()

    let shortened = try await session(backend, defaults: defaults)
    shortened.requestShortening()
    try await waitUntil("Shortening did not finish") { !shortened.isAiWorking }
    let review = try unwrap(shortened.aiReview, "Missing shortening review")
    try require(shortened.message == source, "Suggestion changed the draft before approval")
    shortened.sealAndCopy(as: .image)
    try require(!shortened.isWorking, "Sealing ignored an unreviewed AI suggestion")
    try require(!shortened.applyAiReview(review, text: ""), "Empty suggestion was accepted")
    // Edit the model's suggestion before accepting it; sealing must use the edited version.
    let accepted = "Meet Alex at 17:00 at the north entrance."
    try require(
      shortened.applyAiReview(review, text: accepted), "Could not apply edited suggestion")
    try require(
      shortened.message == accepted && shortened.aiReview == nil, "Message was not updated")
    let file = try await seal(shortened, using: backend, as: .image)
    defer { try? FileManager.default.removeItem(at: file) }
    let decoded = try backend.open(file: Data(contentsOf: file), passphrase: nil)
    try require(decoded.text == accepted, "PNG did not contain the accepted AI text")
    print("PASS edited AI suggestion → message → PNG → opened text")

    let hidden = try await session(backend, defaults: defaults)
    hidden.requestDecoy()
    try await waitUntil("Decoy generation did not finish") { !hidden.isAiWorking }
    let decoyReview = try unwrap(hidden.aiReview, "Missing decoy review")
    try require(
      !hidden.applyAiReview(decoyReview, text: "Too short"), "Invalid decoy length was accepted")
    try require(hidden.applyAiReview(decoyReview, text: backend.decoy), "Could not apply decoy")
    try require(
      hidden.hiddenMode && hidden.decoy == backend.decoy, "Decoy was not wired to Hidden mode")
    hidden.sealAndCopy(as: .audio)
    try require(!hidden.isWorking, "Hidden mode allowed sealing without a passphrase")
    hidden.passphrase = "test-only-passphrase"
    // The prior PNG was opened above, establishing this one-way ML-KEM session.
    hidden.compactMessages = true
    let hiddenFile = try await seal(hidden, using: backend, as: .audio)
    defer { try? FileManager.default.removeItem(at: hiddenFile) }
    let bytes = try Data(contentsOf: hiddenFile)
    let ordinary = try backend.open(file: bytes, passphrase: nil)
    let secret = try backend.open(file: bytes, passphrase: "test-only-passphrase")
    try require(
      ordinary.text == backend.decoy && secret.text == source,
      "WAV did not preserve the applied decoy and real message")
    print("PASS AI decoy → Hidden mode → passphrase required → WAV → both messages")

    let discarded = try await session(backend, defaults: defaults)
    discarded.requestShortening()
    try await waitUntil("Discard test did not finish") { !discarded.isAiWorking }
    discarded.discardAiReview()
    try require(
      discarded.message == source && discarded.aiReview == nil, "Discard changed the draft")
    discarded.requestShortening()
    try await waitUntil("Stale review test did not finish") { !discarded.isAiWorking }
    let stale = try unwrap(discarded.aiReview, "Missing stale review")
    discarded.message = "A different message"
    try require(
      !discarded.applyAiReview(stale, text: backend.shorter), "Stale review overwrote a new draft")
    print("PASS discard and stale-review protection")

    // A delayed response must never apply after Cancel, navigation, or a draft edit.
    let delayed = try FlowBackend(delay: 0.15)
    let cancelled = try await session(delayed, defaults: defaults)
    cancelled.requestShortening()
    cancelled.cancelAiRequest()
    try await Task.sleep(for: .milliseconds(250))
    try require(
      cancelled.aiReview == nil && cancelled.message == source, "Cancelled response survived")
    cancelled.requestShortening()
    cancelled.show(.open)
    try await Task.sleep(for: .milliseconds(250))
    try require(
      cancelled.aiReview == nil && !cancelled.isAiWorking, "Response survived leaving Seal")
    cancelled.show(.home)
    cancelled.requestShortening()
    cancelled.message = "The message changed during generation"
    try await waitUntil("Edited-during-request test did not finish") { !cancelled.isAiWorking }
    try require(cancelled.aiReview == nil, "An outdated suggestion was shown")
    print("PASS cancellation, navigation, and draft edits during generation")

    let rejected = try await session(FlowBackend(reject: true), defaults: defaults)
    rejected.requestShortening()
    try await waitUntil("Rejection test did not finish") { !rejected.isAiWorking }
    try require(
      rejected.aiReview == nil && rejected.message == source && rejected.statusMessage != nil,
      "AI failure was not recoverable")
    print("PASS rejected suggestions leave the draft intact")

    if CommandLine.arguments.contains("--ui") {
      let draft = try await session(backend, defaults: defaults)
      let preview = FlowPreview(session: draft)
      defer { preview.close() }
      try await preview.capture("home")
      try await preview.checkPanelScrolling()
      draft.requestShortening()
      try await waitUntil("UI shortening did not finish") { !draft.isAiWorking }
      try await preview.capture("shorten-review")
      try preview.press("Use as message")
      try await waitUntil("Inline Use as message button did not apply the suggestion") {
        draft.message == backend.shorter && draft.aiReview == nil
      }
      try await preview.capture("shorten-applied")
      try preview.press("Decoy settings")
      try await preview.capture("decoy-settings")
      try preview.enableDecoy()
      try await waitUntil("Decoy toggle did not enable decoy mode") { draft.hiddenMode }
      try await preview.capture("decoy-fields")
      try preview.press("Generate decoy with local AI")
      try await waitUntil("UI decoy generation did not finish") { !draft.isAiWorking }
      try await preview.capture("decoy-review")
      try preview.press("Use as decoy")
      try await waitUntil("Inline Use as decoy button did not apply the suggestion") {
        draft.decoy == backend.decoy && draft.hiddenMode && draft.aiReview == nil
      }
      try await preview.capture("decoy-applied")
      print("PASS actual inline review buttons update the visible message and decoy")
    }
  }

  static func unwrap<T>(_ value: T?, _ description: String) throws -> T {
    guard let value else { throw CheckError.failed(description) }
    return value
  }

  private static func seal(
    _ session: MessageSession, using backend: FlowBackend, as medium: SealMedium
  ) async throws -> URL {
    // Observe a file generated by the real session, which uses an isolated test pasteboard.
    session.sealAndCopy(as: medium)
    try await waitUntil("Sealing did not finish") { !session.isWorking }
    try require(session.screen == .result, "Sealing failed")
    return try unwrap(backend.lastFileURL, "Sealing did not produce a file")
  }
}
