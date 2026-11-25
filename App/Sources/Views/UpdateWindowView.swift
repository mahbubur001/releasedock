import ReleaseDockCore
import SwiftUI

/// Content of the Software Update window: what's new since the installed version,
/// then download, install and relaunch.
struct UpdateWindowView: View {
    @Environment(SparkleUpdateService.self) private var updater
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(spacing: 0) {
            switch updater.phase {
            case .checking:
                message(title: "Checking for updates…", detail: nil) {
                    ProgressView().controlSize(.small)
                    Spacer()
                    Button("Cancel") { updater.cancel() }
                }
            case .upToDate:
                message(title: "ReleaseDock is up to date", detail: "Version \(AppModel.appVersion) is the newest version available.") {
                    Spacer()
                    Button("OK") { updater.acknowledge() }.keyboardShortcut(.defaultAction)
                }
            case .failed(let error):
                message(title: "The update could not be installed", detail: error) {
                    Spacer()
                    Button("OK") { updater.acknowledge() }.keyboardShortcut(.defaultAction)
                }
            default:
                if let update = updater.available {
                    header(update)
                    notes(update)
                    trust
                    footer(update)
                }
            }
        }
        .frame(width: 600, height: 560)
        .animation(.easeOut(duration: 0.2), value: updater.phase)
    }

    // MARK: Pieces

    private func message<Actions: View>(title: String, detail: String?, @ViewBuilder actions: () -> Actions) -> some View {
        VStack(spacing: 0) {
            Spacer()
            AppIcon(size: 96)
            Text(title).font(.title2.weight(.bold)).padding(.top, 14)
            if let detail {
                Text(detail).foregroundStyle(.secondary).multilineTextAlignment(.center).padding(.top, 4).padding(.horizontal, 40)
            }
            Spacer()
            HStack(spacing: 8) { actions() }
                .padding(.horizontal, 26)
                .padding(.bottom, 20)
        }
    }

    private func header(_ update: AvailableUpdate) -> some View {
        HStack(spacing: 16) {
            AppIcon(size: 64)
            VStack(alignment: .leading, spacing: 3) {
                Text(headline(update)).font(.title2.weight(.bold)).tracking(-0.3)
                Text(subtitle(update)).foregroundStyle(.secondary)
                HStack(spacing: 6) {
                    Text("\(AppModel.appVersion) installed")
                        .padding(.horizontal, 8).padding(.vertical, 1)
                        .background(.quaternary, in: Capsule())
                        .foregroundStyle(.secondary)
                    Image(systemName: "arrow.right").font(.caption2.weight(.semibold)).foregroundStyle(.tertiary)
                    Text(update.version)
                        .fontWeight(.semibold)
                        .padding(.horizontal, 8).padding(.vertical, 1)
                        .background(Color.accentColor, in: Capsule())
                        .foregroundStyle(.white)
                }
                .font(.caption)
                .padding(.top, 5)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 26)
        .padding(.top, 38)
        .padding(.bottom, 18)
    }

    private func headline(_ update: AvailableUpdate) -> String {
        switch updater.phase {
        case .downloading: "Downloading ReleaseDock \(update.version)…"
        case .extracting: "Preparing ReleaseDock \(update.version)…"
        case .readyToInstall: "ReleaseDock \(update.version) is ready to install"
        case .installing: "Installing ReleaseDock \(update.version)…"
        default: "A new version of ReleaseDock is ready"
        }
    }

    private func subtitle(_ update: AvailableUpdate) -> String {
        var parts: [String] = []
        if let date = update.date { parts.append("Released \(date.formatted(.relative(presentation: .named)))") }
        if update.size > 0 { parts.append(ByteCountFormatter.string(fromByteCount: Int64(update.size), countStyle: .file)) }
        return parts.joined(separator: " · ")
    }

    private func notes(_ update: AvailableUpdate) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(update.history.enumerated()), id: \.element.id) { index, entry in
                    if index > 0 { Divider().padding(.vertical, 6) }
                    VersionNotesView(entry: entry, isLatest: index == 0)
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 4)
            .padding(.bottom, 18)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(.quinary.opacity(0.6), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(.separator.opacity(0.6), lineWidth: 0.5))
        .padding(.horizontal, 26)
    }

    private var trust: some View {
        HStack(spacing: 16) {
            Label("Signed update, verified before installing", systemImage: "checkmark.shield")
            Label("Downloads over HTTPS", systemImage: "lock")
            Spacer()
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .symbolRenderingMode(.hierarchical)
        .padding(.horizontal, 26)
        .padding(.top, 10)
    }

    private func footer(_ update: AvailableUpdate) -> some View {
        HStack(spacing: 8) {
            switch updater.phase {
            case .downloading(let received, let expected):
                progress(value: expected > 0 ? Double(received) / Double(expected) : nil,
                         label: expected > 0 ? "\(bytes(received)) of \(bytes(expected))" : bytes(received))
                Button("Cancel") { updater.cancel() }
            case .extracting(let value):
                progress(value: value, label: "Checking the signature and unpacking")
            case .readyToInstall:
                Label("Downloaded and verified", systemImage: "checkmark.circle.fill")
                    .font(.callout)
                    .foregroundStyle(.green)
                Spacer()
                Button("Install on Quit") { updater.later() }
                Button("Relaunch Now") { updater.install() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            case .installing:
                ProgressView().controlSize(.small)
                Text("Installing…").foregroundStyle(.secondary)
                Spacer()
            default:
                if let url = releaseURL(update.version) {
                    Button {
                        openURL(url)
                    } label: {
                        Label("View on GitHub", systemImage: "arrow.up.right.square")
                    }
                    .buttonStyle(InlineLinkButtonStyle())
                    .linkCursor()
                    .font(.callout)
                }
                Spacer()
                Button("Skip This Version") { updater.skip() }
                Button("Later") { updater.later() }
                    .keyboardShortcut(.cancelAction)
                Button("Install & Relaunch") { updater.install() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .controlSize(.large)
        .frame(minHeight: 32)
        .padding(.horizontal, 26)
        .padding(.top, 14)
        .padding(.bottom, 20)
    }

    private func progress(value: Double?, label: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if let value { ProgressView(value: min(max(value, 0), 1)) } else { ProgressView().progressViewStyle(.linear) }
            Text(label).font(.caption).foregroundStyle(.secondary).monospacedDigit()
        }
        .frame(maxWidth: .infinity)
        .padding(.trailing, 8)
    }

    private func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(count), countStyle: .file)
    }

    private func releaseURL(_ version: String) -> URL? {
        guard let repository = Bundle.main.object(forInfoDictionaryKey: "RDGitHubRepository") as? String, !repository.isEmpty else { return nil }
        return URL(string: "https://github.com/\(repository)/releases/tag/v\(version)")
    }
}

/// One version: number, date, then its notes grouped by kind of change.
private struct VersionNotesView: View {
    let entry: VersionNotes
    let isLatest: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(entry.version).font(.headline)
                if let date = entry.date {
                    Text(date.formatted(date: .abbreviated, time: .omitted)).font(.caption).foregroundStyle(.secondary)
                }
                if isLatest {
                    Text("LATEST")
                        .font(.system(size: 9, weight: .bold))
                        .tracking(0.4)
                        .padding(.horizontal, 5).padding(.vertical, 1)
                        .foregroundStyle(.green)
                        .background(Color.green.opacity(0.15), in: RoundedRectangle(cornerRadius: 4))
                }
            }
            .padding(.top, 12)
            if entry.notes.isEmpty {
                Text("No release notes.").foregroundStyle(.secondary)
            }
            ForEach(Array(entry.notes.sections.enumerated()), id: \.offset) { _, section in
                VStack(alignment: .leading, spacing: 4) {
                    if let title = section.title {
                        let kind = ChangeKind(title)
                        HStack(spacing: 6) {
                            Image(systemName: kind.symbol)
                                .font(.system(size: 9, weight: .bold))
                                .foregroundStyle(.white)
                                .frame(width: 17, height: 17)
                                .background(kind.color, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
                            Text(kind.title.uppercased())
                                .font(.caption.weight(.bold))
                                .tracking(0.5)
                                .foregroundStyle(kind.color)
                        }
                    }
                    ForEach(Array(section.items.enumerated()), id: \.offset) { _, item in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Circle().fill(.tertiary).frame(width: 4, height: 4).alignmentGuide(.firstTextBaseline) { $0[.bottom] + 4 }
                            Text(Self.markdown(item))
                                .fixedSize(horizontal: false, vertical: true)
                                .textSelection(.enabled)
                        }
                        .padding(.leading, section.title == nil ? 0 : 23)
                    }
                }
            }
        }
    }

    private static func markdown(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
    }
}

/// Keep a Changelog headings shown as New, Improved, Fixed…
private struct ChangeKind {
    let title: String
    let symbol: String
    let color: Color

    init(_ heading: String) {
        switch heading.lowercased() {
        case let h where h.contains("add") || h.contains("new"): (title, symbol, color) = ("New", "plus", .blue)
        case let h where h.contains("chang") || h.contains("improv"): (title, symbol, color) = ("Improved", "arrow.up", .purple)
        case let h where h.contains("fix"): (title, symbol, color) = ("Fixed", "checkmark", .orange)
        case let h where h.contains("remov") || h.contains("deprecat"): (title, symbol, color) = ("Removed", "minus", .gray)
        case let h where h.contains("secur"): (title, symbol, color) = ("Security", "lock.fill", .red)
        default: (title, symbol, color) = (heading, "circle.fill", .gray)
        }
    }
}

/// The running app's icon.
struct AppIcon: View {
    var size: CGFloat

    var body: some View {
        Image(nsImage: NSApplication.shared.applicationIconImage)
            .resizable()
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}
