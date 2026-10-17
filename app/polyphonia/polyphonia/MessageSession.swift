import AppKit
import Combine
import Foundation
import UniformTypeIdentifiers

@MainActor
final class MessageSession: ObservableObject {
  enum Screen {
    case home
    case open
    case result
    case contact
  }

  @Published private(set) var screen: Screen = .home
  @Published var message = ""
  @Published var decoy = ""
  @Published var passphrase = ""
  @Published var hiddenMode = false
  @Published var compactMessages = false
  @Published var audioFormat: AudioFormat = .wav
  @Published private(set) var imageCover: ImageCover?
  @Published private(set) var audioCover: AudioCover?
  @Published private(set) var aiServers: [AiServer] = []
  @Published var selectedAiServerURL: String?
  @Published var selectedAiModel: String?
  @Published private(set) var aiReview: AiReview?
  @Published private(set) var isAiWorking = false
  @Published var isWorking = false
  @Published var statusMessage: String?
  @Published private(set) var resultText: String?
  @Published private(set) var fileSummary: String?
  @Published private(set) var selectionIssue: SelectionIssue?
  @Published private(set) var sendingGuide: SendingGuide?
  @Published private(set) var resendGuide: SendingGuide?
  @Published private(set) var contacts: [Contact] = []
  @Published var selectedContactID: UUID?
  @Published private(set) var pendingInvitation: PublicKeyInvitation?
  @Published var invitationName = ""
  let platformSelection = PlatformSelection()

  private let service: any PolyphoniaBackend
  private let contactStore: ContactStore
  private let pasteboard: NSPasteboard
  private var generatedFileURL: URL?
  private var aiDiscoveryID = UUID()
  private var aiRequestID: UUID?

  init(
    service: any PolyphoniaBackend, defaults: UserDefaults = .standard,
    pasteboard: NSPasteboard = .general
  ) {
    self.service = service
    contactStore = ContactStore(defaults: defaults)
    self.pasteboard = pasteboard
    do {
      contacts = try contactStore.load()
      selectedContactID = contacts.first(where: { $0.algorithm != .fake })?.id
    } catch {
      statusMessage = "Could not load your contacts."
      return
    }
    Task {
      do {
        let identity = try await Task.detached(priority: .userInitiated) {
          try service.identity(algorithm: .mlkem)
        }.value
        addOwnContact(identity, algorithm: .mlkem)
      } catch {
        statusMessage = "Could not load your encryption identity from Keychain."
      }
    }
  }

  private func addOwnContact(_ identity: AppIdentity, algorithm: CryptoAlgorithm) {
    if !contacts.contains(where: { $0.publicKey == identity.publicKey && $0.algorithm == algorithm }
    ) {
      contacts.insert(
        Contact(name: "This Mac", publicKey: identity.publicKey, algorithm: algorithm), at: 0)
      do { try contactStore.save(contacts) } catch {
        statusMessage = "Could not save your public contact. Please try again."
      }
    }
    if selectedContactID == nil {
      selectedContactID = contacts.first(where: { $0.publicKey == identity.publicKey })?.id
    }
  }

  func beginSession(with selection: SelectionCapture, sourceApplication: NSRunningApplication?) {
    guard !isWorking else { return }
    message = selection.text ?? ""
    selectionIssue = selection.issue
    platformSelection.beginSession(source: sourceApplication)
    sendingGuide = nil
    resendGuide = nil
    decoy = ""
    passphrase = ""
    hiddenMode = false
    statusMessage = selection.issue?.message
    resultText = nil
    fileSummary = nil
    pendingInvitation = nil
    invitationName = ""
    imageCover = nil
    audioCover = nil
    aiReview = nil
    cancelAiRequest()
    discoverAiServers()
    screen = .home
  }

  var selectedAiServer: AiServer? {
    aiServers.first(where: { $0.baseUrl == selectedAiServerURL })
  }

