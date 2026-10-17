import SwiftUI

struct AiReview: Identifiable, Sendable {
  enum Kind: Sendable { case shorten, decoy }

  let id = UUID()
  let kind: Kind
  let sourceMessage: String
  let original: String
  let suggestion: String
  let allowedBytes: ClosedRange<Int>?

  var title: String { kind == .shorten ? "Review shorter message" : "Review decoy" }

  func validationMessage(for text: String) -> String? {
    guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
      return "Enter text before using this suggestion."
    }
    if let allowedBytes, !allowedBytes.contains(text.utf8.count) {
      return "Edit the decoy to fit the byte range below."
    }
    if kind == .shorten, text.utf8.count >= sourceMessage.utf8.count {
      return "The new message must be shorter than the current message."
    }
    return nil
  }
}

struct AiReviewCard: View {
  let review: AiReview
  let onAccept: (String) -> Void
  let onCancel: () -> Void

  @State private var draft: String

  init(review: AiReview, onAccept: @escaping (String) -> Void, onCancel: @escaping () -> Void) {
    self.review = review
    self.onAccept = onAccept
    self.onCancel = onCancel
    _draft = State(initialValue: review.suggestion)
  }

  private var draftBytes: Int { draft.utf8.count }
  private var canAccept: Bool { review.validationMessage(for: draft) == nil }

  var body: some View {
    VStack(alignment: .leading, spacing: RingLayout.gap) {
      Text(review.title).fontWeight(.semibold)
      Text(
        review.kind == .shorten
          ? "Check the meaning, then use the suggestion as the message to seal."
          : "Use the suggestion as the decoy shown without a passphrase."
      )
      .font(.caption)
      .foregroundStyle(.secondary)

      HStack(alignment: .top, spacing: RingLayout.gap) {
        VStack(alignment: .leading, spacing: 6) {
          Text(review.kind == .shorten ? "Current message" : "Real message")
            .font(.caption).foregroundStyle(.secondary)
          ScrollView {
            Text(review.sourceMessage)
              .frame(maxWidth: .infinity, alignment: .topLeading)
          }
          .frame(height: RingLayout.reviewEditorHeight)
          .padding(RingLayout.gap)
          .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
        }
        VStack(alignment: .leading, spacing: 6) {
          Text("Suggestion · editable").font(.caption).foregroundStyle(.secondary)
          TextEditor(text: $draft)
            .frame(height: RingLayout.reviewEditorHeight)
            .scrollContentBackground(.hidden)
            .padding(RingLayout.gap)
            .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
        }
      }

      if let validation = review.validationMessage(for: draft) {
        Text(validation).foregroundStyle(RingColor.warning)
      }

      if let range = review.allowedBytes {
        Text("\(draftBytes) bytes · decoy should be \(range.lowerBound)–\(range.upperBound) bytes")
          .foregroundStyle(canAccept ? Color.secondary : RingColor.warning)
      } else {
        Text("\(review.sourceMessage.utf8.count) → \(draftBytes) bytes")
          .foregroundStyle(canAccept ? Color.secondary : RingColor.warning)
      }

      HStack {
        Spacer()
        Button("Discard", action: onCancel)
        Button(review.kind == .shorten ? "Use as message" : "Use as decoy") { onAccept(draft) }
          .buttonStyle(.borderedProminent)
          .disabled(!canAccept)
      }
    }
    .controlSize(.small)
    .font(.system(size: RingLayout.captionSize))
    .padding(RingLayout.padding)
    .background { RingAccessoryGlass() }
    .preferredColorScheme(.dark)
  }
}
