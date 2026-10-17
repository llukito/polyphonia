import SwiftUI

struct ImageCoverControls: View {
  @ObservedObject var selection: PlatformSelection
  let coverName: String?
  let onChoose: () -> Void
  let onRemove: () -> Void

  private var supportsCover: Bool { selection.imageMode == .stealth }

  var body: some View {
    VStack(spacing: RingLayout.smallGap) {
      HStack(spacing: RingLayout.gap) {
        Image(systemName: "photo")
        Text(supportsCover ? (coverName ?? "Generated image") : "Generated image")
          .lineLimit(1)
          .truncationMode(.middle)
          .frame(maxWidth: .infinity, alignment: .leading)
        Button(coverName == nil ? "Choose…" : "Replace…", action: onChoose)
          .foregroundStyle(RingColor.seal)
          .disabled(!supportsCover)
          .accessibilityLabel("Choose a cover image")
        if coverName != nil {
          Button(action: onRemove) {
            Image(systemName: "xmark")
          }
          .help("Remove cover image")
          .accessibilityLabel("Remove cover image")
        }
      }
      .buttonStyle(.plain)
      .padding(.horizontal, RingLayout.padding)
      .frame(height: RingLayout.fieldHeight)
      .background { RingAccessoryGlass() }

      Text(
        supportsCover
          ? "Image cover · PNG or JPEG → new PNG"
          : "Custom covers need Stealth. Robust uses a generated image."
      )
      .font(.system(size: RingLayout.tinySize))
      .multilineTextAlignment(.center)
    }
    .font(.system(size: RingLayout.captionSize))
    .foregroundStyle(RingColor.secondary)
  }
}
