import AppKit
import SwiftUI

enum AccessoryTarget: Hashable { case message, hidden, aiTools, aiReview, status }

struct AccessoryScrollRequest {
  let id = UUID()
  let target: AccessoryTarget
}

private enum AccessoryDocumentSpace { static let name = "accessoryDocument" }

private struct AccessoryPositions: Equatable {
  var requestID: UUID?
  var frames: [AccessoryTarget: CGRect] = [:]
}

private struct AccessoryPositionsKey: PreferenceKey {
  static let defaultValue = AccessoryPositions()
  static func reduce(value: inout AccessoryPositions, nextValue: () -> AccessoryPositions) {
    value.frames.merge(nextValue().frames) { _, new in new }
  }
}

extension View {
  func accessoryTarget(_ target: AccessoryTarget) -> some View {
    background {
      GeometryReader { geometry in
        Color.clear.preference(
          key: AccessoryPositionsKey.self,
          value: AccessoryPositions(frames: [
            target: geometry.frame(in: .named(AccessoryDocumentSpace.name))
          ]))
      }
    }
  }
}

/// Native hit testing keeps transparent gaps scrollable while preserving the controls above them.
struct AccessoryScrollView<Content: View>: NSViewRepresentable {
  let contentWidth: CGFloat
  var scrollRequest: AccessoryScrollRequest? = nil
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
    view.scrollRequest = scrollRequest
    view.hostingView.rootView = AnyView(
      content()
        .frame(width: contentWidth)
        .fixedSize(horizontal: false, vertical: true)
        .coordinateSpace(name: AccessoryDocumentSpace.name)
        .background {
          GeometryReader { geometry in
            Color.clear.preference(key: AccessoryContentHeight.self, value: geometry.size.height)
          }
        }
        .onPreferenceChange(AccessoryContentHeight.self) { [weak view] height in
          guard let view, height.isFinite, height > 0 else { return }
          view.setDocumentSize(width: contentWidth, height: height)
        }
        .transformPreference(AccessoryPositionsKey.self) { positions in
          // Include the request ID so an unchanged target still receives a fresh scroll request.
          positions.requestID = scrollRequest?.id
        }
        .onPreferenceChange(AccessoryPositionsKey.self) { [weak view] positions in
          DispatchQueue.main.async {
            guard let view, let request = view.scrollRequest,
              positions.requestID == request.id, let frame = positions.frames[request.target]
            else { return }
            view.reveal(frame, requestID: request.id)
          }
        }
        .preferredColorScheme(.dark))
    // Bootstrap the document before its first geometry preference can be delivered.
    view.setDocumentSize(width: contentWidth, height: view.hostingView.fittingSize.height)
  }
}

final class AccessoryNativeScrollView: NSScrollView {
  let hostingView = AccessoryHostingView(rootView: AnyView(EmptyView()))
  var onContentHeightChange: ((CGFloat) -> Void)?
  var scrollRequest: AccessoryScrollRequest?
  private var completedScrollID: UUID?
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

  func reveal(_ frame: CGRect, requestID: UUID) {
    guard completedScrollID != requestID else { return }
    completedScrollID = requestID
    let maximumY = max(0, hostingView.frame.height - contentView.bounds.height)
    contentView.scroll(to: NSPoint(x: 0, y: min(max(0, frame.minY), maximumY)))
    reflectScrolledClipView(contentView)
  }

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
