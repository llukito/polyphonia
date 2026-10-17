import SwiftUI

struct AiReview: Identifiable, Sendable {
  enum Kind: Sendable { case shorten, decoy }

  let id = UUID()
  let kind: Kind
  let original: String
  let suggestion: String
  let allowedBytes: ClosedRange<Int>?

  var title: String { kind == .shorten ? "Review shorter message" : "Review decoy" }
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
  private var canAccept: Bool {
    guard !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return false }
    if let range = review.allowedBytes { return range.contains(draftBytes) }
    return draftBytes < review.original.utf8.count
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      Text(review.title).font(.headline)
      Text("A local model proposed this text. Check the meaning before using it.")
        .font(.caption)
        .foregroundStyle(.secondary)

      HStack(alignment: .top, spacing: 12) {
        VStack(alignment: .leading, spacing: 6) {
          Text(review.kind == .shorten ? "Original" : "Current decoy")
            .font(.caption).foregroundStyle(.secondary)
          ScrollView {
            Text(review.original.isEmpty ? "No decoy yet" : review.original)
              .frame(maxWidth: .infinity, alignment: .topLeading)
          }
          .frame(height: 160)
          .padding(8)
          .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
        }
        VStack(alignment: .leading, spacing: 6) {
          Text("Suggestion · editable").font(.caption).foregroundStyle(.secondary)
          TextEditor(text: $draft)
            .frame(minHeight: 160, maxHeight: 160)
            .scrollContentBackground(.hidden)
            .padding(6)
            .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
        }
      }

      if let range = review.allowedBytes {
        Text("\(draftBytes) bytes · decoy should be \(range.lowerBound)–\(range.upperBound) bytes")
          .foregroundStyle(canAccept ? Color.secondary : RingColor.warning)
      } else {
        Text("\(review.original.utf8.count) → \(draftBytes) bytes")
          .foregroundStyle(canAccept ? Color.secondary : RingColor.warning)
      }

      HStack {
        Spacer()
        Button("Cancel", action: onCancel)
        Button("Use this text") { onAccept(draft) }
          .buttonStyle(.borderedProminent)
          .disabled(!canAccept)
      }
    }
    .padding(20)
    .frame(width: 560)
    .preferredColorScheme(.dark)
  }
}
