import SwiftUI

enum RingAction: String, Identifiable {
  case seal, sealImage, sealAudio, open, paste, browse, save, copy, done
  case recipient, hidden, ai, cancelAi, sealFile, passphrase, back, guide

  var id: String { rawValue }

  var title: String {
    switch self {
    case .seal: "Seal"
    case .sealImage: "Image"
    case .sealAudio: "Audio"
    case .open: "Open"
    case .paste: "Paste file"
    case .browse: "Browse"
    case .save: "Save file"
    case .copy: "Copy text"
    case .done: "Done"
    case .recipient: "Recipient"
    case .hidden: "Hidden"
    case .ai: "Local AI"
    case .cancelAi: "Cancel AI"
    case .sealFile: "Seal file"
    case .passphrase: "Passphrase"
    case .back: "Back"
    case .guide: "Send steps"
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
    case .recipient: "person.crop.circle"
    case .hidden: "eye.slash"
    case .ai: "sparkles"
    case .cancelAi: "xmark.circle"
    case .sealFile: "lock.fill"
    case .passphrase: "key"
    case .back: "arrow.uturn.backward"
    case .guide: "list.bullet"
    }
  }
}

/// Both mouse release and shortcut release resolve through this same state and geometry.
struct RingInteraction {
  private(set) var isSealExpanded = false
  private(set) var hoveredAction: RingAction?

  mutating func update(
    at point: CGPoint, leading: RingAction, trailing: RingAction,
    utilities: [RingAction] = []
  ) {
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
    hoveredAction = action(at: point, leading: leading, trailing: trailing, utilities: utilities)
    if hoveredAction == .seal { isSealExpanded = true }
    if hoveredAction == trailing { isSealExpanded = false }
  }

  mutating func release(
    at point: CGPoint, leading: RingAction, trailing: RingAction,
    utilities: [RingAction] = []
  ) -> RingAction?
  {
    update(at: point, leading: leading, trailing: trailing, utilities: utilities)
    let target = action(
      at: point, leading: leading, trailing: trailing, utilities: utilities,
      extendingOutward: true)
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
    at point: CGPoint, leading: RingAction, trailing: RingAction,
    utilities: [RingAction], extendingOutward: Bool = false
  ) -> RingAction? {
    let center = RingLayout.interactionDiameter / 2
    let offset = CGPoint(x: point.x - center, y: point.y - center)
    let radius = hypot(offset.x, offset.y)
    let angle = atan2(offset.y, offset.x) * RingAngles.halfTurn / .pi

    if radius >= RingLayout.branchInnerRadius, radius <= RingLayout.branchOuterRadius,
      let index = RingUtilityArc.index(at: angle, count: utilities.count)
    {
      return utilities[index]
    }

    if leading == .seal, isSealExpanded, offset.x < 0, radius >= RingLayout.branchInnerRadius,
      radius <= RingLayout.branchOuterRadius
    {
      if RingBranch.image.angles.contains(angle) { return .sealImage }
      if RingBranch.audio.angles.contains(angle) { return .sealAudio }
      return nil
    }

    // Marking gestures may finish beyond the visible arc. The hub and divider gaps stay inactive.
    guard radius >= RingLayout.innerRadius,
      extendingOutward || radius <= RingLayout.outerRadius
    else { return nil }
    let seamDistance = abs(abs(angle) - RingAngles.quarterTurn)
    guard seamDistance > RingLayout.dividerGap else { return nil }
    return offset.x < 0 ? leading : trailing
  }
}

enum RingUtilityArc {
  // The right-hand second layer stays clear of the vertical seam and left branches.
  static let start = -75.0
  static let end = 75.0
  // Two degrees between utility sectors matches the visual space around branch labels.
  static let gap = 2.0

  static func angles(for index: Int, count: Int) -> ClosedRange<Double> {
    let span = (end - start) / Double(count)
    let lower = start + Double(index) * span + gap / 2
    return lower...(lower + span - gap)
  }

  static func index(at angle: Double, count: Int) -> Int? {
    guard count > 0 else { return nil }
    return (0..<count).first { angles(for: $0, count: count).contains(angle) }
  }
}

struct RingUtilitySector: Shape {
  let index: Int
  let count: Int

  func path(in rect: CGRect) -> Path {
    let center = CGPoint(x: rect.midX, y: rect.midY)
    let angles = RingUtilityArc.angles(for: index, count: count)
    var path = Path()
    path.addArc(center: center, radius: RingLayout.branchOuterRadius,
                startAngle: .degrees(angles.lowerBound), endAngle: .degrees(angles.upperBound),
                clockwise: false)
    path.addArc(center: center, radius: RingLayout.branchInnerRadius,
                startAngle: .degrees(angles.upperBound), endAngle: .degrees(angles.lowerBound),
                clockwise: true)
    path.closeSubpath()
    return path
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
}
