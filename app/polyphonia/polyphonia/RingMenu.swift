import AppKit
import SwiftUI

/// Message handling stays in the overlay; this view owns the radial navigation gesture.
struct RingMenu<Hub: View>: View {
  let leading: RingAction
  let trailing: RingAction
  let isPresented: Bool
  let isWorking: Bool
  var releaseRequest: RingReleaseRequest? = nil
  var onExpansionChanged: (Bool) -> Void = { _ in }
  let onAction: (RingAction) -> Void
  @ViewBuilder var hub: (RingAction?) -> Hub

  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var interaction = RingInteraction()
  @State private var isDragging = false

  var body: some View {
    ZStack {
      if leading == .seal, interaction.isSealExpanded {
        ForEach(RingBranch.allCases, id: \.self) { branch in
          RingTile(
            action: branch.action, shape: RingBranchSector(branch: branch),
            diameter: RingLayout.interactionDiameter, labelOffset: branch.labelOffset,
            tint: RingColor.seal, isHighlighted: interaction.hoveredAction == branch.action,
            isWorking: isWorking, onAction: { onAction(branch.action) })
        }
        .transition(
          .scale(scale: RingLayout.collapsedScale, anchor: .trailing).combined(with: .opacity))
      }

      sector(action: leading, side: .left, tint: RingColor.seal)
      sector(action: trailing, side: .right, tint: RingColor.open)

      // One stable surface owns hover, clicks, and drags, including the hub and branch corridor.
      Color.clear
        .contentShape(Circle())
        .gesture(
          DragGesture(minimumDistance: 0, coordinateSpace: .local)
            .onChanged { value in
              guard isPresented, !isWorking else { return }
              isDragging = true
              interaction.update(at: value.location, leading: leading, trailing: trailing)
            }
            .onEnded { value in
              isDragging = false
              release(at: value.location)
            }
        )
        .allowsHitTesting(isPresented && !isWorking)
        .accessibilityHidden(true)

      hub(interaction.hoveredAction)
        .frame(width: RingLayout.hubDiameter, height: RingLayout.hubDiameter)
        .background {
          RingGlass().clipShape(Circle())
            .overlay(Circle().fill(RingColor.hubTint))
        }
        .overlay(Circle().strokeBorder(RingColor.edge, lineWidth: RingLayout.hairline))
        // Start a marking gesture in the hub; result text still supports selection and scrolling.
        .allowsHitTesting(leading == .save || leading == .copy)
    }
    .frame(width: RingLayout.interactionDiameter, height: RingLayout.interactionDiameter)
    .onContinuousHover(coordinateSpace: .local) { phase in
      guard isPresented, !isWorking else { return }
      switch phase {
      case .active(let point): interaction.update(at: point, leading: leading, trailing: trailing)
      case .ended:
        if !isDragging { interaction.reset() }
      }
    }
    .scaleEffect(isPresented || reduceMotion ? 1 : RingLayout.collapsedScale)
    .rotationEffect(.degrees(isPresented || reduceMotion ? 0 : RingLayout.collapsedRotation))
    .blur(radius: isPresented || reduceMotion ? 0 : RingLayout.collapsedBlur)
    .opacity(isPresented ? 1 : 0)
    .shadow(color: .black.opacity(0.22), radius: RingLayout.shadowRadius, y: RingLayout.shadowY)
    .animation(
      reduceMotion ? RingMotion.reduced : RingMotion.change, value: interaction.isSealExpanded
    )
    .animation(
      reduceMotion ? RingMotion.reduced : RingMotion.hover, value: interaction.hoveredAction
    )
    .allowsHitTesting(isPresented)
    .onChange(of: isPresented) { _, visible in
      if !visible { reset() }
    }
    .onChange(of: isWorking) { _, working in
      if working { reset() }
    }
    .onChange(of: leading) { _, _ in reset() }
    .onChange(of: trailing) { _, _ in reset() }
    .onChange(of: interaction.isSealExpanded) { _, expanded in
      onExpansionChanged(expanded)
    }
    .onChange(of: releaseRequest) { _, request in
      if let request { release(at: request.location) }
    }
  }

  private func release(at location: CGPoint) {
    guard isPresented, !isWorking else { return }
    if let action = interaction.release(at: location, leading: leading, trailing: trailing) {
      onAction(action)
    }
  }

  private func reset() {
    isDragging = false
    interaction.reset()
  }

  private func sector(action: RingAction, side: RingSide, tint: Color) -> some View {
    let highlighted =
      interaction.hoveredAction == action
      || (action == .seal && interaction.isSealExpanded)
    return RingTile(
      action: action, shape: RingSector(side: side), diameter: RingLayout.diameter,
      labelOffset: CGSize(width: side.direction * RingLayout.labelRadius, height: 0),
      tint: tint, isHighlighted: highlighted, isWorking: isWorking,
      onAction: {
        if action == .seal { interaction.expandSeal() } else { onAction(action) }
      })
  }
}

enum RingSide {
  case left, right
  var direction: CGFloat { self == .left ? -1 : 1 }
}

/// The same resting geometry determines hover and clicks, including the hub and seam gaps.
struct RingHitRegion: Shape {
  func path(in rect: CGRect) -> Path {
    var path = RingSector(side: .left).path(in: rect)
    path.addPath(RingSector(side: .right).path(in: rect))
    return path
  }

  static func side(at location: CGPoint, in rect: CGRect) -> RingSide? {
    if RingSector(side: .left).path(in: rect).contains(location) { return .left }
    if RingSector(side: .right).path(in: rect).contains(location) { return .right }
    return nil
  }
}

/// An annular half, with small gaps at twelve and six o'clock.
struct RingSector: Shape {
  let side: RingSide

  func path(in rect: CGRect) -> Path {
    let center = CGPoint(x: rect.midX, y: rect.midY)
    let outer = min(rect.width, rect.height) / 2
    let inner = outer * RingLayout.innerRadius / RingLayout.outerRadius
    // A half turn is 180 degrees; starting at 90 puts the seams on the vertical axis.
    let halfTurn = 180.0
    let seamAngle = 90.0
    let start = (side == .left ? seamAngle : -seamAngle) + RingLayout.dividerGap
    let end = start + halfTurn - RingLayout.dividerGap * 2
    var path = Path()
    path.addArc(
      center: center, radius: outer, startAngle: .degrees(start), endAngle: .degrees(end),
      clockwise: false)
    path.addArc(
      center: center, radius: inner, startAngle: .degrees(end), endAngle: .degrees(start),
      clockwise: true)
    path.closeSubpath()
    return path
  }
}

/// Native behind-window material keeps the desktop visible through the ring.
struct RingGlass: NSViewRepresentable {
  func makeNSView(context: Context) -> NSVisualEffectView {
    let view = PassthroughGlassView()
    view.material = .hudWindow
    view.blendingMode = .behindWindow
    view.state = .active
    view.appearance = NSAppearance(named: .darkAqua)
    return view
  }

  func updateNSView(_ view: NSVisualEffectView, context: Context) {}
}

private final class PassthroughGlassView: NSVisualEffectView {
  // Glass is decorative; the surrounding SwiftUI sector handles the click.
  override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
