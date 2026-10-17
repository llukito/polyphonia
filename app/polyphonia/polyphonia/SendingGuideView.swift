import SwiftUI

struct SendingGuideButton: View {
  let guide: SendingGuide
  var title = "Image sending steps"
  var showOnAppear = false
  @State private var isPresented = false

  var body: some View {
    Button {
      isPresented = true
    } label: {
      Label(title, systemImage: "paperplane")
        .font(.system(size: RingLayout.captionSize))
        .foregroundStyle(RingColor.seal)
    }
    .buttonStyle(.plain)
    .popover(isPresented: $isPresented) { SendingGuideView(guide: guide) }
    .onAppear { if showOnAppear { isPresented = true } }
  }
}

struct SendingGuideView: View {
  let guide: SendingGuide

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: RingLayout.padding) {
        Text("\(guide.platformName) · \(guide.mode.title)")
          .font(.headline)
        if let verification = guide.verification {
          Text(verification.caption).foregroundStyle(RingColor.secondary)
        }
        ForEach(Array(guide.steps.enumerated()), id: \.offset) { index, step in
          HStack(alignment: .top, spacing: RingLayout.gap) {
            Text("\(index + 1). ").foregroundStyle(RingColor.seal)
            Text(step).frame(maxWidth: .infinity, alignment: .leading)
          }
        }
        if let notes = guide.notes {
          DisclosureGroup("Details and sources") {
            VStack(alignment: .leading, spacing: RingLayout.gap) {
              Text(notes)
              ForEach(guide.sources, id: \.self) { url in
                if url.scheme == "https" || url.scheme == "http" {
                  Link(url.host ?? "Source", destination: url)
                }
              }
            }
            .padding(.top, RingLayout.gap)
          }
        }
      }
      .font(.system(size: RingLayout.guideTextSize))
      .padding(RingLayout.hubPadding)
    }
    .frame(width: RingLayout.guideWidth, height: RingLayout.guideHeight)
    .preferredColorScheme(.dark)
  }
}
