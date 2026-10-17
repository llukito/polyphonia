import SwiftUI

enum RingOption: String, Identifiable {
  case stealth, robust, wav, midi, cover, removeCover, platform, recipient, hidden, ai, seal
  case passphrase, back, guide

  var id: String { rawValue }

  var title: String {
    switch self {
    case .stealth: "Stealth"
    case .robust: "Robust"
    case .wav: "WAV"
    case .midi: "MIDI"
    case .cover: "Cover"
    case .removeCover: "Remove"
    case .platform: "Platform"
    case .recipient: "Recipient"
    case .hidden: "Hidden"
    case .ai: "Local AI"
    case .seal: "Seal file"
    case .passphrase: "Passphrase"
    case .back: "Back"
    case .guide: "Send steps"
    }
  }

  var symbol: String {
    switch self {
    case .stealth, .robust: "photo"
    case .wav, .midi: "waveform"
    case .cover: "square.and.arrow.up"
    case .removeCover: "xmark.circle"
    case .platform: "paperplane"
    case .recipient: "person.crop.circle"
    case .hidden: "eye.slash"
    case .ai: "sparkles"
    case .seal: "lock.fill"
    case .passphrase: "key"
    case .back: "arrow.uturn.backward"
    case .guide: "list.bullet"
    }
  }
}

struct SealOptionsRing: View {
  let options: [RingOption]
  let isWorking: Bool
  let isAiWorking: Bool
  let isSelected: (RingOption) -> Bool
  let onSelect: (RingOption) -> Void

  var body: some View {
    ZStack {
      ForEach(Array(options.enumerated()), id: \.element.id) { index, option in
        let span = 360.0 / Double(options.count)
        let start = -90.0 + Double(index) * span
        let shape = RingOptionSector(start: start + 1.5, end: start + span - 1.5)
        let angle = (start + span / 2) * .pi / 180
        Button { onSelect(option) } label: {
          ZStack {
            RingGlass()
              .overlay(RingColor.smoke)
              .overlay(isSelected(option) ? RingColor.seal.opacity(0.17) : .clear)
              .clipShape(shape)
            shape.strokeBorder(
              isSelected(option) ? RingColor.seal : RingColor.edge,
              lineWidth: RingLayout.hairline)
            VStack(spacing: 2) {
              Image(systemName: isAiWorking && option == .ai ? "xmark.circle" :
                    isSelected(option) ? "checkmark.circle.fill" : option.symbol)
                .font(.system(size: 14))
              Text(isAiWorking && option == .ai ? "Cancel AI" : option.title)
                .font(.system(size: 9, weight: .medium))
                .lineLimit(1)
            }
            .foregroundStyle(isSelected(option) ? RingColor.seal : RingColor.ink)
            .offset(
              x: cos(angle) * RingLayout.optionsLabelRadius,
              y: sin(angle) * RingLayout.optionsLabelRadius)
          }
          .frame(width: RingLayout.optionsOuterRadius * 2,
                 height: RingLayout.optionsOuterRadius * 2)
          .contentShape(shape)
        }
        .buttonStyle(.plain)
        .disabled(isWorking || (isAiWorking && option != .ai))
        .accessibilityLabel(isAiWorking && option == .ai ? "Cancel local AI request" : option.title)
        .accessibilityAddTraits(isSelected(option) ? [.isSelected] : [])
      }
    }
    .frame(width: RingLayout.optionsOuterRadius * 2,
           height: RingLayout.optionsOuterRadius * 2)
    .transition(.scale(scale: 0.94).combined(with: .opacity))
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
