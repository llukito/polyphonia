import AppKit
import SwiftUI

/// Native hit testing keeps transparent gaps scrollable while preserving the controls above them.
struct AccessoryScrollView<Content: View>: NSViewRepresentable {
  let contentWidth: CGFloat
  let onContentHeightChange: (CGFloat) -> Void
  @ViewBuilder var content: () -> Content

  func makeNSView(context: Context) -> AccessoryNativeScrollView {
    let view = AccessoryNativeScrollView()
    updateContent(in: view)
    return view
  }

  func updateNSView(_ view: AccessoryNativeScrollView, context: Context) {
    updateContent(in: view)
  }

  private func updateContent(in view: AccessoryNativeScrollView) {
    view.onContentHeightChange = onContentHeightChange
    view.hostingView.rootView = AnyView(
      content()
        .frame(width: contentWidth)
        .fixedSize(horizontal: false, vertical: true)
        .background {
          GeometryReader { geometry in
            Color.clear.preference(key: AccessoryContentHeight.self, value: geometry.size.height)
          }
        }
        .onPreferenceChange(AccessoryContentHeight.self) { [weak view] height in
          guard let view, height.isFinite, height > 0 else { return }
          view.setDocumentSize(width: contentWidth, height: height)
        }
        .preferredColorScheme(.dark))
    // Bootstrap the document before its first geometry preference can be delivered.
    view.setDocumentSize(width: contentWidth, height: view.hostingView.fittingSize.height)
  }
}

final class AccessoryNativeScrollView: NSScrollView {
  let hostingView = AccessoryHostingView(rootView: AnyView(EmptyView()))
  var onContentHeightChange: ((CGFloat) -> Void)?
  private var reportedHeight: CGFloat?

  override var isOpaque: Bool { false }

  init() {
    super.init(frame: .zero)
    drawsBackground = false
    backgroundColor = .clear
    contentView.drawsBackground = false
    hasVerticalScroller = true
    hasHorizontalScroller = false
    autohidesScrollers = true
    scrollerStyle = .overlay
    borderType = .noBorder
    // A top-origin document keeps the first controls visible when its height changes.
    hostingView.isFlipped = true
    documentView = hostingView
  }

  required init?(coder: NSCoder) { fatalError("Use init()") }

  func setDocumentSize(width: CGFloat, height: CGFloat) {
    guard height.isFinite, height > 0 else { return }
    let size = NSSize(width: width, height: height)
    if hostingView.frame.size != size {
      hostingView.setFrameSize(size)
      let origin = contentView.constrainBoundsRect(contentView.bounds).origin
      contentView.scroll(to: origin)
      reflectScrolledClipView(contentView)
    }
    guard reportedHeight != height else { return }
    reportedHeight = height
    DispatchQueue.main.async { [weak self] in self?.onContentHeightChange?(height) }
  }

  override func hitTest(_ point: NSPoint) -> NSView? {
    guard !isHidden, alphaValue > 0, bounds.contains(convert(point, from: superview)) else {
      return nil
    }
    return super.hitTest(point) ?? self
  }
}

final class AccessoryHostingView: NSHostingView<AnyView> {
  override func hitTest(_ point: NSPoint) -> NSView? {
    guard !isHidden, alphaValue > 0, bounds.contains(convert(point, from: superview)) else {
      return nil
    }
    return super.hitTest(point) ?? self
  }

  override func scrollWheel(with event: NSEvent) {
    if let scrollView = enclosingScrollView {
      scrollView.scrollWheel(with: event)
    } else {
      super.scrollWheel(with: event)
    }
  }
}

private struct AccessoryContentHeight: PreferenceKey {
  static let defaultValue: CGFloat = .zero
  static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = nextValue() }
}
