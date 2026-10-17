import SwiftUI

enum RingOption: String, Identifiable {
  case stealth, robust, wav, midi, cover, removeCover, platform

  var id: String { rawValue }

  var title: String {
    switch self {
    case .stealth: "Stealth"
    case .robust: "Robust"
    case .wav: "WAV"
    case .midi: "MIDI"
    case .cover: "Upload"
    case .removeCover: "Remove"
    case .platform: "Platform"
    }
  }

  var symbol: String {
    switch self {
    case .stealth, .robust: "photo"
    case .wav, .midi: "waveform"
    case .cover: "square.and.arrow.up"
    case .removeCover: "xmark.circle"
    case .platform: "paperplane"
    }
  }
}

/// A short outer arc follows the active Image or Audio branch.
struct SealOptionsRing: View {
  // A small angular seam keeps adjacent outer options distinct on hover.
  private static let optionGap: Double = 1.6
  let medium: SealMedium
  let options: [RingOption]
  let isWorking: Bool
  let isSelected: (RingOption) -> Bool
  let onSelect: (RingOption) -> Void

  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var hoveredOption: RingOption?

  var body: some View {
    ZStack {
      ForEach(Array(options.enumerated()), id: \.element.id) { index, option in
        let angles = Self.angles(medium: medium, index: index, count: options.count)
        let shape = RingOptionSector(start: angles.lowerBound, end: angles.upperBound)
        let angle = (angles.lowerBound + angles.upperBound) / 2 * .pi / 180
        let highlighted = hoveredOption == option
        Button { onSelect(option) } label: {
          ZStack {
            RingGlass()
              .overlay(RingColor.smoke)
              .overlay(
                RingColor.seal.opacity(highlighted ? 0.24 : isSelected(option) ? 0.17 : 0))
              .clipShape(shape)
            shape.strokeBorder(
              highlighted || isSelected(option) ? RingColor.seal : RingColor.edge,
              lineWidth: RingLayout.hairline)
            VStack(spacing: 2) {
              Image(systemName: isSelected(option) ? "checkmark.circle.fill" : option.symbol)
                .font(.system(size: 13))
              Text(option.title)
                .font(.system(size: 9, weight: .medium))
                .lineLimit(1)
            }
            .foregroundStyle(highlighted || isSelected(option) ? RingColor.seal : RingColor.ink)
            .scaleEffect(highlighted && !reduceMotion ? RingLayout.labelHoverScale : 1)
            .offset(
              x: cos(angle) * RingLayout.optionsLabelRadius,
              y: sin(angle) * RingLayout.optionsLabelRadius)
          }
          .frame(width: RingLayout.optionsOuterRadius * 2,
                 height: RingLayout.optionsOuterRadius * 2)
          .contentShape(shape)
        }
        .buttonStyle(.plain)
        .disabled(isWorking)
        .allowsHitTesting(false)
        .accessibilityLabel(option.title)
        .accessibilityAddTraits(isSelected(option) ? [.isSelected] : [])
        .offset(
          x: highlighted && !reduceMotion ? cos(angle) * RingLayout.hoverLift : 0,
          y: highlighted && !reduceMotion ? sin(angle) * RingLayout.hoverLift : 0)
        .animation(reduceMotion ? RingMotion.reduced : RingMotion.hover, value: highlighted)
      }

      Color.clear
        .contentShape(RingOptionHitShape(medium: medium, count: options.count))
        .onContinuousHover(coordinateSpace: .local) { phase in
          switch phase {
          case .active(let point): hoveredOption = option(at: point)
          case .ended: hoveredOption = nil
          }
        }
        .gesture(
          DragGesture(minimumDistance: 0, coordinateSpace: .local)
            .onChanged { hoveredOption = option(at: $0.location) }
            .onEnded { point in
              if let option = option(at: point.location) { onSelect(option) }
            })
        .allowsHitTesting(!isWorking)
        .accessibilityHidden(true)
    }
    .frame(width: RingLayout.optionsOuterRadius * 2,
           height: RingLayout.optionsOuterRadius * 2)
    .transition(.scale(scale: 0.96).combined(with: .opacity))
  }

  func option(at point: CGPoint) -> RingOption? {
    Self.option(at: point, medium: medium, options: options)
  }

  static func option(
    at point: CGPoint, medium: SealMedium, options: [RingOption]
  ) -> RingOption? {
    let center = RingLayout.optionsOuterRadius
    let radius = hypot(point.x - center, point.y - center)
    guard radius >= RingLayout.optionsInnerRadius,
      radius <= RingLayout.optionsOuterRadius + RingLayout.hoverLift
    else { return nil }
    let angle = atan2(point.y - center, point.x - center) * 180 / .pi
    return options.enumerated().first {
      angles(medium: medium, index: $0.offset, count: options.count).contains(angle)
    }?.element
  }

  static func angles(medium: SealMedium, index: Int, count: Int) -> ClosedRange<Double> {
    let branch = medium == .image ? RingBranch.image : RingBranch.audio
    let range = branch.angles
    let span = (range.upperBound - range.lowerBound) / Double(count)
    let lower = range.lowerBound + Double(index) * span + optionGap / 2
    return lower...(lower + span - optionGap)
  }
}

private struct RingOptionHitShape: Shape {
  let medium: SealMedium
  let count: Int

  func path(in rect: CGRect) -> Path {
    var path = Path()
    for index in 0..<count {
      let angles = SealOptionsRing.angles(medium: medium, index: index, count: count)
      path.addPath(RingOptionSector(start: angles.lowerBound, end: angles.upperBound).path(in: rect))
    }
    return path
  }
}

private struct RingOptionSector: InsettableShape {
  let start: Double
  let end: Double
  var insetAmount: CGFloat = 0

  func path(in rect: CGRect) -> Path {
    let center = CGPoint(x: rect.midX, y: rect.midY)
    let outer = RingLayout.optionsOuterRadius - insetAmount
    let inner = RingLayout.optionsInnerRadius + insetAmount
    var path = Path()
    path.addArc(center: center, radius: outer, startAngle: .degrees(start),
                endAngle: .degrees(end), clockwise: false)
    path.addArc(center: center, radius: inner, startAngle: .degrees(end),
                endAngle: .degrees(start), clockwise: true)
    path.closeSubpath()
    return path
  }

  func inset(by amount: CGFloat) -> Self {
    var shape = self
    shape.insetAmount += amount
    return shape
  }
}
