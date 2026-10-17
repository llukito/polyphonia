import AppKit
import ApplicationServices

@MainActor
enum SelectionReader {
  static func readSelectedText(from source: NSRunningApplication?) async -> SelectionCapture {
    let promptKey = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
    guard AXIsProcessTrustedWithOptions([promptKey: true] as CFDictionary) else {
      return .unavailable(.permissionRequired)
    }
    guard let source, source.processIdentifier != ProcessInfo.processInfo.processIdentifier else {
      return .unavailable(.noSourceApplication)
    }
    guard isFrontmost(source) else { return .unavailable(.sourceChanged) }

    let application = AXUIElementCreateApplication(source.processIdentifier)
    AXUIElementSetMessagingTimeout(application, SelectionPolicy.messageTimeout)
    let focus =
      attribute(kAXFocusedUIElementAttribute, of: application)
      ?? attribute(kAXFocusedUIElementAttribute, of: AXUIElementCreateSystemWide())
    if let focus, CFGetTypeID(focus) == AXUIElementGetTypeID() {
      let focused = focus as! AXUIElement
      var focusedPID: pid_t = 0
      guard AXUIElementGetPid(focused, &focusedPID) == .success,
        focusedPID == source.processIdentifier
      else { return .unavailable(.sourceChanged) }
      if attribute(kAXSubroleAttribute, of: focused) as? String == kAXSecureTextFieldSubrole {
        return .unavailable(.secureField)
      }
      if let text = accessibleSelection(in: focused) { return .selected(text) }
    }

    // Custom web and Electron editors do not always expose AXSelectedText.
    // Copy runs in the original app, before the ring takes keyboard focus.
    return await SelectionClipboard.capture(from: source)
  }

  static func isFrontmost(_ source: NSRunningApplication) -> Bool {
    NSWorkspace.shared.frontmostApplication?.processIdentifier == source.processIdentifier
  }

  /// Accessibility offsets use UTF-16, as NSString does. Never use the full field as a selection.
  static func selectedSubstring(in value: String, range: CFRange) -> String? {
    let text = value as NSString
    guard range.location >= 0, range.length > 0,
      range.location <= text.length, range.length <= text.length - range.location
    else { return nil }
    return text.substring(with: NSRange(location: range.location, length: range.length))
  }

  private static func accessibleSelection(in focused: AXUIElement) -> String? {
    var candidate: AXUIElement? = focused
    for _ in 0..<SelectionPolicy.maximumSelectionAncestors {
      guard let current = candidate else { break }
      AXUIElementSetMessagingTimeout(current, SelectionPolicy.messageTimeout)
      if let text = nonemptyText(attribute(kAXSelectedTextAttribute, of: current)) { return text }

      // WebKit exposes some selections as opaque text markers rather than character ranges.
      if let marker = attribute(kAXSelectedTextMarkerRangeAttribute, of: current),
        let text = nonemptyText(
          parameter(
            kAXStringForTextMarkerRangeParameterizedAttribute,
            value: marker, of: current))
      {
        return text
      }

      if let rangeValue = attribute(kAXSelectedTextRangeAttribute, of: current),
        CFGetTypeID(rangeValue) == AXValueGetTypeID()
      {
        let accessibilityValue = rangeValue as! AXValue
        var range = CFRange()
        if AXValueGetType(accessibilityValue) == .cfRange,
          AXValueGetValue(accessibilityValue, .cfRange, &range), range.length > 0
        {
          if let text = nonemptyText(
            parameter(
              kAXStringForRangeParameterizedAttribute,
              value: rangeValue, of: current))
          {
            return text
          }
          if let value = attribute(kAXValueAttribute, of: current) as? String,
            let text = selectedSubstring(in: value, range: range),
            !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
          {
            return text
          }
        }
      }
      guard let parent = attribute(kAXParentAttribute, of: current),
        CFGetTypeID(parent) == AXUIElementGetTypeID()
      else { break }
      candidate = (parent as! AXUIElement)
    }
    return nil
  }

  private static func attribute(_ name: String, of element: AXUIElement) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else {
      return nil
    }
    return value
  }

  private static func parameter(_ name: String, value: CFTypeRef, of element: AXUIElement)
    -> CFTypeRef?
  {
    var result: CFTypeRef?
    guard
      AXUIElementCopyParameterizedAttributeValue(element, name as CFString, value, &result)
        == .success
    else { return nil }
    return result
  }

  private static func nonemptyText(_ value: CFTypeRef?) -> String? {
    guard let text = value as? String,
      !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    else { return nil }
    return text
  }
}

private enum SelectionPolicy {
  // Bound cross-process requests so an unresponsive editor cannot indefinitely block the ring.
  static let messageTimeout: Float = 0.25
  // Check the focused element and up to three enclosing editor/web containers.
  static let maximumSelectionAncestors = 4
}
