import AppKit
import ApplicationServices

/// Best-effort identification of the source, before the ring activates. No titles or URLs are saved.
@MainActor
enum PlatformDetector {
  static func platformID(for source: NSRunningApplication?) -> String? {
    guard let source else { return nil }
    if let bundleID = source.bundleIdentifier, let id = nativeBundleIDs[bundleID] { return id }
    if let name = source.localizedName?.lowercased(), let id = nativeNames[name] { return id }

    guard AXIsProcessTrusted(),
      NSWorkspace.shared.frontmostApplication?.processIdentifier == source.processIdentifier
    else { return nil }
    let application = AXUIElementCreateApplication(source.processIdentifier)
    guard let focused = attribute(kAXFocusedUIElementAttribute, of: application),
      CFGetTypeID(focused) == AXUIElementGetTypeID()
    else { return nil }
    var element = focused as! AXUIElement
    var focusedPID: pid_t = 0
    guard AXUIElementGetPid(element, &focusedPID) == .success,
      focusedPID == source.processIdentifier
    else { return nil }

    // Only inspect the focused ancestry, never other tabs, the address bar, or browser history.
    for _ in 0..<DetectionPolicy.maximumAncestors {
      if attribute(kAXRoleAttribute, of: element) as? String == DetectionPolicy.webAreaRole,
        let value = attribute(kAXURLAttribute, of: element),
        let url = (value as? URL) ?? (value as? String).flatMap(URL.init(string:)),
        let host = url.host?.lowercased()
      {
        return platformID(host: host)
      }
      guard let parent = attribute(kAXParentAttribute, of: element),
        CFGetTypeID(parent) == AXUIElementGetTypeID()
      else { break }
      element = (parent as! AXUIElement)
    }
    return nil
  }

  static func platformID(host: String) -> String? {
    webDomains.first { host == $0.domain || host.hasSuffix("." + $0.domain) }?.platform
  }

  private static func attribute(_ name: String, of element: AXUIElement) -> CFTypeRef? {
    AXUIElementSetMessagingTimeout(element, DetectionPolicy.messageTimeout)
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else {
      return nil
    }
    return value
  }

  // Native bundle identifiers take precedence over app display names.
  private static let nativeBundleIDs = [
    "ru.keepcoder.Telegram": "telegram", "org.telegram.desktop": "telegram",
    "net.whatsapp.WhatsApp": "whatsapp", "com.hnc.Discord": "discord",
    "org.whispersystems.signal-desktop": "signal", "com.apple.MobileSMS": "imessage",
    "com.apple.mail": "email", "com.tinyspeck.slackmacgap": "slack",
    "com.microsoft.teams2": "teams", "com.microsoft.teams": "teams",
    "com.viber.mac": "viber", "com.tencent.xinWeChat": "wechat",
  ]
  private static let nativeNames = [
    "telegram": "telegram", "telegram lite": "telegram", "whatsapp": "whatsapp",
    "discord": "discord", "signal": "signal", "messages": "imessage", "mail": "email",
    "slack": "slack", "microsoft teams": "teams", "viber": "viber", "wechat": "wechat",
  ]
  // Match complete domain boundaries so an unrelated lookalike host cannot select a profile.
  private static let webDomains: [(domain: String, platform: String)] = [
    ("telegram.org", "telegram"), ("whatsapp.com", "whatsapp"),
    ("discord.com", "discord"), ("discordapp.com", "discord"),
    ("instagram.com", "instagram"), ("messenger.com", "messenger"),
    ("facebook.com", "messenger"), ("slack.com", "slack"),
    ("teams.microsoft.com", "teams"), ("teams.live.com", "teams"),
    ("chat.google.com", "google-chat"), ("mail.google.com", "email"),
    ("outlook.live.com", "email"), ("outlook.office.com", "email"),
    ("linkedin.com", "linkedin"), ("snapchat.com", "snapchat"),
    ("x.com", "x"), ("twitter.com", "x"), ("wechat.com", "wechat"),
  ]
}

private enum DetectionPolicy {
  // WebKit/Chromium expose this AX role on macOS versions predating AppKit's public webArea constant.
  static let webAreaRole = "AXWebArea"
  // Stop at the enclosing web area/application; do not scan the whole accessibility tree.
  static let maximumAncestors = 8
  // Bound each AX request against an unresponsive browser.
  static let messageTimeout: Float = 0.15
}
