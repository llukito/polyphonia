import SwiftUI

struct AudioCoverControls: View {
  let format: AudioFormat
  let coverName: String?
  let onChoose: () -> Void
  let onRemove: () -> Void

  private var supportsCover: Bool { format == .wav }

  var body: some View {
    VStack(spacing: RingLayout.smallGap) {
      HStack(spacing: RingLayout.gap) {
        Image(systemName: "waveform")
        Text(supportsCover ? (coverName ?? "Generated music") : "Generated music")
          .lineLimit(1)
          .truncationMode(.middle)
          .frame(maxWidth: .infinity, alignment: .leading)
        Button(coverName == nil ? "Choose…" : "Replace…", action: onChoose)
          .foregroundStyle(RingColor.seal)
          .disabled(!supportsCover)
          .accessibilityLabel("Choose a cover WAV")
        if coverName != nil {
          Button(action: onRemove) {
            Image(systemName: "xmark")
          }
          .help("Remove cover audio")
          .accessibilityLabel("Remove cover audio")
        }
      }
      .buttonStyle(.plain)
      .padding(.horizontal, RingLayout.padding)
      .frame(height: RingLayout.fieldHeight)
      .background { RingAccessoryGlass() }

      Text(
        supportsCover
          ? "Audio cover · 16-bit PCM WAV → new WAV"
          : "Custom audio needs WAV. MIDI uses generated music."
      )
      .font(.system(size: RingLayout.tinySize))
      .multilineTextAlignment(.center)
    }
    .font(.system(size: RingLayout.captionSize))
    .foregroundStyle(RingColor.secondary)
  }
}