  func discoverAiServers() {
    let discoveryID = UUID()
    aiDiscoveryID = discoveryID
    let backend = service
    Task {
      let servers = await Task.detached(priority: .utility) {
        backend.detectAiServers()
      }.value
      guard aiDiscoveryID == discoveryID else { return }
      aiServers = servers.filter { !$0.models.isEmpty }
      if !aiServers.contains(where: { $0.baseUrl == selectedAiServerURL }) {
        selectedAiServerURL = aiServers.first?.baseUrl
      }
      if selectedAiServer?.models.contains(selectedAiModel ?? "") != true {
        selectedAiModel = selectedAiServer?.models.first
      }
    }
  }

  func requestShortening() { requestAiSuggestion(kind: .shorten) }
  func requestDecoy() { requestAiSuggestion(kind: .decoy) }

  private func requestAiSuggestion(kind: AiReview.Kind) {
    guard screen == .home, !isWorking, !isAiWorking, aiReview == nil,
      let server = selectedAiServer, let model = selectedAiModel,
      server.models.contains(model)
    else { return }
    let source = message
    let currentDecoy = decoy
    guard !source.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
      statusMessage = "Select a message before asking the local model."
      return
    }
    let requestID = UUID()
    aiRequestID = requestID
    isAiWorking = true
    statusMessage = nil
    let backend = service
    Task {
      do {
        let review = try await Task.detached(priority: .userInitiated) {
          if kind == .shorten {
            let result = try backend.shorten(message: source, server: server, model: model)
            return AiReview(
              kind: .shorten, sourceMessage: source, original: source, suggestion: result.text,
              allowedBytes: nil)
          }
          let result = try backend.generateDecoy(message: source, server: server, model: model)
          return AiReview(
            kind: .decoy, sourceMessage: source, original: currentDecoy, suggestion: result.text,
            allowedBytes: Int(result.minBytes)...Int(result.maxBytes))
        }.value
        guard aiRequestID == requestID else { return }
        if message == source, kind == .shorten || decoy == currentDecoy {
          aiReview = review
        } else {
          statusMessage = "The draft changed while AI was working. Try again with the current text."
        }
      } catch {
        guard aiRequestID == requestID else { return }
        if case AiFfiError.Rejected = error {
          statusMessage =
            "The local model could not keep the required facts. Try again or edit the text yourself."
        } else {
          statusMessage = "Local AI could not finish: \(error.localizedDescription)"
        }
      }
      guard aiRequestID == requestID else { return }
      aiRequestID = nil
      isAiWorking = false
    }
  }

  func cancelAiRequest() {
    aiRequestID = nil
    isAiWorking = false
  }

  func discardAiReview() { aiReview = nil }

  @discardableResult
  func applyAiReview(_ review: AiReview, text: String) -> Bool {
    guard aiReview?.id == review.id else { return false }
    guard message == review.sourceMessage,
      review.kind != .decoy || decoy == review.original
    else {
      aiReview = nil
      statusMessage = "The draft changed. Generate a new suggestion for the current text."
      return false
    }
    guard review.validationMessage(for: text) == nil else { return false }
    switch review.kind {
    case .shorten: message = text
    case .decoy:
      decoy = text
      hiddenMode = true
    }
    self.aiReview = nil
    statusMessage =
      review.kind == .shorten
      ? "Shorter message applied. Seal → Image or Audio will use this text."
      : passphrase.isEmpty
        ? "Decoy applied. Add an agreed passphrase, then choose Seal → Image or Audio."
        : "Decoy applied. Choose Seal → Image or Audio when ready."
    return true
  }

  func show(_ screen: Screen) {
    if screen != .home {
      cancelAiRequest()
      discardAiReview()
    }
    statusMessage = nil
    resendGuide = nil
    self.screen = screen
  }

  func selectImageCover(fileURL: URL) {
    guard !isWorking else { return }
    isWorking = true
    statusMessage = nil
    Task {
      defer { isWorking = false }
      do {
        imageCover = try await Task.detached(priority: .userInitiated) {
          try ImageCover.read(from: fileURL)
        }.value
      } catch {
        statusMessage = "Could not read that cover image. Choose another PNG or JPEG."
      }
    }
  }

  func removeImageCover() {
    guard !isWorking else { return }
    imageCover = nil
    statusMessage = nil
  }

  func selectAudioCover(fileURL: URL) {
    guard !isWorking else { return }
    isWorking = true
    statusMessage = nil
    Task {
      defer { isWorking = false }
      do {
        audioCover = try await Task.detached(priority: .userInitiated) {
          try AudioCover.read(from: fileURL)
        }.value
      } catch {
        statusMessage = "Could not read that cover audio. Choose another WAV."
      }
    }
  }

  func removeAudioCover() {
    guard !isWorking else { return }
    audioCover = nil
    statusMessage = nil
  }

  func sealAndCopy(as medium: SealMedium) {
    guard !isWorking else { return }
    guard !isAiWorking, aiReview == nil else {
      statusMessage =
        isAiWorking
        ? "Wait for local AI or cancel it before sealing."
        : "Use or discard the AI suggestion before sealing."
      return
    }
    guard !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
      statusMessage = selectionIssue?.message ?? "Select text in your chat, then press ⌘⇧P again."
      return
    }
    guard !hiddenMode || (!decoy.isEmpty && !passphrase.isEmpty) else {
      statusMessage =
        "Enter a decoy message and an agreed passphrase in Decoy settings before sealing."
      return
    }
    guard let contact = selectedContact else {
      statusMessage = "Import a contact public key before sealing a message."
      return
    }
    if medium == .image, let loadError = platformSelection.loadError {
      statusMessage = loadError
      return
    }
    guard
      let selectedCarrier = medium.carrier(
        imageMode: platformSelection.imageMode, audioFormat: audioFormat)
    else {
      statusMessage =
        "Choose Stealth or Robust below the platform selector before sealing an image."
      return
    }
    let guide =
      medium == .image
      ? platformSelection.imageMode.flatMap { platformSelection.guide(for: $0) } : nil

    let realMessage = message
    let decoyMessage = hiddenMode ? decoy : nil
    let hiddenPassphrase = hiddenMode ? passphrase : nil
    guard contact.algorithm != .fake else {
      statusMessage = CryptoSetupError.legacyContact.localizedDescription
      return
    }
    let recipientPublicKey = contact.publicKey
    let algorithm = contact.algorithm
    let compact = compactMessages
    let coverBytes = selectedCarrier == .stealthPng ? imageCover?.bytes : nil
    let coverAudioBytes = selectedCarrier == .musicWav ? audioCover?.bytes : nil
    let backend = service
    statusMessage = nil
    resendGuide = nil
    isWorking = true

    Task {
      do {
        let carrier = try await Task.detached(priority: .userInitiated) {
          try backend.seal(
            message: realMessage, recipientPublicKey: recipientPublicKey,
            decoy: decoyMessage, passphrase: hiddenPassphrase,
            algorithm: algorithm, compact: compact, carrier: selectedCarrier,
            coverImage: coverBytes, coverAudio: coverAudioBytes)
        }.value
        let fileURL = carrier.fileURL

        pasteboard.clearContents()
        guard pasteboard.writeObjects([fileURL as NSURL]) else {
          try? FileManager.default.removeItem(at: fileURL)
          throw MessageActionError.clipboardWriteFailed
        }
        if let previousFile = generatedFileURL {
          try? FileManager.default.removeItem(at: previousFile)
        }
        generatedFileURL = fileURL
        resultText = nil
        fileSummary = carrier.summary
        sendingGuide = guide
        passphrase = ""
        decoy = ""
        hiddenMode = false
        statusMessage =
          guide.map { "Image copied. Follow the sending steps for \($0.platformName)." }
          ?? "File copied. Paste it into the chat as a document."
        screen = .result
      } catch PolyError.NotImplemented {
        statusMessage = "\(medium.title) sealing is not available in the current backend yet."
      } catch {
        if case PolyError.CarrierTooSmall = error, algorithm == .mlkem, !compact,
          coverBytes == nil, coverAudioBytes == nil
        {
          statusMessage =
            "This carrier is too small for a full ML-KEM message. Use Stealth PNG or a custom WAV. Once the recipient opens that file, enable compact messages for smaller carriers."
        } else if case PolyError.CarrierTooSmall = error, !aiServers.isEmpty {
          statusMessage =
            "Message too large. Use Local AI to shorten it, choose a larger cover, or remove the cover."
        } else {
          statusMessage = FriendlyError.message(
            for: error, sealing: true,
            usingCover: coverBytes != nil ? .image : (coverAudioBytes != nil ? .audio : nil))
        }
      }
      isWorking = false
    }
  }

  var selectedContact: Contact? {
    contacts.first(where: { $0.id == selectedContactID })
  }

  func importContact(fileURL: URL) {
    guard !isWorking else { return }
    if pendingInvitation != nil { cancelInvitation() }
    statusMessage = nil
    isWorking = true
    Task {
      defer { isWorking = false }
      let hasFileAccess = fileURL.startAccessingSecurityScopedResource()
      defer { if hasFileAccess { fileURL.stopAccessingSecurityScopedResource() } }
      do {
        let imported = try await Task.detached(priority: .userInitiated) {
          if let invitation = try PublicKeyImage.decode(Data(contentsOf: fileURL)) {
            return ImportedContact.invitation(invitation)
          }
          return ImportedContact.file(try Contact.read(from: fileURL))
        }.value
        if case .invitation(let invitation) = imported {
          reviewInvitation(invitation)
          return
        }
        guard case .file(let contact) = imported else { return }
        var updated = contacts.filter { $0.publicKey != contact.publicKey }
        updated.append(contact)
        try contactStore.save(updated)
        contacts = updated
        selectedContactID = contact.id
        statusMessage =
          contact.algorithm == .fake
          ? CryptoSetupError.legacyContact.localizedDescription
          : "Imported contact: \(contact.name) · \(contact.algorithm.title)."
      } catch {
        statusMessage =
          (error as? PublicKeyImageError)?.localizedDescription
          ?? "Could not import that public key. Check that its algorithm and key bytes are valid."
      }
    }
  }

  func exportMyPublicKey(algorithm: CryptoAlgorithm) {
    guard !isWorking, algorithm != .fake else { return }
    let panel = NSSavePanel()
    panel.nameFieldStringValue = "Polyphonia-\(algorithm.rawValue).pub"
    panel.allowedContentTypes = [.data]
    guard panel.runModal() == .OK, let destination = panel.url else { return }
    isWorking = true
    let backend = service
    Task {
      defer { isWorking = false }
      do {
        let identity = try await Task.detached(priority: .userInitiated) {
          try backend.identity(algorithm: algorithm)
        }.value
        try Contact.writePublicKey(identity.publicKey, algorithm: algorithm, to: destination)
        addOwnContact(identity, algorithm: algorithm)
        statusMessage =
          "Exported your \(algorithm.title) public key. Share the .pub file with your contact."
      } catch {
        statusMessage = "Could not export the public key."
      }
    }
  }

  func exportPublicKeyImage(algorithm: CryptoAlgorithm, coverURL: URL? = nil) {
    guard !isWorking, algorithm != .fake else { return }
    cancelAiRequest()
    discardAiReview()
    isWorking = true
    statusMessage = nil
    let backend = service
    Task {
      defer { isWorking = false }
      do {
        let (identity, fileURL) = try await Task.detached(priority: .userInitiated) {
          let identity = try backend.identity(algorithm: algorithm)
          let cover = try coverURL.map { try ImageCover.read(from: $0).bytes }
          let image = try PublicKeyImage.encode(
            publicKey: identity.publicKey, algorithm: algorithm, cover: cover)
          let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("Image-\(UUID().uuidString).png")
          try image.write(to: url, options: .atomic)
          return (identity, url)
        }.value
        pasteboard.clearContents()
        guard pasteboard.writeObjects([fileURL as NSURL]) else {
          throw MessageActionError.clipboardWriteFailed
        }
        addOwnContact(identity, algorithm: algorithm)
        generatedFileURL = fileURL
        pendingInvitation = nil
        invitationName = ""
        resultText = nil
        fileSummary = "Public key image · \(algorithm.title)"
        sendingGuide = nil
        resendGuide = nil
        statusMessage =
          "Key image copied. Send it as an original file. They can use Open → Paste to add you."
        screen = .result
      } catch {
        statusMessage = FriendlyError.message(for: error)
      }
    }
  }

  private func reviewInvitation(_ invitation: PublicKeyInvitation) {
    pendingInvitation = invitation
    invitationName =
      contacts.first {
        $0.publicKey == invitation.publicKey && $0.algorithm == invitation.algorithm
      }?.name ?? ""
    resultText = nil
    fileSummary = nil
    passphrase = ""
    show(.contact)
  }

  func confirmInvitation() {
    guard !isWorking, let invitation = pendingInvitation else { return }
    let existing = contacts.first {
      $0.publicKey == invitation.publicKey && $0.algorithm == invitation.algorithm
    }
    let name = invitationName.trimmingCharacters(in: .whitespacesAndNewlines)
    let contact = Contact(
      id: existing?.id ?? UUID(), name: name.isEmpty ? (existing?.name ?? "New contact") : name,
      publicKey: invitation.publicKey, algorithm: invitation.algorithm)
    do {
      var updated = contacts.filter { $0.id != contact.id }
      updated.append(contact)
      try contactStore.save(updated)
      contacts = updated
      selectedContactID = contact.id
      pendingInvitation = nil
      invitationName = ""
      screen = .home
      statusMessage = "Added \(contact.name). Select a message and choose Seal to send it to them."
    } catch {
      statusMessage = "Could not save this contact. Please try again."
    }
  }

  func cancelInvitation() {
    pendingInvitation = nil
    invitationName = ""
    statusMessage = nil
    screen = .open
  }

  func open(fileURL: URL) {
    guard fileURL.isFileURL else {
      statusMessage = "Choose a file saved on this Mac."
      return
    }
    open {
      let hasFileAccess = fileURL.startAccessingSecurityScopedResource()
      defer { if hasFileAccess { fileURL.stopAccessingSecurityScopedResource() } }
      return try Data(contentsOf: fileURL)
    }
  }

  func open(imageData: Data) {
    open { imageData }
  }

  private func open(readFile: @escaping @Sendable () throws -> Data) {
    guard !isWorking else { return }
    cancelAiRequest()
    discardAiReview()
    if pendingInvitation != nil { cancelInvitation() }
    let backend = service
    let enteredPassphrase = passphrase.isEmpty ? nil : passphrase
    statusMessage = nil
    sendingGuide = nil
    resendGuide = nil
    isWorking = true

    Task {
      do {
        let received = try await Task.detached(priority: .userInitiated) {
          let file = try readFile()
          if let invitation = try PublicKeyImage.decode(file) {
            return ReceivedFile.invitation(invitation)
          }
          return ReceivedFile.message(try backend.open(file: file, passphrase: enteredPassphrase))
        }.value
        switch received {
        case .invitation(let invitation): reviewInvitation(invitation)
        case .message(let opened):
          pendingInvitation = nil
          resultText = opened.text
          fileSummary = opened.summary
          passphrase = ""
          statusMessage = nil
          screen = .result
        }
      } catch {
        showOpenError(error)
      }
      isWorking = false
    }
  }

  private func showOpenError(_ error: Error) {
    if let polyError = error as? PolyError, polyError == .ChecksumMismatch {
      statusMessage = FriendlyError.message(for: PolyError.ChecksumMismatch)
      resendGuide = platformSelection.guide(for: .stealth)
    } else {
      statusMessage = FriendlyError.message(for: error)
    }
  }

  func pasteReceivedFile() {
    guard !isWorking else { return }
    let objects = pasteboard.readObjects(
      forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
    if let fileURL = (objects as? [URL])?.first {
      open(fileURL: fileURL)
      return
    }

    // Preserve the original PNG bytes when available; bitmap conversion is only a fallback.
    if let pngData = pasteboard.data(forType: .png) {
      open(imageData: pngData)
      return
    }

    // Messaging apps often put copied images on the clipboard as image data,
    // rather than as a file URL. Convert that bitmap back to PNG for the decoder.
    if let image = (pasteboard.readObjects(forClasses: [NSImage.self], options: nil) as? [NSImage])?
      .first,
      let tiffData = image.tiffRepresentation,
      let bitmap = NSBitmapImageRep(data: tiffData),
      let pngData = bitmap.representation(using: .png, properties: [:])
    {
      open(imageData: pngData)
      return
    }

    statusMessage = "Copy an image or file first, or choose Browse."
  }

  func saveGeneratedFile() {
    guard let generatedFileURL else { return }
    let panel = NSSavePanel()
    panel.nameFieldStringValue = generatedFileURL.lastPathComponent
    panel.canCreateDirectories = true
    guard panel.runModal() == .OK, let destination = panel.url else { return }
    do {
      let data = try Data(contentsOf: generatedFileURL)
      try data.write(to: destination, options: .atomic)
      statusMessage = "Saved carrier file."
    } catch {
      statusMessage = "Could not save the file. Please try again."
    }
  }

  func copyOpenedText() {
    guard let resultText else { return }
    pasteboard.clearContents()
    pasteboard.setString(resultText, forType: .string)
    statusMessage = "Text copied."
  }

  func closeResult() {
    resultText = nil
    fileSummary = nil
    sendingGuide = nil
    resendGuide = nil
    passphrase = ""
    decoy = ""
    hiddenMode = false
    screen = .home
    statusMessage = nil
  }
}

