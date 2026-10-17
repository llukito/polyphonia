import AppKit
import Carbon.HIToolbox
import Combine
import SwiftUI

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
        onQuit: { NSApp.terminate(nil) }))
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
    transitionID = UUID()
    removeMonitors()
    presentation.isPresented = false
    window.orderOut(nil)
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
