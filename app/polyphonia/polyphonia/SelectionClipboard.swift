import AppKit
import Carbon.HIToolbox

/// Compatibility path for editors that implement Copy but omit accessibility selection attributes.
@MainActor
enum SelectionClipboard {
  static func capture(from source: NSRunningApplication) async -> SelectionCapture {
    // The user may still be holding Shift from the trigger. Wait so the editor receives plain ⌘C.
    for _ in 0..<CopyTiming.modifierPolls {
      guard SelectionReader.isFrontmost(source) else { return .unavailable(.sourceChanged) }
      if CGEventSource.flagsState(.combinedSessionState).intersection(CopyTiming.modifiers).isEmpty
      {
        return await copySelection(from: source)
      }
      try? await Task.sleep(for: CopyTiming.pollInterval)
      if Task.isCancelled { return .unavailable(.sourceChanged) }
    }
    return .unavailable(.shortcutStillHeld)
  }

  private static func copySelection(from source: NSRunningApplication) async -> SelectionCapture {
    guard SelectionReader.isFrontmost(source) else { return .unavailable(.sourceChanged) }
    let pasteboard = NSPasteboard.general
    guard let savedClipboard = SelectionClipboardSnapshot(pasteboard: pasteboard) else {
      return .unavailable(.copyUnavailable(app: source.localizedName ?? "This app"))
    }
    guard
      let down = CGEvent(
        keyboardEventSource: nil, virtualKey: CGKeyCode(kVK_ANSI_C), keyDown: true),
      let up = CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(kVK_ANSI_C), keyDown: false)
    else { return .unavailable(.copyUnavailable(app: source.localizedName ?? "This app")) }

    // An empty board ensures that an editor ignoring Copy can never return old clipboard text.
    var ownedChange = pasteboard.clearContents()
    defer {
      savedClipboard.restore(to: pasteboard, ifUnchanged: ownedChange)
    }

    down.flags = .maskCommand
    up.flags = .maskCommand
    down.postToPid(source.processIdentifier)
    up.postToPid(source.processIdentifier)

    for _ in 0..<CopyTiming.clipboardPolls {
      try? await Task.sleep(for: CopyTiming.pollInterval)
      guard !Task.isCancelled, SelectionReader.isFrontmost(source) else {
        return .unavailable(.sourceChanged)
      }
      if pasteboard.changeCount != ownedChange {
        let copiedChange = pasteboard.changeCount
        let text = pasteboard.string(forType: .string)
        // A provider may change the board while materializing its data; leave that newer write alone.
        guard pasteboard.changeCount == copiedChange else { break }
        ownedChange = copiedChange
        if let text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
          return .selected(text)
        }
        break
      }
    }
    return .unavailable(.copyUnavailable(app: source.localizedName ?? "This app"))
  }

}

/// Materialize every representation before Copy, including file URLs and rich text.
@MainActor
struct SelectionClipboardSnapshot {
  private let items: [NSPasteboardItem]

  init?(pasteboard: NSPasteboard) {
    let originalChange = pasteboard.changeCount
    var items: [NSPasteboardItem] = []
    for original in pasteboard.pasteboardItems ?? [] {
      let saved = NSPasteboardItem()
      for type in original.types {
        // Abort rather than discard a representation that cannot be safely backed up.
        guard let data = original.data(forType: type) else { return nil }
        saved.setData(data, forType: type)
      }
      items.append(saved)
    }
    guard pasteboard.changeCount == originalChange else { return nil }
    self.items = items
  }

  func restore(to pasteboard: NSPasteboard, ifUnchanged expectedChange: Int) {
    // Leave any write made after the captured response alone.
    guard pasteboard.changeCount == expectedChange else { return }
    pasteboard.clearContents()
    if !items.isEmpty { pasteboard.writeObjects(items) }
  }
}

private enum CopyTiming {
  // Poll without blocking the main run loop, allowing the source app to service Copy.
  static let pollInterval: Duration = .milliseconds(25)
  // Allow one second for the user to release the trigger keys: 40 × 25 ms.
  static let modifierPolls = 40
  // Allow 600 ms for the source app's clipboard response: 24 × 25 ms.
  static let clipboardPolls = 24
  // Any held shortcut modifier could change the meaning of the synthetic Copy command.
  static let modifiers: CGEventFlags = [.maskCommand, .maskShift, .maskControl, .maskAlternate]
}