private enum ReceivedFile: Sendable {
  case invitation(PublicKeyInvitation)
  case message(OpenedMessage)
}

private enum ImportedContact: Sendable {
  case invitation(PublicKeyInvitation)
  case file(Contact)
}

struct Contact: Identifiable, Codable, Hashable, Sendable {
  let id: UUID
  var name: String
  let publicKey: Data
  let algorithm: CryptoAlgorithm

  init(id: UUID = UUID(), name: String, publicKey: Data, algorithm: CryptoAlgorithm) {
    self.id = id
    self.name = name
    self.publicKey = publicKey
    self.algorithm = algorithm
  }

  private enum CodingKeys: String, CodingKey { case id, name, publicKey, algorithm }

  init(from decoder: Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    id = try values.decode(UUID.self, forKey: .id)
    name = try values.decode(String.self, forKey: .name)
    publicKey = try values.decode(Data.self, forKey: .publicKey)
    // Contacts saved before real crypto had only Fake keys. Never silently promote them.
    algorithm = try values.decodeIfPresent(CryptoAlgorithm.self, forKey: .algorithm) ?? .fake
  }

  static func read(from fileURL: URL) throws -> Contact {
    let lines = try String(contentsOf: fileURL, encoding: .utf8)
      .split(whereSeparator: \.isNewline)
    guard lines.count == 2 else { throw ContactFileError.invalidFormat }
    let header = lines[0].split(separator: " ")
    guard header.count == 3, header[0] == "polyphonia-public-key", header[1] == "v1",
      let algorithm = CryptoAlgorithm(rawValue: String(header[2]))
    else {
      throw ContactFileError.invalidFormat
    }
    let keyBytes = try Data(hexadecimal: String(lines[1]))
    try validatePublicKey(publicKey: keyBytes, algorithm: algorithm.core)
    let name = fileURL.deletingPathExtension().lastPathComponent
    return Contact(name: name, publicKey: keyBytes, algorithm: algorithm)
  }

