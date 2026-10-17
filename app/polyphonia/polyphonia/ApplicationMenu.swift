import AppKit

/// Standard responder commands keep paste and quit working in a storyboard-free app.
enum ApplicationMenu {
  @MainActor
  static func install() {
    let main = NSMenu()
    let applicationItem = NSMenuItem()
    let application = NSMenu(title: "Polyphonia")
    application.addItem(
      withTitle: "Quit Polyphonia", action: #selector(NSApplication.terminate(_:)),
      keyEquivalent: "q"
    )
    .target = NSApp
    applicationItem.submenu = application
    main.addItem(applicationItem)

    let editItem = NSMenuItem()
    let edit = NSMenu(title: "Edit")
    edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
    edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
    edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
    edit.addItem(
      withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
    editItem.submenu = edit
    main.addItem(editItem)
    NSApp.mainMenu = main
  }
}
