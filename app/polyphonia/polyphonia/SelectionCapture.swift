import Foundation

enum SelectionCapture {
  case selected(String)
  case unavailable(SelectionIssue)

  var text: String? {
    if case .selected(let text) = self { return text }
    return nil
  }

  var issue: SelectionIssue? {
    if case .unavailable(let issue) = self { return issue }
    return nil
  }
}

enum SelectionIssue {
  case permissionRequired
  case noSourceApplication
  case sourceChanged
  case secureField
  case shortcutStillHeld
  case copyUnavailable(app: String)

  var needsPermission: Bool {
    if case .permissionRequired = self { return true }
    return false
  }

  var message: String {
    switch self {
    case .permissionRequired:
      "macOS has not granted this running copy Accessibility access. Enable Polyphonia in Accessibility, then quit and run it again."
    case .noSourceApplication:
      "Select text in another app, then press ⌘⇧P."
    case .sourceChanged:
      "The active app changed before capture finished. Select the text and try again."
    case .secureField:
      "Password fields cannot be captured. Select text in a regular message field."
    case .shortcutStillHeld:
      "Release ⌘⇧P after pressing it so Polyphonia can copy the selection."
    case .copyUnavailable(let app):
      "\(app) did not provide selected text through Accessibility or Copy. Keep the text highlighted and try again."
    }
  }
}