  static func writePublicKey(_ publicKey: Data, algorithm: CryptoAlgorithm, to fileURL: URL) throws
  {
    let hexKey = publicKey.map { String(format: "%02x", $0) }.joined()
    let content = "polyphonia-public-key v1 \(algorithm.rawValue)\n\(hexKey)\n"
    try content.write(to: fileURL, atomically: true, encoding: .utf8)
  }
}

private struct ContactStore {
  private let storageKey = "polyphonia.demo.contacts.v1"
  let defaults: UserDefaults

  func load() throws -> [Contact] {
    guard let data = defaults.data(forKey: storageKey) else { return [] }
    return try JSONDecoder().decode([Contact].self, from: data)
  }

  func save(_ contacts: [Contact]) throws {
    let data = try JSONEncoder().encode(contacts)
    defaults.set(data, forKey: storageKey)
  }
}

private enum ContactFileError: Error {
  case invalidFormat
}

private enum MessageActionError: Error {
  case clipboardWriteFailed
}

extension Data {
  fileprivate init(hexadecimal value: String) throws {
    guard value.count.isMultiple(of: 2) else { throw ContactFileError.invalidFormat }
    var bytes = Data()
    bytes.reserveCapacity(value.count / 2)
    var cursor = value.startIndex
    while cursor < value.endIndex {
      let next = value.index(cursor, offsetBy: 2)
      guard let byte = UInt8(value[cursor..<next], radix: 16) else {
        throw ContactFileError.invalidFormat
      }
      bytes.append(byte)
      cursor = next
    }
    self = bytes
  }
}

