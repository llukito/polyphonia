import AppKit
import Carbon.HIToolbox
import Combine
import SwiftUI
import UniformTypeIdentifiers

@MainActor
final class RingPresentation: ObservableObject {
  @Published var isPresented = false
  @Published var releaseRequest: RingReleaseRequest?
  // The initial toolbar, one gap, and demo caption; the view supplies its measured size.
  @Published var accessoryHeight = RingLayout.fieldHeight + RingLayout.gap + RingLayout.captionSize
}

/// Owns the native overlay and its dismissal. The view only owns the ring's appearance.
@MainActor
final class RingWindowController {
  private let window: RingPanel
  private let session = MessageSession(service: PolyphoniaService())
  private let presentation = RingPresentation()
  private var keyboardMonitor: Any?
  private var outsideClickMonitor: Any?
  private var deactivationObserver: NSObjectProtocol?
  private var sourceApplication: NSRunningApplication?
  private var transitionID = UUID()
  private var anchor: CGPoint?
  private var geometrySubscription: AnyCancellable?
  private var filePicker: NSOpenPanel?

  var isPresented: Bool { presentation.isPresented }

  init() {
    window = RingPanel(
      contentRect: NSRect(origin: .zero, size: RingLayout.canvasSize),
      styleMask: [.borderless, .fullSizeContentView],
      backing: .buffered, defer: false)
    window.isFloatingPanel = true
    window.hidesOnDeactivate = false
    window.level = .floating
    window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
    window.backgroundColor = .clear
    window.isOpaque = false
    window.hasShadow = false
    window.isReleasedWhenClosed = false
    window.acceptsMouseMovedEvents = true
    window.animationBehavior = .none
    window.onDismiss = { [weak self] in self?.dismiss() }
    window.contentView = NSHostingView(
      rootView: RingOverlayView(
        session: session, presentation: presentation,
        onDismiss: { [weak self] in self?.dismiss() },
        onQuit: { NSApp.terminate(nil) },
        onBrowse: { [weak self] in self?.browseReceivedFile() },
        onImportContact: { [weak self] in self?.browsePublicKey() },
        onChooseCover: { [weak self] in self?.browseImageCover() },
        onChooseAudioCover: { [weak self] in self?.browseAudioCover() }))
    geometrySubscription = presentation.$accessoryHeight.removeDuplicates().sink {
      [weak self] height in
      self?.positionAtAnchor(accessoryHeight: height)
    }
  }

  func show(selection: SelectionCapture, sourceApplication: NSRunningApplication?) {
    transitionID = UUID()
    self.sourceApplication = sourceApplication
    presentation.releaseRequest = nil
    session.beginSession(with: selection, sourceApplication: sourceApplication)
    anchor = NSEvent.mouseLocation
    positionAtAnchor(accessoryHeight: presentation.accessoryHeight)
    installMonitors()
    NSApp.activate(ignoringOtherApps: true)
    window.makeKeyAndOrderFront(nil)

    // Let the hosting view mount at its contracted size before the spring starts.
    DispatchQueue.main.async { [weak self] in
      guard let self, self.window.isVisible else { return }
      withAnimation(self.presentationAnimation) {
        self.presentation.isPresented = true
      }
    }
  }

  func dismiss(restoreFocus: Bool = true) {
    if let filePicker {
      filePicker.cancel(nil)
      return
    }
    guard window.isVisible else { return }
    removeMonitors()
    let closingTransition = UUID()
    transitionID = closingTransition
    withAnimation(RingMotion.dismiss) {
      presentation.isPresented = false
    } completion: { [weak self] in
      guard let self, self.transitionID == closingTransition else { return }
      let shouldRestoreFocus = restoreFocus && NSApp.isActive
      self.window.orderOut(nil)
      if shouldRestoreFocus { self.sourceApplication?.activate() }
    }
  }

  func closeImmediately() {
    let pickerToCancel = filePicker
    filePicker = nil
    pickerToCancel?.cancel(nil)
    transitionID = UUID()
    removeMonitors()
    presentation.isPresented = false
    window.orderOut(nil)
  }

  private func browseReceivedFile() {
    chooseFile(title: "Open a received Polyphonia file", prompt: "Open", types: [.item]) {
      [weak self] url in
      self?.session.open(fileURL: url)
    }
  }

  private func browsePublicKey() {
    chooseFile(title: "Import a contact public key", prompt: "Import", types: [.data]) {
      [weak self] url in
      self?.session.importContact(fileURL: url)
    }
  }

