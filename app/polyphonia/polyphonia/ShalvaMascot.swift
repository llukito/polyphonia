import AppKit
import SwiftUI

/// Displays Shalva's transparent animated GIF without flattening its animation.
struct ShalvaMascot: NSViewRepresentable {
  let animates: Bool

  func makeNSView(context: Context) -> NSImageView {
    let imageView = NSImageView()
    imageView.imageScaling = .scaleProportionallyUpOrDown
    imageView.animates = animates
    imageView.setAccessibilityLabel("Shalva, the dancing Georgian crab mascot")

    if let gifURL = Bundle.main.url(forResource: "shalva-transparent", withExtension: "gif"),
      let image = NSImage(contentsOf: gifURL)
    {
      imageView.image = image
    } else {
      imageView.image = NSImage(
        systemSymbolName: "waveform", accessibilityDescription: "Polyphonia")
      imageView.contentTintColor = .labelColor
    }

    return imageView
  }

  func updateNSView(_ imageView: NSImageView, context: Context) {
    imageView.animates = animates
  }
}
