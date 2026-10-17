import SwiftUI

/// The same glass and spring treatment is used for parent actions and their outer branches.
struct RingTile<Sector: Shape>: View {
  let action: RingAction
  let shape: Sector
  let diameter: CGFloat
  let labelOffset: CGSize
  let tint: Color
  let isHighlighted: Bool
  let isWorking: Bool
  let onAction: () -> Void

  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  private var emphasized: Bool { isHighlighted && !isWorking }

  var body: some View {
    Button(action: onAction) {
      ZStack {
        RingGlass()
          .overlay(RingColor.smoke)
          .overlay {
            LinearGradient(
              colors: [RingColor.glassTop, RingColor.glassBottom],
              startPoint: .topLeading, endPoint: .bottomTrailing)
          }
          .overlay(tint.opacity(emphasized ? 0.20 : 0))
          .clipShape(shape)

        shape.stroke(
          LinearGradient(
            colors: [.white.opacity(emphasized ? 0.38 : 0.24), .white.opacity(0.04)],
            startPoint: .top, endPoint: .bottom), lineWidth: RingLayout.hairline)

        VStack(spacing: RingLayout.gap) {
          Image(systemName: action.symbol)
            .font(.system(size: RingLayout.iconSize, weight: .light))
            .contentTransition(.symbolEffect(.replace))
          Text(action.title)
            .font(.system(size: RingLayout.captionSize, weight: .medium))
            .tracking(0.3)
            .contentTransition(.opacity)
        }
        .foregroundStyle(emphasized ? tint : RingColor.ink)
        .scaleEffect(emphasized && !reduceMotion ? RingLayout.labelHoverScale : 1)
        .offset(labelOffset)
      }
      .frame(width: diameter, height: diameter)
      .contentShape(shape)
    }
    .buttonStyle(.plain)
    .disabled(isWorking)
    // Native button accessibility is retained; all pointer input uses one stationary surface.
    .allowsHitTesting(false)
    .offset(
      x: emphasized && !reduceMotion ? lift.width : 0,
      y: emphasized && !reduceMotion ? lift.height : 0
    )
    .animation(reduceMotion ? RingMotion.reduced : RingMotion.hover, value: emphasized)
    .accessibilityLabel(action.title)
  }

  private var lift: CGSize {
    let radius = hypot(labelOffset.width, labelOffset.height)
    guard radius > 0 else { return .zero }
    return CGSize(
      width: labelOffset.width / radius * RingLayout.hoverLift,
      height: labelOffset.height / radius * RingLayout.hoverLift)
  }
}
