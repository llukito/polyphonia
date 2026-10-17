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

  @State private var selectedMedium: SealMedium?
  @State private var settingsPanel: RingSettingsPanel?
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    ZStack(alignment: .top) {
      Color.clear
        .contentShape(Rectangle())
        .onTapGesture(perform: onDismiss)

      if let selectedMedium, !ringOptions.isEmpty {
        SealOptionsRing(
          medium: selectedMedium, options: ringOptions,
          isWorking: session.isWorking || session.isAiWorking,
          isSelected: optionIsSelected, onSelect: selectOption)
          .position(RingLayout.center)
      }

      RingMenu(
        leading: actions.leading, trailing: actions.trailing,
        isPresented: presentation.isPresented,
        isWorking: session.isWorking || session.isAiWorking,
        utilities: utilityActions,
        showBranches: selectedMedium != nil,
        releaseRequest: presentation.releaseRequest,
        onHoverChanged: updateMedium,
        onOuterRelease: releaseOuterOption,
        onAction: perform
      ) { hovered in
        hub(hovered: hovered)
      }
      .position(RingLayout.center)

      closeButton
        .position(x: RingLayout.center.x, y: RingLayout.closeY)

    }
    .frame(width: RingLayout.canvasWidth, height: RingLayout.canvasHeight)
    .preferredColorScheme(.dark)
    .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: session.screen)
    .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: session.hiddenMode)
    .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: selectedMedium)
    .sheet(item: $settingsPanel) { panel in
      RingSettingsCard(
        panel: panel, session: session,
        onImportContact: {
          settingsPanel = nil
          DispatchQueue.main.async(execute: onImportContact)
        },
        onDone: { settingsPanel = nil })
    }
    .sheet(item: $session.aiReview) { review in
      AiReviewCard(
        review: review,
        onAccept: session.applyAiReview,
        onCancel: { session.aiReview = nil })
    }
    .onChange(of: presentation.isPresented) { _, shown in
      if !shown { selectedMedium = nil; settingsPanel = nil }
    }
    .onChange(of: session.screen) { _, screen in
      if screen != .home { selectedMedium = nil }
      if screen == .result, session.sendingGuide != nil { settingsPanel = .guide }
    }
    .dropDestination(for: URL.self) { files, _ in
      guard let file = files.first, !session.isWorking else { return false }
      session.open(fileURL: file)
      return true
    }
    .accessibilityHidden(!presentation.isPresented)
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
    case .sealImage: selectedMedium = .image
    case .sealAudio: selectedMedium = .audio
    case .open:
      session.passphrase = ""
      session.hiddenMode = false
      selectedMedium = nil
      session.show(.open)
    case .paste: session.pasteReceivedFile()
    case .browse: onBrowse()
    case .save: session.saveGeneratedFile()
    case .copy: session.copyOpenedText()
    case .done: onDismiss()
    case .recipient: settingsPanel = .contacts
    case .hidden: settingsPanel = .hidden
    case .ai:
      settingsPanel = .ai
    case .cancelAi: session.cancelAiRequest()
    case .sealFile:
      if let selectedMedium { session.sealAndCopy(as: selectedMedium) }
    case .passphrase: settingsPanel = .passphrase
    case .back: session.show(.home)
    case .guide: settingsPanel = .guide
    }
  }

  private func updateMedium(_ hovered: RingAction?) {
    guard session.screen == .home else { return }
    switch hovered {
    case .sealImage: selectedMedium = .image
    case .sealAudio: selectedMedium = .audio
    case .open: selectedMedium = nil
    default: break
    }
  }

  private func releaseOuterOption(_ point: CGPoint) {
    guard let selectedMedium else { return }
    let offset = RingLayout.optionsOuterRadius - RingLayout.interactionDiameter / 2
    let optionPoint = CGPoint(x: point.x + offset, y: point.y + offset)
    if let option = SealOptionsRing.option(
      at: optionPoint, medium: selectedMedium, options: ringOptions)
    { selectOption(option) }
  }

  @ViewBuilder
  private func hub(hovered: RingAction?) -> some View {
    VStack(spacing: RingLayout.gap) {
      if session.isWorking || session.isAiWorking {
        ProgressView().controlSize(.small)
        Text(session.isAiWorking ? "Asking local AI…" : "Working…")
          .font(.system(size: RingLayout.captionSize))
      } else if let status = session.statusMessage, session.screen != .result {
        Image(systemName: "exclamationmark.circle")
          .foregroundStyle(RingColor.warning)
        Text(status)
          .font(.system(size: RingLayout.tinySize))
          .lineLimit(5)
      } else {
        switch session.screen {
        case .home:
          Image(systemName: selectedMedium == .image ? "photo" :
                selectedMedium == .audio ? "waveform" : hovered?.symbol ?? "waveform")
            .font(.system(size: RingLayout.titleSize, weight: .light))
            .foregroundStyle(
              hovered == .seal || hovered == .sealImage || hovered == .sealAudio
                ? RingColor.seal : RingColor.ink
            )
            .contentTransition(.symbolEffect(.replace))
          Text(selectedMedium?.title ?? hovered?.title ?? "Polyphonia")
            .font(.system(size: RingLayout.titleSize, weight: .medium))
            .contentTransition(.opacity)
          Text(homeHint(hovered: hovered))
            .font(.system(size: RingLayout.tinySize))
            .foregroundStyle(RingColor.secondary)
            .lineLimit(2)
            .contentTransition(.opacity)
          Text("FAKE CIPHER")
            .font(.system(size: RingLayout.tinySize, weight: .medium))
            .foregroundStyle(RingColor.warning)
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
    if let selectedMedium {
      let format = selectedMedium == .image
        ? (session.platformSelection.imageMode?.title ?? "Choose mode")
        : session.audioFormat.title
      return "\(format) · \(session.message.utf8.count) bytes"
    }
    return switch hovered {
    case .seal:
      "Move out to Image or Audio"
    case .sealImage, .sealAudio:
      session.message.isEmpty ? "Selection unavailable" : "Choose to open settings"
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

  private var ringOptions: [RingOption] {
    switch session.screen {
    case .home:
      guard let selectedMedium else { return [] }
      var options: [RingOption] = selectedMedium == .image
        ? [.stealth, .robust, .cover]
        : [.wav, .midi, .cover]
      let hasCover = selectedMedium == .image
        ? session.imageCover != nil : session.audioCover != nil
      if hasCover {
        options.append(.removeCover)
      }
      if selectedMedium == .image { options.append(.platform) }
      return options
    case .open, .result: return []
    }
  }

  private var utilityActions: [RingAction] {
    switch session.screen {
    case .home:
      guard selectedMedium != nil else { return [] }
      var actions: [RingAction] = [.recipient, .hidden]
      if session.isAiWorking { actions.append(.cancelAi) }
      else if !session.aiServers.isEmpty { actions.append(.ai) }
      actions.append(.sealFile)
      return actions
    case .open: return [.passphrase, .back]
    case .result:
      return session.sendingGuide != nil || session.resendGuide != nil ? [.guide] : []
    }
  }

  private func optionIsSelected(_ option: RingOption) -> Bool {
    switch option {
    case .stealth: session.platformSelection.imageMode == .stealth
    case .robust: session.platformSelection.imageMode == .robust
    case .wav: session.audioFormat == .wav
    case .midi: session.audioFormat == .midi
    case .cover:
      selectedMedium == .image ? session.imageCover != nil : session.audioCover != nil
    default: false
    }
  }

  private func selectOption(_ option: RingOption) {
    switch option {
    case .stealth: session.platformSelection.selectMode(.stealth)
    case .robust: session.platformSelection.selectMode(.robust)
    case .wav: session.audioFormat = .wav
    case .midi: session.audioFormat = .midi
    case .cover:
      if selectedMedium == .image {
        session.platformSelection.selectMode(.stealth)
        onChooseCover()
      } else {
        session.audioFormat = .wav
        onChooseAudioCover()
      }
    case .removeCover:
      if selectedMedium == .image { session.removeImageCover() }
      else { session.removeAudioCover() }
    case .platform: settingsPanel = .platform
    }
  }
}
