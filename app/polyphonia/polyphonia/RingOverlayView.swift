import SwiftUI

struct RingOverlayView: View {
  @ObservedObject var session: MessageSession
  @ObservedObject var presentation: RingPresentation
  let onDismiss: () -> Void
  let onQuit: () -> Void
  let onBrowse: () -> Void
  let onImportContact: () -> Void
  let onChooseCover: () -> Void
  let onChooseAudioCover: () -> Void

  @State private var accessoryContentHeight = RingLayout.fieldHeight
  @State private var isSealExpanded = false
  @State private var accessoryScrollRequest: AccessoryScrollRequest?
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    ZStack(alignment: .top) {
      Color.clear
        .contentShape(Rectangle())
        .onTapGesture(perform: onDismiss)

      RingMenu(
        leading: actions.leading, trailing: actions.trailing,
        isPresented: presentation.isPresented,
        isWorking: session.isWorking || session.isAiWorking || session.aiReview != nil,
        releaseRequest: presentation.releaseRequest,
        onExpansionChanged: { isSealExpanded = $0 },
        onAction: perform
      ) { hovered in
        hub(hovered: hovered)
      }
      .position(RingLayout.center)

      closeButton
        .position(x: RingLayout.center.x, y: RingLayout.closeY)

      AccessoryScrollView(
        contentWidth: RingLayout.optionsWidth,
        scrollRequest: accessoryScrollRequest,
        onContentHeightChange: { height in
          accessoryContentHeight = height
          presentation.accessoryHeight = height
        }
      ) {
        accessories
      }
      .frame(
        width: RingLayout.accessoryPanelWidth,
        height: min(
          accessoryContentHeight, RingLayout.maximumAccessoryHeight(isSealExpanded: isSealExpanded))
      )
      .clipShape(RoundedRectangle(cornerRadius: RingLayout.capsuleRadius))
      .overlay {
        RoundedRectangle(cornerRadius: RingLayout.capsuleRadius)
          .strokeBorder(RingColor.edge, lineWidth: RingLayout.hairline)
          .allowsHitTesting(false)
      }
      .padding(.top, RingLayout.accessoryY(isSealExpanded: isSealExpanded))
      .opacity(presentation.isPresented ? 1 : 0)
      .offset(y: presentation.isPresented || reduceMotion ? 0 : RingLayout.gap)
      .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: isSealExpanded)
    }
    .frame(width: RingLayout.canvasWidth, height: RingLayout.canvasHeight)
    .preferredColorScheme(.dark)
    .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: session.screen)
    .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: session.hiddenMode)
    .onChange(of: session.aiReview?.id) { _, reviewID in
      if reviewID != nil { reveal(.aiReview) }
    }
    .onChange(of: session.isAiWorking) { _, working in
      if working {
        reveal(.aiTools)
      } else if session.aiReview == nil, session.statusMessage != nil {
        reveal(.status)
      }
    }
    .onChange(of: session.isWorking) { _, working in
      if !working, session.statusMessage != nil { reveal(.status) }
    }
    .onChange(of: session.screen) { _, _ in reveal(.status) }
    .dropDestination(for: URL.self) { files, _ in
      guard let file = files.first, !session.isWorking else { return false }
      session.open(fileURL: file)
      return true
    }
    .accessibilityHidden(!presentation.isPresented)
  }

  private func reveal(_ target: AccessoryTarget) {
    accessoryScrollRequest = AccessoryScrollRequest(target: target)
  }

  private var actions: (leading: RingAction, trailing: RingAction) {
    switch session.screen {
    case .home: (.seal, .open)
    case .open: (.paste, .browse)
    case .result: (session.resultText == nil ? .save : .copy, .done)
    }
  }

  private func perform(_ action: RingAction) {
    switch action {
    // Seal expands the ring; its children perform encryption.
    case .seal: break
    case .sealImage: session.sealAndCopy(as: .image)
    case .sealAudio: session.sealAndCopy(as: .audio)
    case .open:
      session.passphrase = ""
      session.hiddenMode = false
      session.show(.open)
    case .paste: session.pasteReceivedFile()
    case .browse: onBrowse()
    case .save: session.saveGeneratedFile()
    case .copy: session.copyOpenedText()
    case .done: onDismiss()
    }
  }

  @ViewBuilder
  private func hub(hovered: RingAction?) -> some View {
    VStack(spacing: RingLayout.gap) {
      if session.isWorking || session.isAiWorking {
        ProgressView().controlSize(.small)
        Text(session.isAiWorking ? "Asking local AI…" : "Working…")
          .font(.system(size: RingLayout.captionSize))
      } else if session.aiReview != nil {
        Image(systemName: "sparkles")
        Text("Review suggestion below")
          .font(.system(size: RingLayout.captionSize))
      } else {
        switch session.screen {
        case .home:
          Image(systemName: hovered?.symbol ?? "waveform")
            .font(.system(size: RingLayout.titleSize, weight: .light))
            .foregroundStyle(
              hovered == .seal || hovered == .sealImage || hovered == .sealAudio
                ? RingColor.seal : RingColor.ink
            )
            .contentTransition(.symbolEffect(.replace))
          Text(hovered?.title ?? "Polyphonia")
            .font(.system(size: RingLayout.titleSize, weight: .medium))
            .contentTransition(.opacity)
          Text(homeHint(hovered: hovered))
            .font(.system(size: RingLayout.tinySize))
            .foregroundStyle(RingColor.secondary)
            .lineLimit(2)
            .contentTransition(.opacity)
        case .open:
          Image(systemName: "tray.and.arrow.down")
            .font(.system(size: RingLayout.titleSize, weight: .light))
          Text("Drop a file").font(.system(size: RingLayout.titleSize, weight: .medium))
          Text("or paste / browse")
            .font(.system(size: RingLayout.tinySize)).foregroundStyle(RingColor.secondary)
        case .result:
          if let text = session.resultText {
            Text("OPENED").font(.system(size: RingLayout.tinySize, weight: .medium))
              .tracking(1).foregroundStyle(RingColor.secondary)
            ScrollView(showsIndicators: false) {
              Text(text).font(.system(size: RingLayout.captionSize)).textSelection(.enabled)
            }
          } else {
            Image(systemName: "checkmark").foregroundStyle(RingColor.success)
            Text("File copied").font(.system(size: RingLayout.titleSize, weight: .medium))
            Text(session.sendingGuide == nil ? "Paste into your chat" : "Follow the sending steps")
              .font(.system(size: RingLayout.tinySize)).foregroundStyle(RingColor.secondary)
          }
        }
      }
    }
    .multilineTextAlignment(.center)
    .foregroundStyle(RingColor.ink)
    .padding(RingLayout.hubPadding)
    .id(session.screen)
    .transition(.opacity)
  }

  private func homeHint(hovered: RingAction?) -> String {
    switch hovered {
    case .seal:
      "Move out to Image or Audio"
    case .sealImage, .sealAudio:
      session.message.isEmpty
        ? "Selection unavailable" : "\(session.message.count) characters selected"
    case .open: "From your clipboard or a file"
    default: "Swipe, then release"
    }
  }

  private var closeButton: some View {
    Button(action: onDismiss) {
      Image(systemName: "xmark")
        .font(.system(size: RingLayout.captionSize, weight: .medium))
        .foregroundStyle(RingColor.secondary)
        .frame(width: RingLayout.closeDiameter, height: RingLayout.closeDiameter)
        .background { RingGlass().clipShape(Circle()) }
    }
    .buttonStyle(.plain)
    .help("Close ring · Escape")
    .accessibilityLabel("Close ring")
    .opacity(presentation.isPresented ? 1 : 0)
    .contextMenu {
      Button("Close ring", action: onDismiss)
      Button("Quit Polyphonia", action: onQuit)
    }
  }

  private var accessories: some View {
    VStack(spacing: RingLayout.gap) {
      if let status = session.statusMessage {
        Text(status)
          .font(.system(size: RingLayout.captionSize))
          .foregroundStyle(RingColor.ink)
          .multilineTextAlignment(.center)
          .padding(RingLayout.gap)
          .background { accessoryGlass }
          .accessoryTarget(.status)
      }
      switch session.screen {
      case .home:
        HStack(spacing: RingLayout.padding) {
          contactMenu
          Button {
            reveal(.decoySettings)
          } label: {
            Label("Decoy", systemImage: session.hiddenMode ? "eye.slash.fill" : "eye.slash")
              .foregroundStyle(session.hiddenMode ? RingColor.seal : RingColor.secondary)
          }
          .buttonStyle(.plain)
          .help("Learn about decoys and configure a decoy message and passphrase")
          .accessibilityLabel("Decoy settings")
        }
        .font(.system(size: RingLayout.captionSize))
        .padding(.horizontal, RingLayout.padding)
        .frame(height: RingLayout.fieldHeight)
        .background { accessoryGlass }

        VStack(alignment: .leading, spacing: RingLayout.smallGap) {
          HStack {
            Label(
              session.selectedContact?.algorithm.title ?? "ML-KEM-768", systemImage: "lock.shield")
            Spacer()
            Toggle("Compact", isOn: $session.compactMessages)
              .toggleStyle(.checkbox)
              .help("Use compact messages after the recipient opens your full message")
              .accessibilityLabel("Use compact messages")
          }
          Text(
            session.compactMessages
              ? "The recipient must open your last full message first. Sending sessions last 7 days."
              : session.selectedContact?.algorithm == .mlkem
                ? "Full: use Stealth PNG or a custom WAV. After they open it, compact messages fit generated audio and smaller images."
                : "Full message: starts a fresh session and works without earlier files."
          )
          .foregroundStyle(RingColor.secondary)
          if session.compactMessages {
            Button("Start a new session") { session.compactMessages = false }
              .buttonStyle(.plain)
              .foregroundStyle(RingColor.seal)
          }
        }
        .font(.system(size: RingLayout.captionSize))
        .padding(RingLayout.padding)
        .background { accessoryGlass }

        MessageDraftControls(session: session, onReveal: reveal)

        PlatformControls(selection: session.platformSelection)
        ImageCoverControls(
          selection: session.platformSelection, coverName: session.imageCover?.name,
          onChoose: onChooseCover, onRemove: session.removeImageCover)
        AudioFormatPicker(selection: $session.audioFormat)
        AudioCoverControls(
          format: session.audioFormat, coverName: session.audioCover?.name,
          onChoose: onChooseAudioCover, onRemove: session.removeAudioCover)

      case .open:
        PlatformPicker(selection: session.platformSelection)
        SecureField("Passphrase · optional", text: $session.passphrase)
          .textFieldStyle(RingFieldStyle())
        Text(
          "If the sender used a decoy, enter the agreed passphrase to read the real message. Leave it empty to read the decoy."
        )
        .font(.system(size: RingLayout.captionSize))
        .foregroundStyle(RingColor.secondary)
        Button("Back to Seal / Open") { session.show(.home) }
          .buttonStyle(.plain)
          .font(.system(size: RingLayout.captionSize))
          .foregroundStyle(RingColor.secondary)
      case .result:
        if let summary = session.fileSummary {
          Text(summary)
            .font(.system(size: RingLayout.captionSize))
            .foregroundStyle(RingColor.secondary)
            .padding(RingLayout.gap)
            .background { accessoryGlass }
        }
        if let guide = session.sendingGuide {
          SendingGuideButton(guide: guide, showOnAppear: true)
        }
      }

      if let guide = session.resendGuide {
        SendingGuideButton(guide: guide, title: "How to resend as a file", showOnAppear: true)
          .id(guide.id)
      }

      if session.screen == .home, session.selectionIssue?.needsPermission == true {
        Button("Open Accessibility Settings") {
          guard
            let url = URL(
              string:
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
          else { return }
          NSWorkspace.shared.open(url)
        }
        .buttonStyle(.plain)
        .font(.system(size: RingLayout.captionSize))
        .foregroundStyle(RingColor.seal)
      }

      if session.screen == .home {
        Text(session.selectedContact?.algorithm.title ?? "ML-KEM-768")
          .font(.system(size: RingLayout.tinySize, weight: .medium))
          .tracking(1)
          .foregroundStyle(
            session.selectedContact?.algorithm == .fake ? RingColor.warning : RingColor.secondary)
      }
    }
    .disabled(session.isWorking)
  }

  private var contactMenu: some View {
    Menu {
      ForEach(session.contacts) { contact in
        Button {
          session.selectedContactID = contact.id
        } label: {
          if contact.id == session.selectedContactID {
            Label("\(contact.name) · \(contact.algorithm.title)", systemImage: "checkmark")
          } else {
            Text("\(contact.name) · \(contact.algorithm.title)")
          }
        }
      }
      Divider()
      Button("Import public key…", action: onImportContact)
      Menu("Export my public key") {
        ForEach(CryptoAlgorithm.secureAlgorithms, id: \.self) { algorithm in
          Button(algorithm.title + "…") { session.exportMyPublicKey(algorithm: algorithm) }
        }
      }
      Divider()
      Button("Quit Polyphonia", action: onQuit)
    } label: {
      Label(session.selectedContact?.name ?? "Choose contact", systemImage: "person.crop.circle")
        .lineLimit(1)
        .foregroundStyle(RingColor.secondary)
    }
    .menuStyle(.borderlessButton)
  }

  private var accessoryGlass: some View {
    RingAccessoryGlass()
  }
}

struct RingFieldStyle: TextFieldStyle {
  func _body(configuration: TextField<Self._Label>) -> some View {
    configuration
      .font(.system(size: RingLayout.captionSize))
      .textFieldStyle(.plain)
      .padding(.horizontal, RingLayout.padding)
      .frame(height: RingLayout.fieldHeight)
      .background {
        RingGlass().overlay(RingColor.smoke).clipShape(Capsule())
          .overlay(Capsule().strokeBorder(RingColor.edge, lineWidth: RingLayout.hairline))
      }
  }
}
