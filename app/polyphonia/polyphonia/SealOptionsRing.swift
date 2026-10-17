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
  var hoverLocation: CGPoint? = nil
  let isSelected: (RingOption) -> Bool
  let onSelect: (RingOption) -> Void

  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @Namespace private var pointerSpace
  @State private var dragLocation: CGPoint?

  private var hoveredOption: RingOption? {
    guard !isWorking, let point = dragLocation ?? hoverLocation else { return nil }
    return option(at: point)
  }

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
              x: cos(angle) * labelRadius(highlighted: highlighted),
              y: sin(angle) * labelRadius(highlighted: highlighted))
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
        .animation(reduceMotion ? RingMotion.reduced : RingMotion.hover, value: highlighted)
      }

      Color.clear
        .contentShape(RingOptionHitShape(medium: medium, count: options.count))
        .gesture(
          DragGesture(minimumDistance: 0, coordinateSpace: .named(pointerSpace))
            .onChanged { dragLocation = $0.location }
            .onEnded { point in
              dragLocation = nil
              if let option = option(at: point.location) { onSelect(option) }
            })
        .allowsHitTesting(!isWorking)
        .accessibilityHidden(true)
    }
    .frame(width: RingLayout.optionsOuterRadius * 2,
           height: RingLayout.optionsOuterRadius * 2)
    .coordinateSpace(name: pointerSpace)
    .onChange(of: medium) { _, _ in dragLocation = nil }
    .onChange(of: options) { _, _ in dragLocation = nil }
    .onChange(of: isWorking) { _, working in
      if working { dragLocation = nil }
    }
    .transition(.scale(scale: 0.96).combined(with: .opacity))
  }

  private func labelRadius(highlighted: Bool) -> CGFloat {
    RingLayout.optionsLabelRadius
      + (highlighted && !reduceMotion ? RingLayout.optionsLabelHoverLift : 0)
  }

  func option(at point: CGPoint) -> RingOption? {
    Self.option(at: point, medium: medium, options: options)
  }

  static func option(
    at point: CGPoint, medium: SealMedium, options: [RingOption]
  ) -> RingOption? {
    let diameter = RingLayout.optionsOuterRadius * 2
    let bounds = CGRect(x: 0, y: 0, width: diameter, height: diameter)
    return options.enumerated().first {
      let range = angles(medium: medium, index: $0.offset, count: options.count)
      return RingOptionSector(start: range.lowerBound, end: range.upperBound)
        .path(in: bounds).contains(point)
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
