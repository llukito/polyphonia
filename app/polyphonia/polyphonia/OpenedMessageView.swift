import SwiftUI

/// A reading surface for either kind of opened message; it never identifies the opened slot.
struct OpenedMessageView: View {
  let text: String
  let isPresented: Bool
  let onCopy: () -> Void
  let onDismiss: () -> Void

  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var remaining = Duration.seconds(OpenedMessageLayout.displayDuration)
  @State private var deadline: ContinuousClock.Instant?
  @State private var isHeldOpen = false
  @State private var isCopied = false

  var body: some View {
    VStack(alignment: .leading, spacing: RingLayout.padding) {
      TimelineView(.animation(paused: deadline == nil)) { _ in
        GeometryReader { geometry in
          ZStack(alignment: .leading) {
            Capsule().fill(RingColor.edge)
            Capsule().fill(RingColor.open)
              .frame(width: geometry.size.width * progress)
          }
        }
      }
      .frame(height: OpenedMessageLayout.timerHeight)
      .accessibilityLabel("Message timer")
      .accessibilityValue(isHeldOpen ? "Paused" : "Closes automatically")

      HStack(spacing: RingLayout.gap) {
        Image(systemName: "lock.open")
          .foregroundStyle(RingColor.open)
        Text("Message opened").fontWeight(.medium)
        Spacer()
        Button(action: onDismiss) {
          Image(systemName: "xmark")
            .frame(width: RingLayout.closeDiameter, height: RingLayout.closeDiameter)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(RingColor.secondary)
        .accessibilityLabel("Close message")
        .help("Close message · Escape")
      }
      .font(.system(size: RingLayout.titleSize))

      ScrollView {
        Text(text)
          .font(.system(size: OpenedMessageLayout.textSize))
          .lineSpacing(RingLayout.smallGap)
          .textSelection(.enabled)
          .frame(maxWidth: .infinity, alignment: .leading)
          .fixedSize(horizontal: false, vertical: true)
          .padding(.trailing, RingLayout.gap)
      }
      .frame(height: OpenedMessageLayout.readingHeight)

      HStack {
        Button {
          isHeldOpen.toggle()
          if isHeldOpen { pauseTimer() } else { resumeTimer() }
        } label: {
          Label(
            isHeldOpen ? "Resume timer" : "Keep open",
            systemImage: isHeldOpen ? "play" : "pause")
        }
        .buttonStyle(.plain)
        .foregroundStyle(RingColor.secondary)
        Spacer()
        Button {
          onCopy()
          isCopied = true
        } label: {
          Label(
            isCopied ? "Copied" : "Copy text", systemImage: isCopied ? "checkmark" : "doc.on.doc")
        }
        .buttonStyle(.bordered)
        .tint(RingColor.open)
      }
      .font(.system(size: OpenedMessageLayout.controlSize))
    }
    .foregroundStyle(RingColor.ink)
    .padding(OpenedMessageLayout.padding)
    .frame(width: OpenedMessageLayout.width)
    .background(
      RingColor.panelBackground,
      in: RoundedRectangle(cornerRadius: OpenedMessageLayout.cornerRadius)
    )
    .overlay {
      RoundedRectangle(cornerRadius: OpenedMessageLayout.cornerRadius)
        .strokeBorder(RingColor.edge, lineWidth: RingLayout.hairline)
        .allowsHitTesting(false)
    }
    .shadow(color: RingColor.smoke, radius: RingLayout.shadowRadius, y: RingLayout.shadowY)
    .opacity(isPresented ? 1 : 0)
    .offset(y: isPresented || reduceMotion ? 0 : OpenedMessageLayout.slideDistance)
    .animation(
      reduceMotion
        ? RingMotion.reduced
        : (isPresented ? OpenedMessageLayout.motion : OpenedMessageLayout.dismiss),
      value: isPresented
    )
    .allowsHitTesting(isPresented)
    .onAppear { resumeTimer() }
    .onChange(of: isPresented) { _, presented in
      if presented { resumeTimer() } else { pauseTimer() }
    }
    .task(id: deadline) {
      guard let deadline else { return }
      do {
        try await Task.sleep(until: deadline, clock: .continuous)
      } catch {
        return
      }
      guard isPresented, !isHeldOpen, !Task.isCancelled else { return }
      onDismiss()
    }
  }

  private var progress: CGFloat {
    let timeLeft = deadline.map { ContinuousClock.now.duration(to: $0) } ?? remaining
    return CGFloat(max(0, min(1, timeLeft / .seconds(OpenedMessageLayout.displayDuration))))
  }

  private func resumeTimer() {
    guard isPresented, !isHeldOpen, deadline == nil else { return }
    deadline = ContinuousClock.now.advanced(by: remaining)
  }

  private func pauseTimer() {
    guard let deadline else { return }
    remaining = max(.zero, ContinuousClock.now.duration(to: deadline))
    self.deadline = nil
  }
}

enum OpenedMessageLayout {
  // Eight seconds keeps short messages transient; Keep open allows an unhurried read.
  static let displayDuration: TimeInterval = 8
  // A 460-point card leaves room for readable text while fitting within the existing overlay.
  static let width: CGFloat = 460
  static let readingHeight: CGFloat = 220
  static let textSize: CGFloat = 18
  static let controlSize: CGFloat = 12
  // The card uses twice the ring's twelve-point padding and a matching soft corner.
  static let padding: CGFloat = RingLayout.padding * 2
  static let cornerRadius: CGFloat = padding
  static let timerHeight: CGFloat = 3
  // A short upward glide settles without a pronounced bounce.
  static let slideDistance: CGFloat = 44
  static let motion = Animation.spring(response: 0.5, dampingFraction: 0.9)
  // Match the native panel's closing animation so it stays visible throughout the glide.
  static let dismiss = Animation.easeIn(duration: 0.24)
}
