import SwiftUI

struct PlatformPicker: View {
  @ObservedObject var selection: PlatformSelection

  var body: some View {
    HStack(spacing: RingLayout.gap) {
      Image(systemName: "paperplane")
      Menu {
        ForEach(selection.profiles?.platforms ?? []) { platform in
          Button {
            selection.selectPlatform(platform.id)
          } label: {
            if selection.platformID == platform.id {
              Label(platform.name, systemImage: "checkmark")
            } else {
              Text(platform.name)
            }
          }
        }
        Divider()
        Button("Other") { selection.selectPlatform(PlatformProfiles.otherID) }
      } label: {
        Text(selection.platformName)
      }
      .menuStyle(.borderlessButton)
      .controlSize(.small)
      .font(.system(size: RingLayout.captionSize))
      .frame(maxWidth: .infinity, alignment: .leading)
      .accessibilityLabel("Destination platform: \(selection.platformName)")
      Text(selection.wasDetected ? "Detected" : "Platform")
        .font(.system(size: RingLayout.tinySize))
        .foregroundStyle(RingColor.secondary)
    }
    .foregroundStyle(RingColor.ink)
    .font(.system(size: RingLayout.captionSize))
    .padding(.horizontal, RingLayout.padding)
    .frame(height: RingLayout.fieldHeight)
    .background { RingAccessoryGlass() }
    .disabled(selection.profiles == nil)
    .help("Where will you send the image? You can override the detected platform.")
  }
}

struct PlatformControls: View {
  @ObservedObject var selection: PlatformSelection

  var body: some View {
    VStack(spacing: RingLayout.gap) {
      PlatformPicker(selection: selection)
      if let error = selection.loadError {
        Text(error).foregroundStyle(RingColor.warning)
      } else {
        Text("Image mode")
          .font(.system(size: RingLayout.tinySize))
          .foregroundStyle(RingColor.secondary)
        if selection.platform == nil {
          // Other has no recommendation: explain both options alongside the explicit choice.
          VStack(spacing: RingLayout.gap) {
            ForEach(ImageMode.allCases) { mode in modeButton(mode, showExplanation: true) }
          }
        } else {
          HStack(spacing: RingLayout.smallGap) {
            ForEach(ImageMode.allCases) { mode in modeButton(mode, showExplanation: false) }
          }
          if let verification = selection.platform?.verified {
            Text(verification.caption)
              .foregroundStyle(
                verification.status == .verified ? RingColor.secondary : RingColor.warning)
          }
        }
        if let mode = selection.imageMode, let guide = selection.guide(for: mode) {
          SendingGuideButton(guide: guide)
        }
      }
    }
    .font(.system(size: RingLayout.captionSize))
  }

  private func modeButton(_ mode: ImageMode, showExplanation: Bool) -> some View {
    let selected = selection.imageMode == mode
    return Button {
      selection.selectMode(mode)
    } label: {
      VStack(alignment: showExplanation ? .leading : .center, spacing: RingLayout.smallGap) {
        Label(mode.title, systemImage: selected ? "checkmark.circle.fill" : "circle")
        if showExplanation, let explanation = selection.profiles?.other.explain[mode] {
          Text(explanation).foregroundStyle(RingColor.secondary).fixedSize(
            horizontal: false, vertical: true)
        }
      }
      .frame(
        maxWidth: .infinity, minHeight: RingLayout.fieldHeight,
        alignment: showExplanation ? .leading : .center
      )
      .padding(.horizontal, RingLayout.padding)
      .padding(.vertical, showExplanation ? RingLayout.gap : 0)
      .background {
        RingAccessoryGlass()
          .overlay(RingColor.seal.opacity(selected ? RingLayout.selectedModeOpacity : 0))
          .clipShape(RoundedRectangle(cornerRadius: RingLayout.capsuleRadius))
      }
      .foregroundStyle(selected ? RingColor.seal : RingColor.ink)
    }
    .buttonStyle(.plain)
    .accessibilityAddTraits(selected ? [.isSelected] : [])
  }
}

struct RingAccessoryGlass: View {
  var body: some View {
    RingGlass()
      .overlay(RingColor.smoke)
      .clipShape(RoundedRectangle(cornerRadius: RingLayout.capsuleRadius))
      .overlay {
        RoundedRectangle(cornerRadius: RingLayout.capsuleRadius)
          .strokeBorder(RingColor.edge, lineWidth: RingLayout.hairline)
      }
  }
}
