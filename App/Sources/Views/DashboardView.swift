import ReleaseDockCore
import SwiftUI

struct DashboardView: View {
    @Environment(AppModel.self) private var model
    @Environment(SparkleUpdateService.self) private var updater

    private var candidates: [ReleaseRecord] {
        model.releases.filter { !$0.state.isTerminal }
    }

    private var recent: [ReleaseRecord] {
        Array(model.releases.filter { $0.state == .completed && $0.mode != .dryRun }.prefix(6))
    }

    private var failures: [ReleaseRecord] {
        Array(model.releases.filter { $0.state == .failed && $0.validation?.isBlocking == true }.prefix(5))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let problem = model.svnProblem {
                    banner(problem, systemImage: "exclamationmark.triangle.fill", color: .orange)
                }
                if model.updateChecker.isUpdateAvailable, let release = model.updateChecker.latestRelease {
                    banner("ReleaseDock \(release.version) is available (you have \(AppModel.appVersion)).", systemImage: "arrow.down.app.fill", color: .blue) {
                        Button("View") { model.selection = .updates }
                    }
                }

                HStack(spacing: 12) {
                    StatTile(title: "Products", value: "\(model.products.count)", systemImage: "shippingbox.fill")
                    StatTile(title: "Released", value: "\(model.releases.filter { $0.state == .completed && $0.mode != .dryRun }.count)", systemImage: "checkmark.seal.fill", tint: .green)
                    StatTile(title: "In progress", value: "\(candidates.count)", systemImage: "hourglass", tint: .blue)
                    StatTile(title: "Need attention", value: "\(model.releases.filter { $0.state == .recoveryRequired }.count + failures.count)", systemImage: "exclamationmark.triangle.fill", tint: .orange)
                }

                SectionCard(title: "Products", systemImage: "shippingbox") {
                    if model.products.isEmpty {
                        ContentUnavailableView {
                            Label("No products yet", systemImage: "shippingbox")
                        } description: {
                            Text("Register a WordPress.org plugin or theme to start releasing.")
                        } actions: {
                            Button("Add Product") {
                                model.selection = .products
                                model.showAddProduct = true
                            }
                        }
                    } else {
                        Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 8) {
                            GridRow {
                                Text("Product").foregroundStyle(.secondary)
                                Text("Current version").foregroundStyle(.secondary)
                                Text("Last release").foregroundStyle(.secondary)
                                Text("Mode").foregroundStyle(.secondary)
                                Text("")
                            }
                            .font(.caption)
                            ForEach(model.products) { product in
                                GridRow {
                                    HStack { ProductTypeIcon(type: product.type); Text(product.name) }
                                    Text(product.currentVersion ?? "—").monospacedDigit()
                                    Text(product.lastSuccessfulRelease.map { "\($0.version) · \($0.date.formatted(.relative(presentation: .named)))" } ?? "—")
                                    Text(product.releaseMode.displayName + (product.isAutomaticPublishingAuthorized ? " ✓" : ""))
                                    Button("Release…") {
                                        model.wizardProductID = product.id
                                        model.selection = .wizard
                                    }
                                    .buttonStyle(.link)
                                }
                            }
                        }
                    }
                }

                HStack(alignment: .top, spacing: 16) {
                    SectionCard(title: "Release candidates", systemImage: "hourglass") {
                        if candidates.isEmpty {
                            Text("No release in progress.").foregroundStyle(.secondary)
                        }
                        ForEach(candidates) { release in releaseRow(release) }
                    }
                    SectionCard(title: "Recent releases", systemImage: "checkmark.seal") {
                        if recent.isEmpty {
                            Text("Nothing published yet.").foregroundStyle(.secondary)
                        }
                        ForEach(recent) { release in releaseRow(release) }
                    }
                }

                SectionCard(title: "Validation failures", systemImage: "xmark.octagon") {
                    if failures.isEmpty {
                        Text("No recent validation failures.").foregroundStyle(.secondary)
                    }
                    ForEach(failures) { release in
                        VStack(alignment: .leading, spacing: 4) {
                            releaseRow(release)
                            ForEach(release.validation?.blockingResults.prefix(3) ?? []) { result in
                                Label("\(result.name): \(result.message)", systemImage: "xmark.circle")
                                    .font(.caption).foregroundStyle(.secondary).padding(.leading, 20)
                            }
                        }
                    }
                }

                SectionCard(title: "App updates", systemImage: "arrow.triangle.2.circlepath") {
                    KeyValueRow(key: "Installed", value: "\(AppModel.appVersion) (\(AppModel.appBuild))")
                    KeyValueRow(key: "Latest on GitHub", value: model.updateChecker.latestRelease?.version ?? (model.settings.githubRepository.isEmpty ? "Not configured" : "Unknown"))
                    KeyValueRow(key: "In-app updates", value: updater.isConfigured ? "Enabled (signed updates)" : (updater.disabledReason ?? "Disabled"))
                }
            }
            .padding(20)
        }
        .navigationTitle("Dashboard")
        .toolbar {
            ToolbarItem {
                Button { model.selection = .wizard } label: { Label("New Release", systemImage: "plus") }
                    .disabled(model.products.isEmpty)
            }
        }
    }

    private func releaseRow(_ release: ReleaseRecord) -> some View {
        Button {
            model.historySelection = release.id
            model.selection = .history
        } label: {
            HStack {
                ProductTypeIcon(type: release.productType)
                Text("\(release.productName) \(release.version ?? "")")
                Spacer()
                if let revision = release.committedRevision { Text("r\(revision)").monospacedDigit().foregroundStyle(.secondary) }
                StateBadge(state: release.state)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .pointingHandCursor()
    }

    private func banner(_ text: String, systemImage: String, color: Color, @ViewBuilder action: () -> some View = { EmptyView() }) -> some View {
        HStack {
            Image(systemName: systemImage).foregroundStyle(color)
            Text(text)
            Spacer()
            action()
        }
        .padding(12)
        .background(color.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
    }
}
