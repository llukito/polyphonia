import SwiftUI

enum RingSettingsPanel: String, Identifiable {
  case contacts, platform, hidden, ai, passphrase, guide
  var id: String { rawValue }
}

struct RingSettingsCard: View {
  let panel: RingSettingsPanel
  @ObservedObject var session: MessageSession
  let onImportContact: () -> Void
  let onDone: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      HStack {
        Text(title).font(.headline)
        Spacer()
        Button("Done", action: onDone)
      }
      switch panel {
      case .contacts: contacts
      case .platform: platform
      case .hidden: hidden
      case .ai: ai
      case .passphrase: passphrase
      case .guide:
        if let guide = session.sendingGuide ?? session.resendGuide {
          SendingGuideView(guide: guide)
        }
      }
    }
    .padding(20)
    .frame(width: panel == .guide ? RingLayout.guideWidth + 40 : 380)
    .preferredColorScheme(.dark)
  }

  private var title: String {
    switch panel {
    case .contacts: "Recipient"
    case .platform: "Destination platform"
    case .hidden: "Hidden message"
    case .ai: "Local AI"
    case .passphrase: "Open with passphrase"
    case .guide: "Sending steps"
    }
  }

  private var contacts: some View {
    VStack(alignment: .leading, spacing: 10) {
      ForEach(session.contacts) { contact in
        Button {
          session.selectedContactID = contact.id
          onDone()
        } label: {
          Label(contact.name, systemImage: contact.id == session.selectedContactID
                ? "checkmark.circle.fill" : "circle")
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .buttonStyle(.plain)
      }
      Divider()
      Button("Import public key…", action: onImportContact)
      Button("Export my public key…") {
        onDone()
        DispatchQueue.main.async { session.exportMyPublicKey() }
      }
    }
  }

  private var platform: some View {
    VStack(alignment: .leading, spacing: 12) {
      if let error = session.platformSelection.loadError {
        Text(error).foregroundStyle(RingColor.warning)
      } else {
        Menu {
          ForEach(session.platformSelection.profiles?.platforms ?? []) { profile in
            Button(profile.name) { session.platformSelection.selectPlatform(profile.id) }
          }
          Divider()
          Button("Other") {
            session.platformSelection.selectPlatform(PlatformProfiles.otherID)
          }
        } label: {
          Label(session.platformSelection.platformName, systemImage: "paperplane")
        }
        Text("Choose Stealth or Robust on the outer ring. The destination suggests a mode; you can change it.")
          .font(.caption).foregroundStyle(.secondary)
        if let mode = session.platformSelection.imageMode,
           let guide = session.platformSelection.guide(for: mode) {
          SendingGuideButton(guide: guide)
        }
      }
    }
  }

  private var hidden: some View {
    VStack(alignment: .leading, spacing: 12) {
      Toggle("Use a hidden message", isOn: $session.hiddenMode)
      if session.hiddenMode {
        TextField("Decoy message", text: $session.decoy)
        SecureField("Agreed passphrase", text: $session.passphrase)
        Text("Without the passphrase, the recipient sees the decoy.")
          .font(.caption).foregroundStyle(.secondary)
      }
    }
    .textFieldStyle(.roundedBorder)
  }

  private var ai: some View {
    VStack(alignment: .leading, spacing: 12) {
      Menu {
        ForEach(session.aiServers, id: \.baseUrl) { server in
          Button(server.kind == .ollama ? "Ollama" : "LM Studio") {
            session.selectedAiServerURL = server.baseUrl
            session.selectedAiModel = server.models.first
          }
        }
      } label: {
        Label(session.selectedAiServer?.kind == .ollama ? "Ollama" : "LM Studio",
              systemImage: "desktopcomputer")
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
      Text("Suggestions stay on this Mac and need your approval.")
        .font(.caption).foregroundStyle(.secondary)
      HStack {
        Button("Shorten message") {
          onDone()
          session.requestShortening()
        }
        Button("Generate decoy") {
          onDone()
          session.requestDecoy()
        }
      }
      .disabled(session.selectedAiModel == nil)
    }
  }

  private var passphrase: some View {
    VStack(alignment: .leading, spacing: 8) {
      SecureField("Passphrase · optional", text: $session.passphrase)
        .textFieldStyle(.roundedBorder)
      Text("Leave empty to open the ordinary message.")
        .font(.caption).foregroundStyle(.secondary)
    }
  }
}
