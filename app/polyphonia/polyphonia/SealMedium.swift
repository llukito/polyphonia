enum SealMedium: Sendable {
  case image, audio

  var title: String { self == .image ? "Image" : "Audio" }

  func carrier(imageMode: ImageMode?, audioFormat: AudioFormat) -> CarrierKind? {
    switch self {
    case .image: imageMode?.carrier
    case .audio: audioFormat.carrier
    }
  }
}

enum AudioFormat: String, CaseIterable, Identifiable, Sendable {
  case wav, midi

  var id: Self { self }
  var title: String { self == .wav ? "WAV" : "MIDI" }
  var carrier: CarrierKind { self == .wav ? .musicWav : .musicMidi }
}
