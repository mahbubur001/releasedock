import AppKit
import ReleaseDockCore
import SwiftUI

@MainActor
@Observable
final class ProductEditorModel: Identifiable {
    let id = UUID()
    let isNew: Bool
    private let model: AppModel
    var product: Product
    var svnURLText: String
    var githubText: String
    var excludePatternsText: String
    var password = ""
    var issues: [ProductValidationIssue] = []
    var detectionMessage: String?
    var confirmAutomation = false

    init(model: AppModel, product: Product?) {
        self.model = model
        isNew = product == nil
        let initial = product ?? Product(
            name: "", slug: "", type: .plugin,
            svnRepositoryURL: URL(string: "https://plugins.svn.wordpress.org/")!,
            workingCopyPath: ""
        )
        self.product = initial
        svnURLText = product?.svnRepositoryURL.absoluteString ?? ""
        githubText = product?.githubRepositoryURL?.absoluteString ?? ""
        excludePatternsText = initial.validation.excludePatterns.joined(separator: "\n")
    }

    func chooseFolder(prompt: String) -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = prompt
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// Fills identity fields from a development folder's headers.
    func detect(from path: String) {
        do {
            let suggestion = try model.productService.suggestion(forFolder: URL(fileURLWithPath: path))
            product.type = suggestion.type
            if product.name.isEmpty { product.name = suggestion.name }
            product.expectedHeaderName = suggestion.name
            if product.slug.isEmpty { product.slug = suggestion.slug }
            if suggestion.type == .plugin, let main = suggestion.mainFile, main != "\(product.slug).php" { product.mainPluginFile = main }
            fillDefaults()
            detectionMessage = "Detected \(suggestion.type.displayName.lowercased()) “\(suggestion.name)” version \(suggestion.version ?? "unknown")."
        } catch {
            detectionMessage = error.localizedDescription
        }
    }

    /// For new products, derives the WordPress.org URL and default working copy from the
    /// slug, without overwriting values the user customised.
    func fillDefaults() {
        guard isNew, !product.slug.isEmpty else { return }
        let host = URL(string: svnURLText)?.host ?? ""
        if svnURLText.isEmpty || host.hasSuffix(".svn.wordpress.org") {
            svnURLText = product.type.defaultRepositoryURL(slug: product.slug)?.absoluteString ?? svnURLText
        }
        if product.workingCopyPath.isEmpty || product.workingCopyPath.hasPrefix(model.paths.products.path) {
            product.workingCopyPath = model.productService.defaultWorkingCopyPath(slug: product.slug, type: product.type)
        }
        if product.expectedHeaderName.isEmpty { product.expectedHeaderName = product.name }
    }

    func save() -> Bool {
        guard let url = URL(string: svnURLText.trimmingCharacters(in: .whitespaces)), url.scheme != nil else {
            issues = [.init(field: .svnURL, message: "Enter a valid SVN URL.", isError: true)]
            return false
        }
        product.svnRepositoryURL = url
        let github = githubText.trimmingCharacters(in: .whitespaces)
        product.githubRepositoryURL = github.isEmpty ? nil : URL(string: github.contains("://") ? github : "https://github.com/\(github)")
        product.validation.excludePatterns = excludePatternsText.components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        product.buildCommand = product.effectiveBuildCommand
        for keyPath in [\Product.localDevelopmentPath, \Product.assetsPath, \Product.mainPluginFile, \Product.svnUsername] {
            if product[keyPath: keyPath]?.isEmpty == true { product[keyPath: keyPath] = nil }
        }
        if product.type == .theme { product.assetsPath = nil; product.mainPluginFile = nil }
        if product.expectedHeaderName.isEmpty { product.expectedHeaderName = product.name }

        issues = model.productService.validate(product)
        guard !issues.contains(where: \.isError) else { return false }
        do {
            try model.productService.save(product)
            if let username = product.svnUsername, !password.isEmpty {
                try model.storeCredentials(for: product, username: username, password: password)
            }
            password = ""
            model.reload()
            return true
        } catch {
            issues = [.init(field: .name, message: error.localizedDescription, isError: true)]
            return false
        }
    }
}

