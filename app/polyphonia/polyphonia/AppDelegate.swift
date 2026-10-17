import AppKit
import Carbon.HIToolbox
import SwiftUI

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
  private var ring: RingWindowController?
  private var hotKeyHandler: EventHandlerRef?
  private var hotKey: EventHotKeyRef?
  private var isCapturingSelection = false

  func applicationDidFinishLaunching(_ notification: Notification) {
    // This is a keyboard companion, so it runs without a Dock icon.
    NSApp.setActivationPolicy(.accessory)
    ApplicationMenu.install()
    ring = RingWindowController()
    do {
      try registerGlobalShortcut()
    } catch {
      NSApp.activate(ignoringOtherApps: true)
      NSAlert(error: error).runModal()
      NSApp.terminate(nil)
    }
  }

  func applicationWillTerminate(_ notification: Notification) {
    ring?.closeImmediately()
    if let hotKey { UnregisterEventHotKey(hotKey) }
    if let hotKeyHandler { RemoveEventHandler(hotKeyHandler) }
  }

  private func registerGlobalShortcut() throws {
    var eventTypes = [
      EventTypeSpec(
        eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed)),
      EventTypeSpec(
        eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyReleased)),
    ]
    let callback: EventHandlerProcPtr = { _, event, context in
      guard let context, let event else { return noErr }
      let app = Unmanaged<AppDelegate>.fromOpaque(context).takeUnretainedValue()
      let released = GetEventKind(event) == UInt32(kEventHotKeyReleased)
      DispatchQueue.main.async {
        if released { app.ring?.releaseShortcut() } else { app.toggleRing() }
      }
      return noErr
    }
    let handlerStatus = InstallEventHandler(
      GetEventDispatcherTarget(), callback, eventTypes.count, &eventTypes,
      Unmanaged.passUnretained(self).toOpaque(), &hotKeyHandler)
    guard handlerStatus == noErr else {
      throw ShortcutError.registrationFailed(handlerStatus)
    }

    // Command + Shift + P is the demo shortcut and avoids requiring Input Monitoring.
    let identifier = EventHotKeyID(signature: HotKey.signature, id: HotKey.identifier)
    let shortcutStatus = RegisterEventHotKey(
      UInt32(kVK_ANSI_P), UInt32(cmdKey | shiftKey), identifier,
      GetEventDispatcherTarget(), HotKey.options, &hotKey)
    guard shortcutStatus == noErr else {
      if let hotKeyHandler { RemoveEventHandler(hotKeyHandler) }
      hotKeyHandler = nil
      throw ShortcutError.registrationFailed(shortcutStatus)
    }
  }

  private func toggleRing() {
    if ring?.isPresented == true {
      ring?.dismiss()
      return
    }
    guard !isCapturingSelection else { return }
    let source = NSWorkspace.shared.frontmostApplication
    isCapturingSelection = true
    Task {
      defer { isCapturingSelection = false }
      let selection = await SelectionReader.readSelectedText(from: source)
      ring?.show(selection: selection, sourceApplication: source)
    }
  }
}

private enum ShortcutError: LocalizedError {
  case registrationFailed(OSStatus)

  var errorDescription: String? {
    "Polyphonia could not enable Command–Shift–P."
  }

  var recoverySuggestion: String? {
    switch self {
    case .registrationFailed(let status):
      return "Quit other copies of Polyphonia and try again. macOS returned error \(status)."
    }
  }
}

private enum HotKey {
  // Four-character ASCII code “POLY” identifies this app’s Carbon hotkey registration.
  static let signature: OSType = 0x504F_4C59
  // The first and only global shortcut registered by the demo app.
  static let identifier: UInt32 = 1
  // Carbon requires zero when no registration options are requested.
  static let options: OptionBits = 0
}
