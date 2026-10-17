enum SealMedium: Sendable {
  case image, audio

  var title: String { self == .image ? "Image" : "Audio" }

  func carrier(imageMode: ImageMode?) -> CarrierKind? {
    switch self {
    case .image: imageMode?.carrier
    case .audio: .musicWav
    }
  }
}
