import SwiftUI

struct RingOverlayView: View {
  @ObservedObject var session: MessageSession
  @ObservedObject var presentation: RingPresentation
  let onDismiss: () -> Void
  let onQuit: () -> Void
  let onBrowse: () -> Void
  let onImportContact: () -> Void
  let onChooseCover: () -> Void

  @State private var accessoryContentHeight = RingLayout.fieldHeight
  @State private var isSealExpanded = false
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    ZStack(alignment: .top) {
      Color.clear
        .contentShape(Rectangle())
        .onTapGesture(perform: onDismiss)

      RingMenu(
        leading: actions.leading, trailing: actions.trailing,
        isPresented: presentation.isPresented, isWorking: session.isWorking,
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
        onContentHeightChange: { height in
          accessoryContentHeight = height
          presentation.accessoryHeight = height
        }
      ) {
        accessories
      }
      .frame(
        width: RingLayout.optionsWidth,
        height: min(
          accessoryContentHeight, RingLayout.maximumAccessoryHeight(isSealExpanded: isSealExpanded))
      )
      .padding(.top, RingLayout.accessoryY(isSealExpanded: isSealExpanded))
      .opacity(presentation.isPresented ? 1 : 0)
      .offset(y: presentation.isPresented || reduceMotion ? 0 : RingLayout.gap)
      .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: isSealExpanded)
    }
    .frame(width: RingLayout.canvasWidth, height: RingLayout.canvasHeight)
    .preferredColorScheme(.dark)
    .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: session.screen)
    .animation(reduceMotion ? RingMotion.reduced : RingMotion.change, value: session.hiddenMode)
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
      if session.isWorking {
        ProgressView().controlSize(.small)
        Text("Working…").font(.system(size: RingLayout.captionSize))
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
      switch session.screen {
      case .home:
        HStack(spacing: RingLayout.padding) {
          contactMenu
          Button {
            session.hiddenMode.toggle()
          } label: {
            Label("Hidden", systemImage: session.hiddenMode ? "eye.slash.fill" : "eye.slash")
              .foregroundStyle(session.hiddenMode ? RingColor.seal : RingColor.secondary)
          }
          .buttonStyle(.plain)
          .help("Configure a decoy and passphrase")
        }
        .font(.system(size: RingLayout.captionSize))
        .padding(.horizontal, RingLayout.padding)
        .frame(height: RingLayout.fieldHeight)
        .background { accessoryGlass }

        PlatformControls(selection: session.platformSelection)
        ImageCoverControls(
          selection: session.platformSelection, coverName: session.imageCover?.name,
          onChoose: onChooseCover, onRemove: session.removeImageCover)
        AudioFormatPicker(selection: $session.audioFormat)

        if session.hiddenMode {
          TextField("Decoy message", text: $session.decoy)
            .textFieldStyle(RingFieldStyle())
          SecureField("Agreed passphrase", text: $session.passphrase)
            .textFieldStyle(RingFieldStyle())
        }
      case .open:
        PlatformPicker(selection: session.platformSelection)
        SecureField("Passphrase · optional", text: $session.passphrase)
          .textFieldStyle(RingFieldStyle())
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

      if let status = session.statusMessage {
        Text(status)
          .font(.system(size: RingLayout.captionSize))
          .foregroundStyle(RingColor.ink)
          .multilineTextAlignment(.center)
          .padding(RingLayout.gap)
          .background { accessoryGlass }
          .transition(.opacity)
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

      Text("FAKE CIPHER · NOT SECURE")
        .font(.system(size: RingLayout.tinySize, weight: .medium))
        .tracking(1)
        .foregroundStyle(RingColor.warning.opacity(0.8))
        .shadow(color: .black.opacity(0.6), radius: RingLayout.smallGap)
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
            Label(contact.name, systemImage: "checkmark")
          } else {
            Text(contact.name)
          }
        }
      }
      Divider()
      Button("Import public key…", action: onImportContact)
      Button("Export my public key…", action: session.exportMyPublicKey)
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

private struct RingFieldStyle: TextFieldStyle {
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
