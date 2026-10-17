import SwiftUI

struct AudioFormatPicker: View {
  @Binding var selection: AudioFormat

  var body: some View {
    HStack(spacing: RingLayout.gap) {
      Label("Audio", systemImage: "waveform")
        .foregroundStyle(RingColor.secondary)
      Spacer(minLength: RingLayout.gap)
      ForEach(AudioFormat.allCases) { format in
        Button {
          selection = format
        } label: {
          Text(format.title)
            .foregroundStyle(selection == format ? RingColor.seal : RingColor.secondary)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Audio format: \(format.title)")
        .accessibilityAddTraits(selection == format ? [.isSelected] : [])
      }
    }
    .font(.system(size: RingLayout.captionSize))
    .padding(.horizontal, RingLayout.padding)
    .frame(height: RingLayout.fieldHeight)
    .background { RingAccessoryGlass() }
    .help(
      "Audio uses this format. Send the original file as a document; do not record a voice message."
    )
  }
}
