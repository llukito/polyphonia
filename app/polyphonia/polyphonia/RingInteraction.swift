import SwiftUI

enum RingAction: String, Identifiable {
  case seal, sealImage, sealAudio, open, paste, browse, save, copy, done, options

  var id: String { rawValue }

  var title: String {
    switch self {
    case .seal: "Seal"
    case .sealImage: "Image"
    case .sealAudio: "Audio"
    case .open: "Open"
    case .paste: "Paste"
    case .browse: "Browse"
    case .save: "Save file"
    case .copy: "Copy text"
    case .done: "Done"
    case .options: "Options"
    }
  }

  var symbol: String {
    switch self {
    case .seal: "lock"
    case .sealImage: "photo"
    case .sealAudio: "waveform"
    case .open: "lock.open"
    case .paste: "doc.on.clipboard"
    case .browse: "folder"
    case .save: "arrow.down.to.line"
    case .copy: "doc.on.doc"
    case .done: "checkmark"
    case .options: "slider.horizontal.3"
    }
  }
}

/// Both mouse release and shortcut release resolve through this same state and geometry.
struct RingInteraction {
  private(set) var isSealExpanded = false
  private(set) var hoveredAction: RingAction?

  mutating func update(at point: CGPoint, leading: RingAction, trailing: RingAction) {
    let center = RingLayout.interactionDiameter / 2
    let radius = hypot(point.x - center, point.y - center)
    guard radius >= RingLayout.innerRadius else {
      reset()
      return
    }
    // Fast swipes can skip an entire frame over the parent arc. Crossing its radial direction
    // must still reveal the children, so a single gesture can reach either leaf reliably.
    if leading == .seal, point.x < center {
      isSealExpanded = true
    }
    hoveredAction = action(at: point, leading: leading, trailing: trailing)
    if hoveredAction == .seal { isSealExpanded = true }
    if hoveredAction == trailing || hoveredAction == .options { isSealExpanded = false }
  }

  mutating func release(at point: CGPoint, leading: RingAction, trailing: RingAction) -> RingAction?
  {
    update(at: point, leading: leading, trailing: trailing)
    let target = action(at: point, leading: leading, trailing: trailing, extendingOutward: true)
    hoveredAction = target
    if target == .seal {
      isSealExpanded = true
      return nil
    }
    return target
  }

  mutating func expandSeal() {
    isSealExpanded = true
    hoveredAction = .seal
  }

  mutating func reset() { self = RingInteraction() }

  private func action(
    at point: CGPoint, leading: RingAction, trailing: RingAction, extendingOutward: Bool = false
  ) -> RingAction? {
    let center = RingLayout.interactionDiameter / 2
    let offset = CGPoint(x: point.x - center, y: point.y - center)
    let radius = hypot(offset.x, offset.y)
    let angle = atan2(offset.y, offset.x) * RingAngles.halfTurn / .pi

    if leading == .seal, isSealExpanded, offset.x < 0, radius >= RingLayout.branchInnerRadius,
      extendingOutward || radius <= RingLayout.branchOuterRadius
    {
      if RingBranch.image.angles.contains(angle) { return .sealImage }
      if RingBranch.audio.angles.contains(angle) { return .sealAudio }
      return nil
    }

    // Marking gestures may finish beyond the visible arc. The hub and divider gaps stay inactive.
    guard radius >= RingLayout.innerRadius,
      extendingOutward || radius <= RingLayout.outerRadius
    else { return nil }
    if RingAngles.options.contains(angle, gap: RingLayout.dividerGap) { return .options }
    if RingAngles.seal.contains(angle, gap: RingLayout.dividerGap) { return leading }
    if RingAngles.open.contains(angle, gap: RingLayout.dividerGap) { return trailing }
    return nil
  }
}

enum RingBranch: CaseIterable {
  case image, audio

  var action: RingAction { self == .image ? .sealImage : .sealAudio }

  // Screen coordinates grow downward: negative angles put Image above the horizontal axis.
  var angles: ClosedRange<Double> {
    switch self {
    case .image:
      (-RingAngles.halfTurn + RingLayout.branchDividerGap)...(-RingLayout.branchStartAngle)
    case .audio: RingLayout.branchStartAngle...(RingAngles.halfTurn - RingLayout.branchDividerGap)
    }
  }

  var labelOffset: CGSize {
    let middle = (angles.lowerBound + angles.upperBound) / 2 * .pi / RingAngles.halfTurn
    return CGSize(
      width: cos(middle) * RingLayout.branchLabelRadius,
      height: sin(middle) * RingLayout.branchLabelRadius)
  }
}

struct RingBranchSector: Shape {
  let branch: RingBranch

  func path(in rect: CGRect) -> Path {
    let center = CGPoint(x: rect.midX, y: rect.midY)
    var path = Path()
    path.addArc(
      center: center, radius: RingLayout.branchOuterRadius,
      startAngle: .degrees(branch.angles.lowerBound), endAngle: .degrees(branch.angles.upperBound),
      clockwise: false)
    path.addArc(
      center: center, radius: RingLayout.branchInnerRadius,
      startAngle: .degrees(branch.angles.upperBound), endAngle: .degrees(branch.angles.lowerBound),
      clockwise: true)
    path.closeSubpath()
    return path
  }
}

struct RingReleaseRequest: Equatable {
  // A fresh ID also represents consecutive releases at the same pointer position.
  let id = UUID()
  let location: CGPoint
}

enum RingAngles {
  // Standard angle units used to convert atan2 radians and keep divider seams on the vertical axis.
  static let halfTurn = 180.0
  static let quarterTurn = 90.0

  static let seal = 120.0...270.0
  static let open = -90.0...60.0
  static let options = 60.0...120.0
}

extension ClosedRange where Bound == Double {
  func contains(_ angle: Double, gap: Double) -> Bool {
    if upperBound > 180, angle < lowerBound - 180 {
      return angle + 360 >= lowerBound + gap && angle + 360 <= upperBound - gap
    }
    return angle >= lowerBound + gap && angle <= upperBound - gap
  }
}