struct ProductEditorView: View {
    @Environment(\.dismiss) private var dismiss
    @Bindable var editor: ProductEditorModel

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section("Identity") {
                    HStack {
                        TextField("Development folder", text: optional(\.localDevelopmentPath), prompt: Text("Optional — used to detect the product"))
                        Button("Choose…") {
                            if let path = editor.chooseFolder(prompt: "Use Folder") {
                                editor.product.localDevelopmentPath = path
                                editor.detect(from: path)
                            }
                        }
                    }
                    if let message = editor.detectionMessage {
                        Text(message).font(.caption).foregroundStyle(.secondary)
                    }
                    Picker("Type", selection: $editor.product.type) {
                        ForEach(ProductType.allCases) { Text($0.displayName).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .disabled(!editor.isNew)
                    TextField("Name", text: $editor.product.name)
                    TextField("Slug", text: $editor.product.slug)
                        .onSubmit { editor.fillDefaults() }
                    TextField(editor.product.type == .plugin ? "Expected “Plugin Name”" : "Expected “Theme Name”", text: $editor.product.expectedHeaderName)
                        .help("A package whose header name differs from this is blocked as a product mismatch.")
                    if editor.product.type == .plugin {
                        TextField("Main plugin file", text: optional(\.mainPluginFile), prompt: Text("Auto-detect"))
                    }
                }
                Section("SVN") {
                    TextField("Repository URL", text: $editor.svnURLText, prompt: Text("https://plugins.svn.wordpress.org/slug"))
                    HStack {
                        TextField("Working copy", text: $editor.product.workingCopyPath)
                        Button("Choose…") { if let p = editor.chooseFolder(prompt: "Use Folder") { editor.product.workingCopyPath = p } }
                    }
                    TextField("WordPress.org username", text: optional(\.svnUsername), prompt: Text("Case-sensitive"))
                    SecureField("SVN password", text: $editor.password, prompt: Text(editor.isNew ? "Optional" : "Leave empty to keep the stored password"))
                    Text("Use the SVN password from your WordPress.org profile (Account & Security → SVN credentials). It is stored only in the macOS Keychain. Leave both empty if svn already has cached credentials.")
                        .font(.caption).foregroundStyle(.secondary)
                    if editor.product.type == .plugin {
                        HStack {
                            TextField("Assets folder", text: optional(\.assetsPath), prompt: Text("Optional — mirrored to /assets"))
                            Button("Choose…") { if let p = editor.chooseFolder(prompt: "Use Folder") { editor.product.assetsPath = p } }
                        }
                    }
                    TextField("GitHub repository", text: $editor.githubText, prompt: Text("Optional, owner/name"))
                }
                Section("GitHub release") {
                    Toggle("Create a GitHub release after each version release", isOn: Binding(
                        get: { editor.product.githubRelease?.enabled ?? false },
                        set: { enabled in
                            var settings = editor.product.githubRelease ?? GitHubReleaseSettings()
                            settings.enabled = enabled
                            editor.product.githubRelease = settings
                        }
                    ))
                    if editor.product.githubRelease?.enabled == true {
                        TextField("Tag prefix", text: Binding(
                            get: { editor.product.githubRelease?.tagPrefix ?? "v" },
                            set: { editor.product.githubRelease?.tagPrefix = $0 }
                        ), prompt: Text("None"))
                        Toggle("Create as draft", isOn: Binding(
                            get: { editor.product.githubRelease?.draft ?? false },
                            set: { editor.product.githubRelease?.draft = $0 }
                        ))
                    }
                    Text("GitHub tags the released commit (\(editor.product.githubRelease?.tagPrefix ?? "v")1.2.0) and publishes the changelog entry with the exact ZIP sent to WordPress.org. Your local repository is not changed. Needs a folder release from Git, the commit pushed to GitHub, and a release token in Settings.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Section("Build") {
                    TextField("Build command", text: optional(\.buildCommand), prompt: Text("Optional, e.g. composer install --no-dev && npm run build"), axis: .vertical)
                        .font(.body.monospaced())
                        .lineLimit(1...3)
                    Text("Runs in a login shell (zsh) on a copy of the folder before a folder release is inspected, so build output ships and your folder stays untouched. ZIP releases are used as they are.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Section("Release") {
                    Picker("Mode", selection: $editor.product.releaseMode) {
                        ForEach(ReleaseMode.allCases) { Text($0.displayName).tag($0) }
                    }
                    if editor.product.releaseMode == .automatic {
                        Toggle("Allow automatic publishing", isOn: Binding(
                            get: { editor.product.automationEnabled },
                            set: { newValue in
                                if newValue { editor.confirmAutomation = true } else { editor.product.automationEnabled = false }
                            }
                        ))
                        Text("Automatic publishing commits without confirmation only when every required check passes, the working copy is clean and current, the tag does not exist, credentials work and the preview is unchanged.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                Section("Validation") {
                    Toggle("Treat warnings as blocking", isOn: $editor.product.validation.blockOnWarnings)
                    Toggle("Require a changelog entry for the version", isOn: $editor.product.validation.requireChangelogEntry)
                    Toggle("Require a clean Git working tree", isOn: $editor.product.validation.requireCleanGitTree)
                    Stepper("Package size warning: \(editor.product.validation.packageSizeWarningMB) MB", value: $editor.product.validation.packageSizeWarningMB, in: 1...500)
                    ForEach(ValidationCheckID.allCases.filter { !$0.isRequired }) { check in
                        Toggle(check.title, isOn: Binding(
                            get: { !editor.product.validation.disabledChecks.contains(check) },
                            set: { enabled in
                                if enabled { editor.product.validation.disabledChecks.remove(check) } else { editor.product.validation.disabledChecks.insert(check) }
                            }
                        ))
                    }
                    VStack(alignment: .leading) {
                        Text("Exclusion patterns (one per line, .distignore syntax)")
                        TextEditor(text: $editor.excludePatternsText)
                            .font(.callout.monospaced())
                            .frame(height: 70)
                    }
                }
                if !editor.issues.isEmpty {
                    Section {
                        ForEach(editor.issues) { issue in
                            Label(issue.message, systemImage: issue.isError ? "xmark.octagon.fill" : "exclamationmark.triangle.fill")
                                .foregroundStyle(issue.isError ? .red : .orange)
                        }
                    }
                }
            }
            .formStyle(.grouped)
            Divider()
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(editor.isNew ? "Add Product" : "Save") {
                    if editor.save() { dismiss() }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(editor.product.name.isEmpty || editor.product.slug.isEmpty)
            }
            .padding()
        }
        .frame(width: 620, height: 720)
        .onChange(of: editor.product.slug) { editor.fillDefaults() }
        .onChange(of: editor.product.type) { editor.fillDefaults() }
        .confirmationDialog("Enable automatic publishing?", isPresented: $editor.confirmAutomation) {
            SwiftUI.Button("Enable Automatic Publishing") { editor.product.automationEnabled = true }
        } message: {
            Text("ReleaseDock will commit \(editor.product.name.isEmpty ? "this product" : editor.product.name) to WordPress.org without asking, whenever all safety conditions hold.")
        }
    }

    private func optional(_ keyPath: WritableKeyPath<Product, String?>) -> Binding<String> {
        Binding(get: { editor.product[keyPath: keyPath] ?? "" }, set: { editor.product[keyPath: keyPath] = $0 })
    }
}