  private func browseImageCover() {
    chooseFile(title: "Choose a cover image", prompt: "Use image", types: [.png, .jpeg]) {
      [weak self] url in
      self?.session.selectImageCover(fileURL: url)
    }
  }

  private func browseAudioCover() {
    chooseFile(title: "Choose a cover WAV", prompt: "Use audio", types: [.wav]) {
      [weak self] url in
      self?.session.selectAudioCover(fileURL: url)
    }
  }

  private func chooseFile(
    title: String, prompt: String, types: [UTType], onSelection: @escaping (URL) -> Void
  ) {
    guard filePicker == nil, !session.isWorking else { return }
    let panel = NSOpenPanel()
    panel.title = title
    panel.prompt = prompt
    panel.canChooseFiles = true
    panel.canChooseDirectories = false
    panel.allowsMultipleSelection = false
    panel.allowedContentTypes = types
    panel.level = .modalPanel
    filePicker = panel

    // The file picker owns focus until it finishes; the floating ring must not cover or dismiss it.
    removeMonitors()
    window.orderOut(nil)
    NSApp.activate(ignoringOtherApps: true)
    panel.begin { [weak self, weak panel] response in
      guard let self, let panel, self.filePicker === panel else { return }
      self.filePicker = nil
      NSApp.activate(ignoringOtherApps: true)
      self.window.makeKeyAndOrderFront(nil)
      self.installMonitors()
      if response == .OK, let url = panel.url { onSelection(url) }
    }
  }

  func releaseShortcut() {
    guard presentation.isPresented, window.isVisible, NSApp.isActive else { return }
    let mouse = NSEvent.mouseLocation
    // AppKit screen coordinates grow upward; SwiftUI's ring coordinates grow downward.
    let point = CGPoint(
      x: mouse.x - window.frame.minX,
      y: RingLayout.canvasHeight - (mouse.y - window.frame.minY))
    let halfInteraction = RingLayout.interactionDiameter / 2
    presentation.releaseRequest = RingReleaseRequest(
      location: CGPoint(
        x: point.x - RingLayout.center.x + halfInteraction,
        y: point.y - RingLayout.center.y + halfInteraction))
  }

  private var presentationAnimation: Animation {
    NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
      ? RingMotion.reduced : RingMotion.appear
  }

  private func positionAtAnchor(accessoryHeight: CGFloat) {
    guard let anchor else { return }
    let screen = NSScreen.screens.first(where: { $0.frame.contains(anchor) }) ?? NSScreen.main
    guard let bounds = screen?.visibleFrame else { return }
    let frame = RingLayout.frame(around: anchor, inside: bounds, accessoryHeight: accessoryHeight)
    let animate =
      presentation.isPresented && !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    window.setFrame(frame, display: true, animate: animate)
  }

  private func installMonitors() {
    removeMonitors()
    keyboardMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
      guard let self, self.window.isKeyWindow else { return event }
      let commandPressed = event.modifierFlags.contains(.command)
      if event.keyCode == UInt16(kVK_Escape)
        || (commandPressed && event.keyCode == UInt16(kVK_ANSI_W))
      {
        self.dismiss()
        return nil
      }
      if commandPressed && event.keyCode == UInt16(kVK_ANSI_Q) {
        NSApp.terminate(nil)
        return nil
      }
      return event
    }
    outsideClickMonitor = NSEvent.addGlobalMonitorForEvents(matching: [
      .leftMouseDown, .rightMouseDown,
    ]) {
      [weak self] _ in
      self?.dismiss(restoreFocus: false)
    }
    deactivationObserver = NotificationCenter.default.addObserver(
      forName: NSApplication.didResignActiveNotification, object: NSApp, queue: .main
    ) { [weak self] _ in
      MainActor.assumeIsolated { self?.dismiss(restoreFocus: false) }
    }
  }

  private func removeMonitors() {
    if let keyboardMonitor { NSEvent.removeMonitor(keyboardMonitor) }
    if let outsideClickMonitor { NSEvent.removeMonitor(outsideClickMonitor) }
    if let deactivationObserver { NotificationCenter.default.removeObserver(deactivationObserver) }
    keyboardMonitor = nil
    outsideClickMonitor = nil
    deactivationObserver = nil
  }
}

private final class RingPanel: NSPanel {
  var onDismiss: (() -> Void)?
  override var canBecomeKey: Bool { true }
  override var canBecomeMain: Bool { false }
  override func cancelOperation(_ sender: Any?) { onDismiss?() }
  override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect {
    frameRect
  }
  override func animationResizeTime(_ newFrame: NSRect) -> TimeInterval {
    RingMotion.windowAdjustmentDuration
  }
}
