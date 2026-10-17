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
  }

  @Published private(set) var screen: Screen = .home
  @Published var message = ""
  @Published var decoy = ""
  @Published var passphrase = ""
  @Published var hiddenMode = false
  @Published var audioFormat: AudioFormat = .wav
  @Published private(set) var imageCover: ImageCover?
  @Published private(set) var audioCover: AudioCover?
  @Published var isWorking = false
  @Published var statusMessage: String?
  @Published private(set) var resultText: String?
  @Published private(set) var fileSummary: String?
  @Published private(set) var selectionIssue: SelectionIssue?
  @Published private(set) var sendingGuide: SendingGuide?
  @Published private(set) var resendGuide: SendingGuide?
  @Published private(set) var contacts: [DemoContact] = []
  @Published var selectedContactID: UUID?
  let platformSelection = PlatformSelection()

  private let service: PolyphoniaService
  private let contactStore = ContactStore()
  private var generatedFileURL: URL?

  init(service: PolyphoniaService) {
    self.service = service
    do {
      contacts = try contactStore.load()
      let identity = try service.demoIdentity()
      if !contacts.contains(where: { $0.publicKey == identity.publicKey }) {
        let ownContact = DemoContact(
          name: "This Mac · demo identity", publicKey: identity.publicKey)
        contacts.insert(ownContact, at: 0)
        try contactStore.save(contacts)
      }
      selectedContactID = contacts.first?.id
    } catch {
      statusMessage = "Could not load the demo identity from Keychain."
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
    imageCover = nil
    audioCover = nil
    screen = .home
  }

  func show(_ screen: Screen) {
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
    guard !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
      statusMessage = selectionIssue?.message ?? "Select text in your chat, then press ⌘⇧P again."
      return
    }
    guard !hiddenMode || (!decoy.isEmpty && !passphrase.isEmpty) else {
      statusMessage = "Hidden mode needs a decoy message and passphrase."
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
    let recipientPublicKey = contact.publicKey
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
            decoy: decoyMessage, passphrase: hiddenPassphrase, carrier: selectedCarrier,
            coverImage: coverBytes, coverAudio: coverAudioBytes)
        }.value
        let fileURL = carrier.fileURL

        NSPasteboard.general.clearContents()
        guard NSPasteboard.general.writeObjects([fileURL as NSURL]) else {
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
        statusMessage = FriendlyError.message(
          for: error, sealing: true,
          usingCover: coverBytes != nil ? .image : (coverAudioBytes != nil ? .audio : nil))
      }
      isWorking = false
    }
  }

  var selectedContact: DemoContact? {
    contacts.first(where: { $0.id == selectedContactID })
  }

  func importContact(fileURL: URL) {
    let hasFileAccess = fileURL.startAccessingSecurityScopedResource()
    defer { if hasFileAccess { fileURL.stopAccessingSecurityScopedResource() } }

    do {
      let contact = try DemoContact.read(from: fileURL)
      contacts.removeAll(where: { $0.publicKey == contact.publicKey })
      contacts.append(contact)
      try contactStore.save(contacts)
      selectedContactID = contact.id
      statusMessage = "Imported contact: \(contact.name)."
    } catch {
      statusMessage = "Could not import that public key file."
    }
  }

  func exportMyPublicKey() {
    do {
      let identity = try service.demoIdentity()
      let panel = NSSavePanel()
      panel.nameFieldStringValue = "Polyphonia-demo.pub"
      panel.allowedContentTypes = [.data]
      guard panel.runModal() == .OK, let destination = panel.url else { return }
      try DemoContact.writePublicKey(identity.publicKey, to: destination)
      statusMessage = "Exported your public key. Share the .pub file with your contact."
    } catch {
      statusMessage = "Could not export the public key."
    }
  }

  func open(fileURL: URL) {
    guard !isWorking else { return }
    guard fileURL.isFileURL else {
      statusMessage = "Choose a file saved on this Mac."
      return
    }
    let backend = service
    let enteredPassphrase = passphrase.isEmpty ? nil : passphrase
    statusMessage = nil
    sendingGuide = nil
    resendGuide = nil
    isWorking = true

    Task {
      do {
        let hasFileAccess = fileURL.startAccessingSecurityScopedResource()
        defer { if hasFileAccess { fileURL.stopAccessingSecurityScopedResource() } }
        let fileData = try await Task.detached(priority: .userInitiated) {
          try Data(contentsOf: fileURL)
        }.value
        let opened = try await Task.detached(priority: .userInitiated) {
          try backend.open(file: fileData, passphrase: enteredPassphrase)
        }.value
        resultText = opened.text
        fileSummary = opened.summary
        passphrase = ""
        statusMessage = nil
        screen = .result
      } catch PolyError.ChecksumMismatch {
        statusMessage = FriendlyError.message(for: PolyError.ChecksumMismatch)
        resendGuide = platformSelection.guide(for: .stealth)
      } catch {
        statusMessage = FriendlyError.message(for: error)
      }
      isWorking = false
    }
  }

  func pasteReceivedFile() {
    let objects = NSPasteboard.general.readObjects(
      forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
    guard let fileURL = (objects as? [URL])?.first else {
      statusMessage = "Copy a received file first, or choose Browse."
      return
    }
    open(fileURL: fileURL)
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
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(resultText, forType: .string)
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

struct DemoContact: Identifiable, Codable, Hashable, Sendable {
  let id: UUID
  var name: String
  let publicKey: Data

  init(id: UUID = UUID(), name: String, publicKey: Data) {
    self.id = id
    self.name = name
    self.publicKey = publicKey
  }

  static func read(from fileURL: URL) throws -> DemoContact {
    let lines = try String(contentsOf: fileURL, encoding: .utf8)
      .split(whereSeparator: \.isNewline)
    guard lines.count >= 2, lines[0] == "polyphonia-public-key v1 fake" else {
      throw ContactFileError.invalidFormat
    }
    let keyBytes = try Data(hexadecimal: String(lines[1]))
    guard !keyBytes.isEmpty else { throw ContactFileError.invalidFormat }
    let name = fileURL.deletingPathExtension().lastPathComponent
    return DemoContact(name: name, publicKey: keyBytes)
  }

  static func writePublicKey(_ publicKey: Data, to fileURL: URL) throws {
    let hexKey = publicKey.map { String(format: "%02x", $0) }.joined()
    let content = "polyphonia-public-key v1 fake\n\(hexKey)\n"
    try content.write(to: fileURL, atomically: true, encoding: .utf8)
  }
}

private struct ContactStore {
  private let storageKey = "polyphonia.demo.contacts.v1"

  func load() throws -> [DemoContact] {
    guard let data = UserDefaults.standard.data(forKey: storageKey) else { return [] }
    return try JSONDecoder().decode([DemoContact].self, from: data)
  }

  func save(_ contacts: [DemoContact]) throws {
    let data = try JSONEncoder().encode(contacts)
    UserDefaults.standard.set(data, forKey: storageKey)
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
      if error is CocoaError { return "The file could not be read or saved." }
      return "Polyphonia could not finish that action. Please try again."
    }

    switch coreError {
    case .ChecksumMismatch:
      return "This file was compressed in transit. Ask them to resend it as a document."
    case .WrongRecipient:
      return "This file was made for a different contact."
    case .DecryptFailed:
      return "Could not open this file. Check the passphrase and try again."
    case .UnrecognizedFile, .BadMagic:
      return "This file does not contain a Polyphonia message."
    case .NotImplemented(let what):
      return "That demo option is not ready yet: \(what)"
    case .CarrierTooSmall(let needed, let available):
      return "This carrier fits \(available) bytes, but the message envelope needs \(needed). "
        + (usingCover == .image
          ? "Choose a larger image, remove the cover, or shorten the message."
          : usingCover == .audio
            ? "Choose a longer WAV, remove the cover, or shorten the message."
            : "Try a shorter message or Stealth PNG.")
    case .MessageTooLong:
      return "This message is too long. Select a shorter message and try again."
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