private enum FriendlyError {
  static func message(
    for error: Error, sealing: Bool = false, usingCover: SealMedium? = nil
  ) -> String {
    guard let coreError = error as? PolyError else {
      if let image = error as? PublicKeyImageError { return image.localizedDescription }
      if let setup = error as? CryptoSetupError { return setup.localizedDescription }
      if error is CocoaError { return "The file could not be read or saved." }
      return "Polyphonia could not finish that action. Please try again."
    }

    switch coreError {
    case .ChecksumMismatch:
      return "This file was compressed in transit. Ask them to resend it as a document."
    case .WrongRecipient:
      return "This file was made for a different contact."
    case .UnknownSession:
      return
        "This Mac has no key for this compact message. Open the sender’s full message first, or ask them to turn off compact messages and resend."
    case .DecryptFailed:
      return "Could not open this file. Check the passphrase and try again."
    case .UnrecognizedFile, .BadMagic:
      return "This file does not contain a Polyphonia message."
    case .NotImplemented(let what):
      return "That option is not ready yet: \(what)"
    case .CarrierTooSmall(let needed, let available):
      return "This carrier fits \(available) bytes, but the message envelope needs \(needed). "
        + (usingCover == .image
          ? "Choose a larger image, remove the cover, or shorten the message."
          : usingCover == .audio
            ? "Choose a longer WAV, remove the cover, or shorten the message."
            : "Try a shorter message or Stealth PNG.")
    case .MessageTooLong:
      return "This message is too long. Shorten the message below and try again."
    case .Carrier where sealing:
      if usingCover == .image {
        return "Could not use this cover image. Choose another PNG or JPEG."
      }
      if usingCover == .audio {
        return "Could not use this cover audio. Choose a 16-bit PCM WAV."
      }
      return "Could not generate this carrier. Try a shorter message or another format."
    default:
      return sealing
        ? "Polyphonia could not create this carrier. Please try again."
        : "Polyphonia could not open this file. Check the file and try again."
    }
  }
}
