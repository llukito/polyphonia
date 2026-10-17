import AppKit

@main
enum PolyphoniaApplication {
  @MainActor
  static func main() {
    let application = NSApplication.shared
    let delegate = AppDelegate()
    // The storyboard used to make this connection. Our floating UI starts in code.
    application.delegate = delegate
    withExtendedLifetime(delegate) {
      application.run()
    }
  }
}
