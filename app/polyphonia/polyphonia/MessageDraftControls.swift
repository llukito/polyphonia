import SwiftUI

/// The fields here are the same session values read by Seal → Image and Seal → Audio.
struct MessageDraftControls: View {
  @ObservedObject var session: MessageSession
  let onReveal: (AccessoryTarget) -> Void

  private var draftLocked: Bool {
    session.isWorking || session.isAiWorking || session.aiReview != nil
  }

  var body: some View {
    VStack(alignment: .leading, spacing: RingLayout.padding) {
      draftEditor("Message to seal", text: $session.message)
        .disabled(draftLocked)
        .accessoryTarget(.message)

      if let review = session.aiReview, review.kind == .shorten { reviewCard(review) }

      if !session.aiServers.isEmpty || session.isAiWorking {
        localAi
          .accessoryTarget(.aiTools)
      }

      if session.hiddenMode {
        VStack(alignment: .leading, spacing: RingLayout.gap) {
          draftEditor("Decoy · visible without passphrase", text: $session.decoy)
            .disabled(draftLocked)

          if !session.aiServers.isEmpty, !session.isAiWorking {
            Button(action: session.requestDecoy) {
              Label("Generate decoy with local AI", systemImage: "sparkles")
                .foregroundStyle(RingColor.seal)
                .frame(maxWidth: .infinity, minHeight: RingLayout.fieldHeight)
                .background { RingAccessoryGlass() }
            }
            .buttonStyle(.plain)
            .disabled(draftLocked || session.selectedAiModel == nil || session.message.isEmpty)
          }

          if let review = session.aiReview, review.kind == .decoy { reviewCard(review) }

          SecureField("Agreed passphrase", text: $session.passphrase)
            .textFieldStyle(RingFieldStyle())
            .disabled(session.isWorking)
          Text(
            session.passphrase.isEmpty
              ? "Add an agreed passphrase before sealing the hidden message."
              : "The passphrase opens your real message; without it, the recipient sees the decoy."
          )
          .foregroundStyle(session.passphrase.isEmpty ? RingColor.warning : RingColor.secondary)
        }
        .accessoryTarget(.hidden)
      }

      Text("Choose Seal → Image or Audio when ready.")
        .foregroundStyle(RingColor.secondary)
    }
    .font(.system(size: RingLayout.captionSize))
    .foregroundStyle(RingColor.ink)
    .controlSize(.small)
  }

  private func draftEditor(_ title: String, text: Binding<String>) -> some View {
    VStack(alignment: .leading, spacing: RingLayout.smallGap) {
      HStack {
        Text(title).fontWeight(.medium)
        Spacer()
        Text("\(text.wrappedValue.utf8.count) bytes").foregroundStyle(RingColor.secondary)
      }
      TextEditor(text: text)
        .font(.system(size: RingLayout.guideTextSize))
        .scrollContentBackground(.hidden)
        .frame(height: RingLayout.draftEditorHeight)
        .padding(RingLayout.gap)
        .background { RingAccessoryGlass() }
        .accessibilityLabel(title)
    }
  }

  private var localAi: some View {
    VStack(alignment: .leading, spacing: RingLayout.gap) {
      Label("Local AI", systemImage: "sparkles").fontWeight(.medium)
      if session.isAiWorking {
        HStack {
          ProgressView().controlSize(.small)
          Text("Generating suggestion…")
          Spacer()
          Button("Cancel", action: session.cancelAiRequest)
            .buttonStyle(.plain)
            .foregroundStyle(RingColor.seal)
        }
      } else {
        HStack {
          Menu {
            ForEach(session.aiServers, id: \.baseUrl) { server in
              Button(server.kind == .ollama ? "Ollama" : "LM Studio") {
                session.selectedAiServerURL = server.baseUrl
                session.selectedAiModel = server.models.first
              }
            }
          } label: {
            Text(session.selectedAiServer?.kind == .ollama ? "Ollama" : "LM Studio")
          }
          if let server = session.selectedAiServer {
            Menu {
              ForEach(server.models, id: \.self) { model in
                Button(model) { session.selectedAiModel = model }
              }
            } label: {
              Text(session.selectedAiModel ?? "Choose model").lineLimit(1)
            }
          }
        }
        .disabled(draftLocked)
        Button("Shorten message", action: session.requestShortening)
          .disabled(draftLocked || session.selectedAiModel == nil || session.message.isEmpty)
        Text("Suggestions need your review. Enable Hidden to generate a decoy.")
          .foregroundStyle(RingColor.secondary)
      }
    }
    .padding(RingLayout.padding)
    .background { RingAccessoryGlass() }
  }

  private func reviewCard(_ review: AiReview) -> some View {
    AiReviewCard(
      review: review,
      onAccept: { text in
        if session.applyAiReview(review, text: text) {
          onReveal(review.kind == .shorten ? .message : .hidden)
        } else {
          onReveal(.status)
        }
      },
      onCancel: {
        session.discardAiReview()
        onReveal(review.kind == .shorten ? .message : .hidden)
      }
    )
    .id(review.id)
    .accessoryTarget(.aiReview)
  }
}
