//! Port of `Release/ReleasePipelineService.swift`.
//!
//! The Swift actor becomes a `Sync` struct: the per-product busy set is a mutex-guarded set
//! (an operation on a busy product fails at once, exactly like `withProductLock`), events go
//! through a `tokio::sync::broadcast` channel, and every public operation that Swift wraps in
//! `transcribed` runs inside [`SvnTranscript::with_scope`].

use crate::app_services::{NoopNotificationService, NotificationServicing};
use crate::error::{ReleaseError, io_error};
use crate::fs_util::{
    file_exists, file_exists_is_dir, is_file_url, path_string, relative_to_root, remove_item, standardized,
    trimming_trailing_slash, url_host,
};
use crate::release_zip;
use crate::staging::ReleaseStaging;
use crate::strategy::{RepositoryStrategy, StrategyProvider};
use crate::working_copy;
use rd_inspection::{
    AssetInspector, FolderInspecting, FolderInspectionService, InspectionOptions, ZipInspecting, ZipInspectionService,
};
use rd_integrations::WordPressOrgServicing;
use rd_integrations::{BuildService, Building, CodeChecking, GitHubReleasePublisher, GitHubReleasing, GitServicing};
use rd_keychain::{Keychain, KeychainItem};
use rd_model::{Id, PackageEntry, Product, ProductType, ReleaseMode, ReleaseSummary, SwiftDate};
use rd_records::AuthenticationAvailability;
use rd_records::{
    Approval, ApprovalKind, AutomationGate, ChangeKind, ConflictResolution, DIRECTORY_WAIT_WINDOW, DirectoryState,
    DirectoryStatus, GitRepositoryState, PreviewChange, ReleaseKind, ReleaseManifest, ReleasePreview, ReleaseRecord,
    ReleaseSource, ReleaseState, ReleaseStateError, ReleaseStateMachine, RepositorySnapshot, RestoreOptions,
    StagedCandidate, StateTransition, VerificationReport, WorkingCopyState,
};
use rd_store::{ProductRepository, ReleaseRepository};
use rd_support::{commit_template, hashing, path_safety, redact::redact};
use rd_svn::{
    SvnConflictChoice, SvnCredentials, SvnError, SvnErrorKind, SvnItemStatus, SvnServicing, SvnStatusEntry,
    SvnTranscript, TranscriptScope,
};
use rd_validation::{ValidationContext, ValidationService, ValidationServicing};
use rd_versioning::{ProductDetector, VersionBumpService, php_version};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

type Result<T, E = ReleaseError> = std::result::Result<T, E>;

/// One step of a release, for the UI's live log.
#[derive(Clone, Debug, PartialEq)]
pub struct PipelineEvent {
    pub release_id: Id,
    pub state: ReleaseState,
    pub message: String,
    pub at: SwiftDate,
}

/// Everything the pipeline talks to. Swift's initializer defaults are
/// [`PipelineDependencies::new`] plus the `with_*` builders.
#[derive(Clone)]
pub struct PipelineDependencies {
    pub products: Arc<dyn ProductRepository>,
    pub releases: Arc<dyn ReleaseRepository>,
    pub svn: Arc<dyn SvnServicing>,
    pub git: Arc<dyn GitServicing>,
    pub keychain: Arc<dyn Keychain>,
    pub zip_inspector: Arc<dyn ZipInspecting>,
    pub folder_inspector: Arc<dyn FolderInspecting>,
    pub detector: ProductDetector,
    pub validator: Arc<dyn ValidationServicing>,
    pub staging: Arc<dyn ReleaseStaging>,
    pub strategies: StrategyProvider,
    pub notifications: Arc<dyn NotificationServicing>,
    /// WordPress.org API lookups; None disables them (tests, offline use).
    pub wordpress_org: Option<Arc<dyn WordPressOrgServicing>>,
    /// PHP lint / Plugin Check on the staged candidate; None disables them.
    pub code_checker: Option<Arc<dyn CodeChecking>>,
    /// Runs products' build commands; None disables builds.
    pub builder: Option<Arc<dyn Building>>,
    /// Creates GitHub releases for products that opt in; None disables them.
    pub github_releaser: Option<Arc<dyn GitHubReleasing>>,
    /// Recorded in manifests, e.g. "ReleaseDock 1.0.0 (42)".
    pub generator: String,
}

impl PipelineDependencies {
    /// The required services, with Swift's defaults for the rest: the real ZIP and folder
    /// inspectors, detector, validator and strategies, no notifications, no WordPress.org,
    /// code checks, builds or GitHub releases, and the generator "ReleaseDock".
    pub fn new(
        products: Arc<dyn ProductRepository>,
        releases: Arc<dyn ReleaseRepository>,
        svn: Arc<dyn SvnServicing>,
        git: Arc<dyn GitServicing>,
        keychain: Arc<dyn Keychain>,
        staging: Arc<dyn ReleaseStaging>,
    ) -> Self {
        Self {
            products,
            releases,
            svn,
            git,
            keychain,
            zip_inspector: Arc::new(ZipInspectionService::new()),
            folder_inspector: Arc::new(FolderInspectionService::new()),
            detector: ProductDetector::new(),
            validator: Arc::new(ValidationService::new()),
            staging,
            strategies: StrategyProvider::new(),
            notifications: Arc::new(NoopNotificationService),
            wordpress_org: None,
            code_checker: None,
            builder: None,
            github_releaser: None,
            generator: "ReleaseDock".into(),
        }
    }

    pub fn with_notifications(mut self, notifications: Arc<dyn NotificationServicing>) -> Self {
        self.notifications = notifications;
        self
    }

    pub fn with_wordpress_org(mut self, wordpress_org: Option<Arc<dyn WordPressOrgServicing>>) -> Self {
        self.wordpress_org = wordpress_org;
        self
    }

    pub fn with_code_checker(mut self, code_checker: Option<Arc<dyn CodeChecking>>) -> Self {
        self.code_checker = code_checker;
        self
    }

    pub fn with_builder(mut self, builder: Option<Arc<dyn Building>>) -> Self {
        self.builder = builder;
        self
    }

    pub fn with_github_releaser(mut self, github_releaser: Option<Arc<dyn GitHubReleasing>>) -> Self {
        self.github_releaser = github_releaser;
        self
    }

    pub fn with_validator(mut self, validator: Arc<dyn ValidationServicing>) -> Self {
        self.validator = validator;
        self
    }

    pub fn with_inspectors(
        mut self,
        zip_inspector: Arc<dyn ZipInspecting>,
        folder_inspector: Arc<dyn FolderInspecting>,
    ) -> Self {
        self.zip_inspector = zip_inspector;
        self.folder_inspector = folder_inspector;
        self
    }

    pub fn with_generator(mut self, generator: impl Into<String>) -> Self {
        self.generator = generator.into();
        self
    }
}

/// Orchestrates a release through the state machine. Every transition is persisted
/// before the next side effect, so a crash leaves a record that says exactly how far
/// the release got. Once a commit may have reached the server, the outcome is only
/// ever established by inspecting the repository — never by retrying.
pub struct ReleasePipelineService {
    deps: PipelineDependencies,
    busy_products: Mutex<HashSet<Id>>,
    /// Makes the stale-state check, transition and save of [`Self::move_to`] one step, as
    /// they are inside the Swift actor (where `move` has no suspension point).
    transitions: Mutex<()>,
    events: broadcast::Sender<PipelineEvent>,
}

/// Holds a product's place in the busy set; dropping it (also when the operation's future
/// is dropped) frees the product, like Swift's `defer`.
struct ProductLock<'a> {
    busy: &'a Mutex<HashSet<Id>>,
    product_id: Id,
}

impl Drop for ProductLock<'_> {
    fn drop(&mut self) {
        self.busy
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.product_id);
    }
}

/// The Swift raw value of a serde enum (`recoveryRequired`, `modified`…).
fn raw<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// `record.id.uuidString.prefix(8)`.
fn short_id(id: Id) -> String {
    id.to_string().chars().take(8).collect()
}

/// `ISO8601DateFormatter().string(from: Date())` with `:` replaced by `-`.
fn file_stamp() -> String {
    rd_model::iso8601::format(SwiftDate::now()).replace(':', "-")
}

impl ReleasePipelineService {
    pub fn new(dependencies: PipelineDependencies) -> Self {
        // Swift buffers the newest 100 events per observer.
        let (events, _) = broadcast::channel(100);
        Self {
            deps: dependencies,
            busy_products: Mutex::new(HashSet::new()),
            transitions: Mutex::new(()),
            events,
        }
    }

    pub fn dependencies(&self) -> &PipelineDependencies {
        &self.deps
    }

    // MARK: Events

    /// A new subscription to pipeline events. A receiver that falls more than 100 events
    /// behind skips the oldest (`RecvError::Lagged`), like Swift's `.bufferingNewest(100)`.
    pub fn events(&self) -> broadcast::Receiver<PipelineEvent> {
        self.events.subscribe()
    }

    fn emit(&self, record: &ReleaseRecord, message: &str) {
        let event = PipelineEvent {
            release_id: record.id,
            state: record.state,
            message: message.to_owned(),
            at: SwiftDate::now(),
        };
        // No receivers is not an error.
        let _ = self.events.send(event);
        log::info!(
            target: "release",
            "[{} {}] {}: {}",
            record.product_slug,
            record.version.as_deref().unwrap_or("?"),
            raw(&record.state),
            message
        );
    }

    // MARK: Helpers

    fn load(&self, id: Id) -> Result<ReleaseRecord> {
        self.deps.releases.release(id)?.ok_or(ReleaseError::ReleaseNotFound)
    }

    fn product(&self, id: Id) -> Result<Product> {
        self.deps.products.product(id)?.ok_or(ReleaseError::ProductNotFound)
    }

    fn move_to(&self, record: &mut ReleaseRecord, state: ReleaseState, note: Option<&str>) -> Result<()> {
        let _atomic = self.transitions.lock().unwrap_or_else(|p| p.into_inner());
        // Guard against a stale in-memory copy overwriting a state another operation persisted.
        if let Some(stored) = self.deps.releases.release(record.id)? {
            if stored.state != record.state {
                return Err(ReleaseError::InvalidState(format!(
                    "The release changed to {} in the meantime.",
                    stored.state.display_name()
                )));
            }
        }
        // A new approval or publish attempt invalidates any earlier "verified not committed" result.
        if matches!(state, ReleaseState::Approved | ReleaseState::Publishing) {
            record.verified_not_committed = false;
        }
        ReleaseStateMachine::transition(record, state, note)?;
        self.deps.releases.save_release(record)?;
        self.emit(record, note.unwrap_or(state.display_name()));
        Ok(())
    }

    fn move_with(&self, record: &mut ReleaseRecord, state: ReleaseState, note: &str) -> Result<()> {
        self.move_to(record, state, Some(note))
    }

    fn lock_product(&self, product_id: Id) -> Result<ProductLock<'_>> {
        let mut busy = self.busy_products.lock().unwrap_or_else(|p| p.into_inner());
        if !busy.insert(product_id) {
            return Err(ReleaseError::InvalidState(
                "Another operation is already running for this product.".into(),
            ));
        }
        Ok(ProductLock {
            busy: &self.busy_products,
            product_id,
        })
    }

    /// The product's SVN password from the keychain, when a username is set.
    pub fn credentials(&self, product: &Product) -> Result<Option<SvnCredentials>> {
        let Some(username) = product.svn_username.as_deref().filter(|u| !u.is_empty()) else {
            return Ok(None);
        };
        let Some(host) = url_host(&product.svn_repository_url) else {
            return Ok(None);
        };
        let Some(password) = self.deps.keychain.password(&KeychainItem::svn(host, username))? else {
            return Ok(None);
        };
        Ok(Some(SvnCredentials::new(username, password)))
    }

    pub async fn authentication_availability(&self, product: &Product) -> AuthenticationAvailability {
        if matches!(self.credentials(product), Ok(Some(_))) {
            return AuthenticationAvailability::Keychain;
        }
        if is_file_url(&product.svn_repository_url) {
            // Local repositories need no credentials.
            return AuthenticationAvailability::CachedBySvn;
        }
        if matches!(
            self.deps.svn.has_cached_credentials(&product.svn_repository_url).await,
            Ok(true)
        ) {
            return AuthenticationAvailability::CachedBySvn;
        }
        AuthenticationAvailability::None
    }

    fn strategy(&self, product_type: ProductType) -> &'static dyn RepositoryStrategy {
        self.deps.strategies.strategy(product_type)
    }

    fn svn(&self) -> &dyn SvnServicing {
        self.deps.svn.as_ref()
    }

    // MARK: Create

    /// Registers a new release attempt for a ZIP or folder source. `mode` None uses the
    /// product's release mode; `kind` is [`ReleaseKind::Version`] in Swift by default.
    pub fn create_release(
        &self,
        product_id: Id,
        source: ReleaseSource,
        mode: Option<ReleaseMode>,
        kind: ReleaseKind,
        commit_template: Option<&str>,
    ) -> Result<ReleaseRecord> {
        let product = self.product(product_id)?;
        if kind == ReleaseKind::ReadmeUpdate && product.product_type != ProductType::Plugin {
            return Err(ReleaseError::InvalidState(
                "Readme updates without a new version are only available for plugins.".into(),
            ));
        }
        if let Some(problem) = commit_template.and_then(rd_support::commit_template::problem) {
            return Err(ReleaseError::InvalidState(problem));
        }
        let mut record = ReleaseRecord::new_with(
            Id::new(),
            &product,
            source.clone(),
            mode.unwrap_or(product.release_mode),
            kind,
            SwiftDate::now(),
        );
        record.commit_template = commit_template.map(|t| t.trim().to_owned());
        self.deps.releases.save_release(&record)?;
        let name = Path::new(source.path())
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let kind = match source {
            ReleaseSource::Zip { .. } => "ZIP",
            ReleaseSource::Folder { .. } => "folder",
        };
        self.emit(&record, &format!("Release detected from {kind} {name}"));
        Ok(record)
    }

    // MARK: Restore an older version

    /// The released versions older than the current one, newest first, and a suggested
    /// version for re-releasing one of them.
    pub async fn restore_options(&self, product_id: Id) -> Result<RestoreOptions> {
        let product = self.product(product_id)?;
        let snapshot = self
            .strategy(product.product_type)
            .inspect_remote(&product, None, self.svn())
            .await?;
        Ok(RestoreOptions::new(&snapshot))
    }

    /// Exports `old_version` from the repository and bumps it to `new_version`, which must be
    /// newer than every released version. The result is an ordinary folder release that runs
    /// every check; WordPress.org never moves backwards, so sites on the broken version update.
    async fn create_restore_release_unscoped(
        &self,
        product_id: Id,
        old_version: &str,
        new_version: &str,
        changelog_notes: &str,
        mode: Option<ReleaseMode>,
    ) -> Result<ReleaseRecord> {
        let product = self.product(product_id)?;
        let strategy = self.strategy(product.product_type);
        let snapshot = strategy.inspect_remote(&product, Some(new_version), self.svn()).await?;
        let options = RestoreOptions::new(&snapshot);
        let Some(current) = options.current_version.as_deref() else {
            return Err(ReleaseError::RestoreNotPossible(format!(
                "{} has no released version in the repository.",
                product.name
            )));
        };
        if !snapshot.existing_versions.iter().any(|v| v == old_version) {
            return Err(ReleaseError::RestoreNotPossible(format!(
                "{old_version} is not a released version of {}.",
                product.name
            )));
        }
        if php_version::compare(old_version, current) >= 0 {
            return Err(ReleaseError::RestoreNotPossible(format!(
                "{old_version} is not older than the current version {current}."
            )));
        }
        if let Some(highest) = options.highest_version.as_deref() {
            if php_version::compare(new_version, highest) <= 0 {
                return Err(ReleaseError::RestoreNotPossible(format!(
                    "{new_version} must be newer than {highest} so sites update to it."
                )));
            }
        }

        let mut record = ReleaseRecord::new(
            &product,
            ReleaseSource::folder(""),
            mode.unwrap_or(product.release_mode),
        );
        record.restores_version = Some(old_version.to_owned());
        let folder = self
            .deps
            .staging
            .workspace(record.id)?
            .join("restore")
            .join(&product.slug);
        let exported: Result<()> = async {
            if let Some(parent) = folder.parent() {
                fs::create_dir_all(parent).map_err(io_error(parent))?;
            }
            let base = trimming_trailing_slash(&product.svn_repository_url);
            self.svn()
                .export(&format!("{base}/{}", strategy.release_path(old_version)), &folder)
                .await?;
            let bump = VersionBumpService::new();
            let plan = bump.plan(&folder, &product, new_version, changelog_notes, SwiftDate::now())?;
            bump.apply(&plan, &folder)?;
            Ok(())
        }
        .await;
        if let Err(error) = exported {
            self.deps.staging.remove_workspace(record.id);
            return Err(error);
        }
        record.source = ReleaseSource::folder(path_string(&folder));
        self.deps.releases.save_release(&record)?;
        self.emit(&record, &format!("Restoring the {old_version} code as {new_version}"));
        Ok(record)
    }

    // MARK: Prepare (Detected → Previewed)

    /// Inspects, validates, stages and previews the release. A dry run completes here.
    /// Validation failures end the release in `failed` with the report attached.
    async fn prepare_unscoped(&self, id: Id) -> Result<ReleaseRecord> {
        let mut record = self.load(id)?;
        if record.state != ReleaseState::Detected {
            return Err(ReleaseError::InvalidState(format!(
                "Release is {}; preparation already ran.",
                record.state.display_name()
            )));
        }
        let product = self.product(record.product_id)?;
        let _lock = self.lock_product(product.id)?;
        match self.run_preparation(&mut record, &product).await {
            Ok(record) => Ok(record),
            Err(error) => {
                let _ = self.fail(&mut record, &product, &error).await;
                Err(error)
            }
        }
    }

    async fn run_preparation(&self, record: &mut ReleaseRecord, product: &Product) -> Result<ReleaseRecord> {
        let strategy = self.strategy(product.product_type);
        let options = InspectionOptions {
            exclude_patterns: product.validation.exclude_patterns.clone(),
            ..InspectionOptions::default()
        };

        // 1. Inspect (never modifies the source).
        self.emit(record, "Inspecting package");
        let inspection;
        let mut package_root: Option<PathBuf> = None;
        let mut entries: Vec<PackageEntry> = Vec::new();
        let mut git_state: Option<GitRepositoryState> = None;
        match record.source.clone() {
            ReleaseSource::Zip { path } => {
                let url = PathBuf::from(&path);
                inspection = self.deps.zip_inspector.inspect(&url, &options)?;
                if inspection.blocking_issues().is_empty() {
                    let (root, extracted) = self.deps.staging.extract(&url, &inspection, record.id)?;
                    package_root = Some(root);
                    entries = extracted;
                }
            }
            ReleaseSource::Folder { path } => {
                let url = PathBuf::from(&path);
                // A build runs on a clone; the development folder itself is never modified.
                let mut root = url.clone();
                // Restored code comes from a released tag, which already holds the built files.
                if record.restores_version.is_none() {
                    if let (Some(command), Some(builder)) = (product.effective_build_command(), &self.deps.builder) {
                        self.emit(record, "Running the build command");
                        let workspace = self.deps.staging.workspace(record.id)?;
                        let build = builder.build(&url, command, &workspace).await?;
                        root = PathBuf::from(&build.built_path);
                        record.build = Some(build);
                    }
                }
                inspection = self.deps.folder_inspector.inspect(&root, &options)?;
                package_root = Some(root);
                entries = inspection.included_entries.clone();
                if record.restores_version.is_none() && self.deps.git.is_available().await {
                    let git = self.deps.git.state(&url).await.ok();
                    record.git_commit = git.as_ref().and_then(|g| g.head_commit.clone());
                    git_state = git;
                }
            }
        }
        record.inspection = Some(inspection.clone());
        record.source_sha256 = Some(inspection.sha256.clone());
        record.staging_path = self.deps.staging.recorded_staging_path(record.id);

        let mut detection_error: Option<String> = None;
        if let Some(root) = &package_root {
            match self.deps.detector.detect(
                root,
                Some(product.product_type),
                product.main_plugin_file.as_deref(),
                Some(product.slug.as_str()),
            ) {
                Ok(detected) => {
                    record.version = detected.version.clone();
                    record.detected = Some(detected);
                }
                Err(error) => detection_error = Some(error.to_string()),
            }
        } else {
            detection_error = Some("The package has blocking structural problems and was not extracted.".into());
        }
        self.move_with(
            record,
            ReleaseState::Inspected,
            &format!(
                "Inspected {} files; version {}",
                inspection.included_files().len(),
                record.version.as_deref().unwrap_or("unknown")
            ),
        )?;

        // 2. Validate against the repository.
        self.emit(record, "Checking the SVN repository");
        let snapshot = self.gather_snapshot(product, record.version.as_deref(), strategy).await;
        record.repository = Some(snapshot.clone());
        record.previous_version = snapshot
            .current_version
            .clone()
            .or_else(|| product.current_version.clone());
        if product.product_type == ProductType::Plugin {
            if let Some(assets_path) = &product.assets_path {
                record.assets = AssetInspector::new().inspect(Path::new(assets_path)).ok();
            }
        }
        let context = ValidationContext {
            product: product.clone(),
            inspection: inspection.clone(),
            detected: record.detected.clone(),
            detection_error,
            repository: Some(snapshot.clone()),
            git: git_state,
            preview: None,
            kind: record.kind(),
            code_checks: None,
            assets: record.assets.clone(),
            restores_version: record.restores_version.clone(),
        };
        let report = self.deps.validator.validate(&context);
        let blocking = report.blocking_results().len();
        let warnings = report.warnings().len();
        record.validation = Some(report);
        if blocking > 0 {
            self.move_with(
                record,
                ReleaseState::Failed,
                &format!("Validation failed: {blocking} blocking result(s)"),
            )?;
            self.deps.staging.remove_workspace(record.id);
            return Ok(record.clone());
        }
        self.move_with(
            record,
            ReleaseState::Validated,
            &format!("All required checks passed ({warnings} warning(s))"),
        )?;

        // 3. Stage a clean candidate and apply it to the working copy.
        let (Some(package_root), Some(version)) = (package_root, record.version.clone()) else {
            return Err(ReleaseError::MissingData("package root or version".into()));
        };
        self.emit(record, "Preparing the clean release candidate");
        let candidate = self.deps.staging.build_candidate(&package_root, &entries, record.id)?;
        let recheck = self.deps.detector.detect(
            &candidate.root_dir(),
            Some(product.product_type),
            product.main_plugin_file.as_deref(),
            Some(product.slug.as_str()),
        )?;
        let detected_name = record.detected.as_ref().and_then(|d| d.name.clone());
        if recheck.version.as_deref() != Some(version.as_str()) || recheck.name != detected_name {
            return Err(ReleaseError::CandidateChanged(
                "version or name differs in the staged copy".into(),
            ));
        }
        record.candidate = Some(candidate.clone());

        // 3b. Code checks on exactly what ships, before the working copy is touched.
        if let Some(code_checker) = &self.deps.code_checker {
            let plugin_check = if product.product_type == ProductType::Plugin {
                " and running Plugin Check"
            } else {
                ""
            };
            self.emit(record, &format!("Checking PHP syntax{plugin_check}"));
            let php_files: Vec<String> = candidate
                .files()
                .into_iter()
                .map(|f| f.path.clone())
                .filter(|p| p.to_lowercase().ends_with(".php"))
                .collect();
            let requires_php = record.detected.as_ref().and_then(|d| {
                d.headers
                    .get("Requires PHP")
                    .cloned()
                    .or_else(|| d.readme.as_ref().and_then(|r| r.requires_php().map(str::to_owned)))
            });
            let report = code_checker
                .check(&candidate.root_dir(), &php_files, product, requires_php.as_deref())
                .await;
            record.code_checks = Some(report);
            let mut checked = context.clone();
            checked.code_checks = record.code_checks.clone();
            let checked_report = self.deps.validator.validate(&checked);
            let blocking = checked_report.blocking_results().len();
            record.validation = Some(checked_report);
            if blocking > 0 {
                self.move_with(
                    record,
                    ReleaseState::Failed,
                    &format!("Code checks failed: {blocking} blocking result(s)"),
                )?;
                self.deps.staging.remove_workspace(record.id);
                return Ok(record.clone());
            }
        }
        self.emit(record, "Applying the candidate to the SVN working copy");
        record.working_copy_modified = true;
        self.deps.releases.save_release(record)?;
        match record.kind() {
            ReleaseKind::Version => {
                strategy
                    .apply(
                        &candidate,
                        &version,
                        snapshot.current_version.as_deref(),
                        product,
                        self.svn(),
                    )
                    .await?;
                self.write_manifest(record, strategy)?;
                self.move_with(
                    record,
                    ReleaseState::Staged,
                    &format!(
                        "Staged {} files into {}",
                        candidate.files().len(),
                        strategy.release_path(&version)
                    ),
                )?;
            }
            ReleaseKind::ReadmeUpdate => {
                let Some(readme_file) = record.detected.as_ref().and_then(|d| d.readme_file.clone()) else {
                    return Err(ReleaseError::MissingData("readme".into()));
                };
                strategy
                    .apply_readme_update(&candidate, &readme_file, &version, product, self.svn())
                    .await?;
                self.write_manifest(record, strategy)?;
                let assets = if product.assets_path.is_none() {
                    ""
                } else {
                    " and assets"
                };
                self.move_with(
                    record,
                    ReleaseState::Staged,
                    &format!(
                        "Staged the readme{assets} for trunk and {}",
                        strategy.release_path(&version)
                    ),
                )?;
            }
        }

        // 4. Preview.
        let preview = self.build_preview(record, product, strategy).await?;
        record.preview = Some(preview.clone());
        let mut with_preview = context;
        with_preview.preview = Some(preview.clone());
        with_preview.code_checks = record.code_checks.clone();
        let final_report = self.deps.validator.validate(&with_preview);
        let blocking = final_report.blocking_results().len();
        record.validation = Some(final_report);
        if blocking > 0 {
            return Err(ReleaseError::ValidationBlocked(blocking));
        }
        self.move_with(
            record,
            ReleaseState::Previewed,
            &format!(
                "Preview: {} added, {} modified, {} deleted",
                preview.added(),
                preview.modified(),
                preview.deleted()
            ),
        )?;

        if record.mode == ReleaseMode::DryRun {
            self.reset_working_copy(product, None).await?;
            record.working_copy_modified = false;
            self.move_with(
                record,
                ReleaseState::Completed,
                "Dry run complete; nothing was committed",
            )?;
            self.write_manifest(record, strategy)?;
            let body = if record.kind() == ReleaseKind::ReadmeUpdate {
                format!("The {} {version} readme update is ready to publish.", product.name)
            } else {
                format!("{} {version} is ready to publish.", product.name)
            };
            self.deps
                .notifications
                .notify("Dry run complete", &body, &record.id.to_string())
                .await;
        }
        Ok(record.clone())
    }

    /// Collects repository facts; failures become part of the snapshot instead of failing,
    /// so validation can report them precisely. The WordPress.org lookup runs alongside the
    /// SVN round trips and is best effort: without it, "Tested up to" is only format-checked.
    pub async fn gather_snapshot(
        &self,
        product: &Product,
        version: Option<&str>,
        strategy: &dyn RepositoryStrategy,
    ) -> RepositorySnapshot {
        let wordpress_org = self.deps.wordpress_org.clone();
        let latest = async move {
            match wordpress_org {
                Some(service) => service.latest_word_press_version().await.ok(),
                None => None,
            }
        };
        let (latest, mut snapshot) = tokio::join!(latest, self.gather_repository_snapshot(product, version, strategy));
        snapshot.latest_wordpress_version = latest;
        snapshot
    }

    async fn gather_repository_snapshot(
        &self,
        product: &Product,
        version: Option<&str>,
        strategy: &dyn RepositoryStrategy,
    ) -> RepositorySnapshot {
        let remote: Result<RepositorySnapshot> = async {
            let installation = self.svn().detect_installation().await?;
            let mut snapshot = strategy.inspect_remote(product, version, self.svn()).await?;
            snapshot.svn_version = Some(installation.version);
            Ok(snapshot)
        }
        .await;
        let mut snapshot = match remote {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return RepositorySnapshot {
                    connectivity_error: Some(error.to_string()),
                    authentication: self.authentication_availability(product).await,
                    ..RepositorySnapshot::default()
                };
            }
        };
        snapshot.authentication = self.authentication_availability(product).await;
        if snapshot.repository_exists != Some(true) || !snapshot.structure_problems.is_empty() {
            return snapshot;
        }

        let wc = product.working_copy_dir();
        let local: Result<()> = async {
            snapshot.working_copy_exists = file_exists(&wc.join(".svn"));
            if file_exists(&wc) {
                snapshot.working_copy_url_mismatch =
                    working_copy::working_copy_mismatch(&wc, &product.svn_repository_url, self.svn()).await?;
            }
            if snapshot.working_copy_url_mismatch.is_some() {
                return Ok(());
            }
            if snapshot.working_copy_exists {
                snapshot.working_copy_changes = working_copy::local_changes(&wc, self.svn()).await?;
                // Never update over local changes: that could merge into or conflict with them.
                if !snapshot.working_copy_changes.is_empty() {
                    return Ok(());
                }
            }
            strategy
                .prepare_working_copy(product, snapshot.current_version.as_deref(), self.svn())
                .await?;
            snapshot.working_copy_exists = true;
            let status = self.svn().status(&wc, true).await?;
            snapshot.out_of_date_paths = status
                .iter()
                .filter(|e| e.is_out_of_date())
                .map(|e| e.path.clone())
                .collect();
            snapshot.working_copy_revision = Some(self.svn().info(&path_string(&wc)).await?.revision);
            Ok(())
        }
        .await;
        if let Err(error) = local {
            snapshot.connectivity_error = Some(format!("Working copy: {error}"));
        }
        snapshot
    }

    /// The commit message: the template's summary (with the restored version), a blank line,
    /// and the release's commit marker.
    pub fn commit_message(&self, record: &ReleaseRecord) -> String {
        let template = record.commit_template.as_deref().unwrap_or(match record.kind() {
            ReleaseKind::ReadmeUpdate => commit_template::DEFAULT_README_UPDATE,
            ReleaseKind::Version => commit_template::DEFAULT_VERSION,
        });
        let mut summary = commit_template::render(
            template,
            &record.product_name,
            &record.product_slug,
            record.version.as_deref(),
            record.previous_version.as_deref(),
        );
        if let Some(restored) = &record.restores_version {
            summary.push_str(&format!(" (restores the {restored} code)"));
        }
        format!("{summary}\n\n{}\n", record.commit_marker())
    }

    /// Lists the working-copy changes and fingerprints them together with the content
    /// of every changed file, the candidate digest and the commit message.
    pub async fn build_preview(
        &self,
        record: &ReleaseRecord,
        product: &Product,
        strategy: &dyn RepositoryStrategy,
    ) -> Result<ReleasePreview> {
        let wc = product.working_copy_dir();
        let root = path_string(&standardized(&wc));
        let status = self.svn().status(&wc, false).await?;
        let release_path = strategy.release_path(record.version.as_deref().unwrap_or(""));
        let mut changes: Vec<PreviewChange> = Vec::new();
        let mut fingerprint_lines: Vec<String> = Vec::new();
        for entry in status
            .iter()
            .filter(|e| e.is_local_change() || e.item == SvnItemStatus::Unversioned)
        {
            let relative = relative_to_root(&entry.path, &root).unwrap_or_else(|| entry.path.clone());
            let (exists, is_dir) = file_exists_is_dir(Path::new(&entry.path));
            let kind = match entry.item {
                SvnItemStatus::Added => ChangeKind::Added,
                SvnItemStatus::Deleted | SvnItemStatus::Missing => ChangeKind::Deleted,
                SvnItemStatus::Replaced => ChangeKind::Replaced,
                SvnItemStatus::Modified => ChangeKind::Modified,
                SvnItemStatus::Unversioned => ChangeKind::Added,
                _ if entry.props == SvnItemStatus::Modified => ChangeKind::PropertyChanged,
                _ => ChangeKind::Modified,
            };
            changes.push(PreviewChange {
                path: relative.clone(),
                kind,
                is_directory: is_dir,
                is_copy: entry.copied && relative == release_path,
            });
            let mut line = format!("{}|{}|{}|{relative}", raw(&entry.item), raw(&entry.props), entry.copied);
            if exists && !is_dir {
                let sha = hashing::sha256_file(Path::new(&entry.path)).unwrap_or_else(|_| "unreadable".into());
                line.push('|');
                line.push_str(&sha);
            }
            fingerprint_lines.push(line);
        }
        let mut diff = self.svn().diff(&wc).await.unwrap_or_default();
        let limit = 200_000;
        let truncated = diff.chars().count() > limit;
        if truncated {
            diff = diff.chars().take(limit).collect();
        }
        let message = self.commit_message(record);
        fingerprint_lines.sort();
        let fingerprint = hashing::sha256(
            format!(
                "{}\n{}\n{message}",
                fingerprint_lines.join("\n"),
                record.candidate.as_ref().map_or("", |c| c.tree_digest.as_str())
            )
            .as_bytes(),
        );
        let revision = self.svn().info(&path_string(&wc)).await.ok().map(|i| i.revision);
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(ReleasePreview {
            changes,
            diff_excerpt: diff,
            diff_truncated: truncated,
            commit_message: message,
            fingerprint,
            working_copy_revision: revision,
            created_at: SwiftDate::now(),
        })
    }

    // MARK: Approve

    /// Records approval of the current preview. Automatic approval passes only when
    /// every `AutomationGate` condition holds.
    async fn approve_unscoped(&self, id: Id, kind: ApprovalKind) -> Result<ReleaseRecord> {
        let product = self.product(self.load(id)?.product_id)?;
        let _lock = self.lock_product(product.id)?;
        let mut record = self.load(id)?;
        if record.mode == ReleaseMode::DryRun {
            return Err(ReleaseStateError::DryRunCannotPublish.into());
        }
        match record.state {
            ReleaseState::Previewed => {}
            ReleaseState::RecoveryRequired if record.verified_not_committed && kind == ApprovalKind::User => {}
            state => {
                return Err(ReleaseError::InvalidState(format!(
                    "Only a previewed release can be approved (current state: {}).",
                    state.display_name()
                )));
            }
        }
        let Some(preview) = record.preview.clone() else {
            return Err(ReleaseError::MissingData("preview".into()));
        };
        if let Some(report) = &record.validation {
            if report.is_blocking() {
                return Err(ReleaseError::ValidationBlocked(report.blocking_results().len()));
            }
        }

        let strategy = self.strategy(product.product_type);
        let current = self.build_preview(&record, &product, strategy).await?.fingerprint;
        if kind == ApprovalKind::Automatic {
            let reasons = AutomationGate::blocking_reasons(&product, &record, Some(&current));
            if !reasons.is_empty() {
                return Err(ReleaseError::AutomationNotAuthorized(reasons));
            }
        }
        if current != preview.fingerprint {
            return Err(ReleaseError::PreviewChanged);
        }
        record.approval = Some(Approval::new(kind, SwiftDate::now(), preview.fingerprint));
        let note = if kind == ApprovalKind::Automatic {
            "Automatically authorized"
        } else {
            "Approved by user"
        };
        self.move_with(&mut record, ReleaseState::Approved, note)?;
        Ok(record)
    }

    /// Re-runs validation with a new preview so a changed working copy is never approved
    /// against a stale report. Git state is read again, as at preparation.
    async fn revalidate(&self, record: &mut ReleaseRecord, product: &Product, preview: &ReleasePreview) {
        let Some(inspection) = record.inspection.clone() else {
            return;
        };
        let mut git = None;
        if let ReleaseSource::Folder { path } = &record.source {
            if record.restores_version.is_none() && self.deps.git.is_available().await {
                git = self.deps.git.state(Path::new(path)).await.ok();
            }
        }
        let context = ValidationContext {
            product: product.clone(),
            inspection,
            detected: record.detected.clone(),
            detection_error: None,
            repository: record.repository.clone(),
            git,
            preview: Some(preview.clone()),
            kind: record.kind(),
            code_checks: record.code_checks.clone(),
            assets: record.assets.clone(),
            restores_version: record.restores_version.clone(),
        };
        record.validation = Some(self.deps.validator.validate(&context));
    }

    // MARK: Publish (Approved → Completed)

    async fn publish_unscoped(&self, id: Id) -> Result<ReleaseRecord> {
        let mut record = self.load(id)?;
        let product = self.product(record.product_id)?;
        let approval = match (&record.state, &record.approval) {
            (ReleaseState::Approved, Some(approval)) => approval.clone(),
            _ => {
                return Err(ReleaseError::InvalidState(
                    "The release must be approved before publishing.".into(),
                ));
            }
        };
        let Some(version) = record.version.clone() else {
            return Err(ReleaseError::MissingData("version".into()));
        };
        let strategy = self.strategy(product.product_type);
        let record = &mut record;
        let product = &product;

        let _lock = self.lock_product(product.id)?;
        // Pre-flight: the approved preview must be exactly what is about to be committed.
        self.emit(record, "Re-checking the working copy and repository");
        let current = self.build_preview(record, product, strategy).await?;
        if current.fingerprint != approval.preview_fingerprint {
            record.approval = None;
            record.preview = Some(current.clone());
            self.revalidate(record, product, &current).await;
            if let Some(report) = &record.validation {
                if report.is_blocking() {
                    let blocking = report.blocking_results().len();
                    self.fail_and_reset(
                        record,
                        product,
                        "The working copy changed after approval and no longer passes validation",
                    )
                    .await?;
                    return Err(ReleaseError::ValidationBlocked(blocking));
                }
            }
            self.move_with(
                record,
                ReleaseState::Previewed,
                "Preview changed after approval; review and approve again",
            )?;
            return Err(ReleaseError::PreviewChanged);
        }
        let base = trimming_trailing_slash(&product.svn_repository_url).to_owned();
        let release_path = strategy.release_path(&version);
        let release_url = format!("{base}/{release_path}");
        // Check for an earlier landed attempt first: once it landed, the working copy is
        // necessarily out of date, and that must lead to recovery, not failure.
        // A readme update edits an existing tag, so only its commit marker can tell.
        let release_exists = match record.kind() {
            ReleaseKind::Version => self.svn().exists(&release_url).await?,
            ReleaseKind::ReadmeUpdate => {
                if self.find_committed_revision(record, product).await?.is_some() {
                    self.move_with(
                        record,
                        ReleaseState::Publishing,
                        "Readme update already present on the server",
                    )?;
                    return self.recover_after_uncertain_commit(record, product, strategy).await;
                }
                false
            }
        };
        if !release_exists {
            let status = self.svn().status(&product.working_copy_dir(), true).await?;
            let out_of_date = status.iter().filter(|e| e.is_out_of_date()).count();
            if out_of_date > 0 {
                self.fail_and_reset(
                    record,
                    product,
                    &format!("Repository changed on the server ({out_of_date} path(s)); prepare the release again"),
                )
                .await?;
                return Err(ReleaseError::RemoteChanged(format!(
                    "{out_of_date} path(s) are out of date"
                )));
            }
        }
        if release_exists {
            // Idempotency: an earlier attempt of this very release may have landed.
            if self.find_committed_revision(record, product).await?.is_some() {
                self.move_with(
                    record,
                    ReleaseState::Publishing,
                    "Release already present on the server",
                )?;
                return self.recover_after_uncertain_commit(record, product, strategy).await;
            }
            self.fail_and_reset(
                record,
                product,
                &format!("{release_path} was created on the server by someone else"),
            )
            .await?;
            return Err(ReleaseError::ReleasePathExists(release_path));
        }

        let credentials = self.credentials(product)?;
        if credentials.is_none() {
            let availability = self.authentication_availability(product).await;
            if availability != AuthenticationAvailability::CachedBySvn {
                return Err(ReleaseError::CredentialsRequired);
            }
        }
        record.revision_before = self.svn().info(&base).await?.last_changed_revision;

        // Persist "publishing" before the commit so a crash is recoverable.
        self.move_with(record, ReleaseState::Publishing, &format!("Committing to {base}"))?;
        let wc = product.working_copy_dir();
        if let Ok(backup) = self.svn().diff(&wc).await {
            if !backup.is_empty() {
                let _ = self.deps.staging.write_backup(
                    &backup,
                    &format!("{}-{version}-{}-committed.diff", product.slug, short_id(record.id)),
                );
            }
        }
        match self
            .svn()
            .commit(std::slice::from_ref(&wc), &current.commit_message, credentials.as_ref())
            .await
        {
            Ok(revision) => {
                record.committed_revision = Some(revision);
                self.move_with(
                    record,
                    ReleaseState::Verifying,
                    &format!("Committed revision {revision}"),
                )?;
            }
            Err(SvnError::CommitOutcomeUncertain(detail)) => {
                let note = format!("Commit outcome uncertain: {detail}");
                record.errors.push(note.clone());
                self.move_with(record, ReleaseState::RecoveryRequired, &note)?;
                return self.recover_after_uncertain_commit(record, product, strategy).await;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    Some(SvnErrorKind::AuthenticationFailed | SvnErrorKind::AuthenticationRequired)
                ) =>
            {
                // Rejected before anything was committed; the staged working copy is intact.
                record.approval = None;
                self.move_with(
                    record,
                    ReleaseState::Previewed,
                    "SVN rejected the credentials; nothing was committed",
                )?;
                return Err(error.into());
            }
            Err(error) => {
                self.fail_and_reset(record, product, &format!("Commit rejected: {error}"))
                    .await?;
                return Err(error.into());
            }
        }
        self.verify_and_complete(record, product, strategy).await
    }

    /// Convenience for automatic mode: approve automatically, then publish.
    pub async fn publish_automatically(&self, id: Id) -> Result<ReleaseRecord> {
        self.approve(id, ApprovalKind::Automatic).await?;
        self.publish(id).await
    }

    async fn verify_and_complete(
        &self,
        record: &mut ReleaseRecord,
        product: &Product,
        strategy: &dyn RepositoryStrategy,
    ) -> Result<ReleaseRecord> {
        let (Some(version), Some(candidate)) = (record.version.clone(), record.candidate.clone()) else {
            return Err(ReleaseError::MissingData("candidate".into()));
        };
        self.emit(record, "Verifying the release on the server");
        let verified: Result<VerificationReport> = async {
            match record.kind() {
                ReleaseKind::Version => {
                    let scratch = self.deps.staging.workspace(record.id)?;
                    strategy
                        .verify(&candidate, &version, product, self.svn(), &scratch)
                        .await
                }
                ReleaseKind::ReadmeUpdate => {
                    let Some(readme_file) = record.detected.as_ref().and_then(|d| d.readme_file.clone()) else {
                        return Err(ReleaseError::MissingData("readme".into()));
                    };
                    strategy
                        .verify_readme_update(&candidate, &readme_file, &version, product, self.svn())
                        .await
                }
            }
        }
        .await;
        let report = match verified {
            Ok(report) => report,
            Err(error) => {
                // Leave the release recoverable ("Inspect Repository") rather than stuck in Verifying.
                record
                    .errors
                    .push(format!("Verification failed: {}", redact(&error.to_string())));
                if record.state == ReleaseState::Verifying {
                    self.move_with(
                        record,
                        ReleaseState::RecoveryRequired,
                        &format!("Verification could not complete: {error}"),
                    )?;
                }
                return Err(error);
            }
        };
        record.verification = Some(report.clone());
        if !report.tag_exists {
            self.move_with(
                record,
                ReleaseState::RecoveryRequired,
                &format!("The commit was reported but {} is missing", report.tag_url),
            )?;
            return Ok(record.clone());
        }
        if record.committed_revision.is_none() {
            record.committed_revision = report.committed_revision;
        }

        // Bring the working copy to the committed state.
        if let Err(error) = self.svn().update(&product.working_copy_dir(), None).await {
            if let Some(verification) = &mut record.verification {
                verification
                    .notes
                    .push(format!("Working copy update after commit failed: {error}"));
            }
        }
        record.working_copy_modified = false;
        record.guidance = match record.kind() {
            ReleaseKind::ReadmeUpdate => strategy.post_readme_update_guidance(product, &version),
            ReleaseKind::Version => strategy.post_release_guidance(product, &version),
        };
        let summary = if report.is_clean() {
            "Verified".to_owned()
        } else {
            format!(
                "Verified with {} difference(s)",
                report.missing_files.len() + report.unexpected_files.len() + report.checksum_mismatches.len()
            )
        };
        let revision = record
            .committed_revision
            .map_or_else(|| "?".to_owned(), |r| r.to_string());
        self.move_with(
            record,
            ReleaseState::Completed,
            &format!("{summary}; revision {revision}"),
        )?;

        let mut updated = self
            .deps
            .products
            .product(product.id)
            .ok()
            .flatten()
            .unwrap_or_else(|| product.clone());
        if record.kind() == ReleaseKind::Version {
            updated.current_version = Some(version.clone());
            updated.last_successful_release = Some(ReleaseSummary {
                release_id: record.id,
                version: version.clone(),
                revision: record.committed_revision,
                date: SwiftDate::now(),
            });
        }
        updated.updated_at = SwiftDate::now();
        self.deps.products.save_product(&updated)?;
        self.write_manifest(record, strategy)?;
        let creates_github_release = record.kind() == ReleaseKind::Version
            && updated.github_release.as_ref().is_some_and(|g| g.enabled)
            && self.deps.github_releaser.is_some();
        if creates_github_release {
            // Keep the exact published package next to the manifest for the release asset.
            record.release_zip_path = self
                .write_release_zip(record, &candidate)
                .ok()
                .flatten()
                .map(|p| path_string(&p));
            self.deps.releases.save_release(record)?;
        }
        self.deps.staging.remove_workspace(record.id);
        if creates_github_release {
            self.emit(record, "Creating the GitHub release");
            *record = self.create_github_release(record.clone(), &updated).await?;
        }
        let title = match record.kind() {
            ReleaseKind::ReadmeUpdate => format!("{} {version} readme updated", product.name),
            ReleaseKind::Version => format!("{} {version} released", product.name),
        };
        let revision = record
            .committed_revision
            .map_or_else(|| "?".to_owned(), |r| r.to_string());
        let differences = if report.is_clean() {
            ""
        } else {
            " Verification found differences; review the report."
        };
        self.deps
            .notifications
            .notify(
                &title,
                &format!("Committed revision {revision}.{differences}"),
                &record.id.to_string(),
            )
            .await;
        Ok(record.clone())
    }

    // MARK: GitHub release

    /// Creates (or completes) the GitHub release of a completed version release. Never
    /// changes the release's state: the WordPress.org release stands either way.
    pub async fn publish_github_release(&self, id: Id) -> Result<ReleaseRecord> {
        let record = self.load(id)?;
        if record.state != ReleaseState::Completed
            || record.kind() != ReleaseKind::Version
            || record.mode == ReleaseMode::DryRun
        {
            return Err(ReleaseError::InvalidState(
                "Only a completed version release can get a GitHub release.".into(),
            ));
        }
        let product = self.product(record.product_id)?;
        self.create_github_release(record, &product).await
    }

    async fn create_github_release(&self, record: ReleaseRecord, product: &Product) -> Result<ReleaseRecord> {
        let Some(releaser) = self.deps.github_releaser.clone() else {
            return Ok(record);
        };
        let token = self
            .deps
            .keychain
            .password(&KeychainItem::github_release_token())
            .ok()
            .flatten();
        let zip = record
            .release_zip_path
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| file_exists(p));
        let result = GitHubReleasePublisher::new(releaser)
            .publish(&record, product, zip.as_deref(), token.as_deref())
            .await;
        let mut latest = self.load(record.id)?;
        latest.github_release = Some(result.clone());
        self.deps.releases.save_release(&latest)?;
        let message = if result.succeeded() {
            format!("GitHub release {} is ready", result.tag)
        } else {
            format!("GitHub release failed: {}", result.error.as_deref().unwrap_or(""))
        };
        self.emit(&latest, &message);
        if !result.succeeded() {
            self.deps
                .notifications
                .notify(
                    &format!("GitHub release failed: {}", product.name),
                    result.error.as_deref().unwrap_or(""),
                    &format!("{}-github", record.id),
                )
                .await;
        }
        Ok(latest)
    }

    /// Zips the candidate as `<slug>/…` into `<slug>-<version>.zip` next to the manifest.
    fn write_release_zip(&self, record: &ReleaseRecord, candidate: &StagedCandidate) -> Result<Option<PathBuf>> {
        let (Some(version), Some(manifest)) = (&record.version, &record.manifest_path) else {
            return Ok(None);
        };
        let Some(folder) = Path::new(manifest).parent() else {
            return Ok(None);
        };
        let zip = folder.join(format!("{}-{version}.zip", record.product_slug));
        if fs::symlink_metadata(&zip).is_ok() {
            let _ = remove_item(&zip);
        }
        release_zip::zip_tree(&candidate.root_dir(), &record.product_slug, &zip).map_err(io_error(&zip))?;
        Ok(Some(zip))
    }

    // MARK: Directory status

    /// Asks WordPress.org whether it offers a completed release yet, records the answer,
    /// and notifies once it goes live.
    pub async fn refresh_directory_status(&self, id: Id) -> Result<ReleaseRecord> {
        let record = self.load(id)?;
        let (true, Some(version), Some(wordpress_org)) = (
            record.is_directory_tracked(),
            record.version.clone(),
            self.deps.wordpress_org.clone(),
        ) else {
            return Ok(record);
        };
        let listing = wordpress_org
            .directory_listing(&record.product_slug, record.product_type)
            .await?;
        // Reload: the record may have changed while the request ran.
        let mut latest = self.load(id)?;
        let was_live = latest.directory_status.as_ref().map(|s| s.state) == Some(DirectoryState::Live);
        let state = DirectoryStatus::state(&version, listing.as_ref());
        let now = SwiftDate::now();
        let live_since = if state == DirectoryState::Live {
            Some(
                latest
                    .directory_status
                    .as_ref()
                    .and_then(|s| s.live_since)
                    .unwrap_or(now),
            )
        } else {
            None
        };
        latest.directory_status = Some(DirectoryStatus::new(
            state,
            listing.and_then(|l| l.version),
            now,
            live_since,
        ));
        self.deps.releases.save_release(&latest)?;
        if state == DirectoryState::Live && !was_live {
            self.emit(
                &latest,
                &format!("{} {version} is live on WordPress.org", latest.product_name),
            );
            self.deps
                .notifications
                .notify(
                    &format!("{} {version} is live", latest.product_name),
                    &format!("WordPress.org now offers {version} to sites."),
                    &format!("{}-live", latest.id),
                )
                .await;
        }
        Ok(latest)
    }

    /// Refreshes every recent release that WordPress.org does not offer yet. Errors are
    /// skipped; the next round tries again. Swift's `now` defaults to `Date()`.
    pub async fn refresh_awaiting_directory_releases(&self, now: SwiftDate) -> Vec<ReleaseRecord> {
        if self.deps.wordpress_org.is_none() {
            return Vec::new();
        }
        let Ok(releases) = self.deps.releases.releases(None) else {
            return Vec::new();
        };
        let mut refreshed = Vec::new();
        for release in releases
            .iter()
            .filter(|r| r.awaits_directory(now, DIRECTORY_WAIT_WINDOW))
        {
            if let Ok(updated) = self.refresh_directory_status(release.id).await {
                refreshed.push(updated);
            }
        }
        refreshed
    }

    // MARK: Recovery

    /// Searches the product's recent history for this release's commit marker.
    pub async fn find_committed_revision(&self, record: &ReleaseRecord, product: &Product) -> Result<Option<i64>> {
        let log = self
            .svn()
            .log(trimming_trailing_slash(&product.svn_repository_url), 30)
            .await?;
        let marker = record.commit_marker();
        Ok(log.iter().find(|e| e.message.contains(&marker)).map(|e| e.revision))
    }

    /// Establishes what happened to an uncertain commit by inspecting the repository.
    async fn recover_unscoped(&self, id: Id) -> Result<ReleaseRecord> {
        let mut record = self.load(id)?;
        if record.state != ReleaseState::RecoveryRequired {
            return Err(ReleaseError::InvalidState("Release does not require recovery.".into()));
        }
        let product = self.product(record.product_id)?;
        let strategy = self.strategy(product.product_type);
        let _lock = self.lock_product(product.id)?;
        self.recover_after_uncertain_commit(&mut record, &product, strategy)
            .await
    }

    async fn recover_after_uncertain_commit(
        &self,
        record: &mut ReleaseRecord,
        product: &Product,
        strategy: &dyn RepositoryStrategy,
    ) -> Result<ReleaseRecord> {
        let Some(version) = record.version.clone() else {
            return Err(ReleaseError::MissingData("version".into()));
        };
        self.emit(record, "Inspecting the repository to determine the commit outcome");
        let wc = product.working_copy_dir();
        let _ = self.svn().cleanup(&wc).await;

        if let Some(revision) = self.find_committed_revision(record, product).await? {
            record.committed_revision = Some(revision);
            if record.state != ReleaseState::Verifying {
                self.move_with(
                    record,
                    ReleaseState::Verifying,
                    &format!("Found this release's commit in revision {revision}"),
                )?;
            }
            // The server has the commit; discard the local scheduled copy and update to it.
            self.reset_working_copy(product, None).await?;
            return self.verify_and_complete(record, product, strategy).await;
        }

        let release_path = strategy.release_path(&version);
        let release_url = format!(
            "{}/{release_path}",
            trimming_trailing_slash(&product.svn_repository_url)
        );
        // A readme update's tag existed before; only a missing new tag says anything here.
        if record.kind() == ReleaseKind::Version && self.svn().exists(&release_url).await? {
            record
                .errors
                .push(format!("{release_url} exists but was not created by this release"));
            if record.state == ReleaseState::Publishing {
                self.move_with(
                    record,
                    ReleaseState::RecoveryRequired,
                    "Release path exists without this release's marker",
                )?;
            }
            self.fail_and_reset(
                record,
                product,
                &format!("{release_path} exists but was not created by this release"),
            )
            .await?;
            return Ok(record.clone());
        }

        record.verified_not_committed = true;
        let note = "Verified: the commit is not in the repository";
        if record.state == ReleaseState::Publishing {
            self.move_with(record, ReleaseState::RecoveryRequired, note)?;
        } else {
            record.transitions.push(StateTransition::new(
                Some(ReleaseState::RecoveryRequired),
                ReleaseState::RecoveryRequired,
                SwiftDate::now(),
                Some(note.into()),
            ));
            self.deps.releases.save_release(record)?;
            self.emit(record, note);
        }
        Ok(record.clone())
    }

    // MARK: Cancel / abandon

    /// Cancels a release that has not started publishing, restoring the working copy.
    async fn cancel_unscoped(&self, id: Id) -> Result<ReleaseRecord> {
        let mut record = self.load(id)?;
        let product = self.product(record.product_id)?;
        if record.state.may_have_committed() || record.state.is_terminal() {
            return Err(ReleaseError::InvalidState(
                "A release cannot be cancelled once publishing started; use recovery.".into(),
            ));
        }
        let _lock = self.lock_product(product.id)?;
        if record.working_copy_modified {
            // The staging started from a clean working copy, so when it still matches the
            // preview there is nothing of the user's to save (as after a dry run).
            let mut untouched = false;
            if let Some(preview) = &record.preview {
                let strategy = self.strategy(product.product_type);
                untouched = self
                    .build_preview(&record, &product, strategy)
                    .await
                    .ok()
                    .map(|p| p.fingerprint)
                    .as_deref()
                    == Some(preview.fingerprint.as_str());
            }
            let backup = (!untouched).then(|| format!("{}-{}-cancelled.diff", product.slug, short_id(record.id)));
            self.reset_working_copy(&product, backup).await?;
            record.working_copy_modified = false;
        }
        self.move_with(&mut record, ReleaseState::Cancelled, "Cancelled by user")?;
        self.deps.staging.remove_workspace(record.id);
        Ok(record)
    }

    /// Ends a recovery whose commit was verified absent.
    async fn abandon_unscoped(&self, id: Id) -> Result<ReleaseRecord> {
        let mut record = self.load(id)?;
        let product = self.product(record.product_id)?;
        if record.state != ReleaseState::RecoveryRequired || !record.verified_not_committed {
            return Err(ReleaseError::InvalidState(
                "Only a release verified as not committed can be abandoned.".into(),
            ));
        }
        let _lock = self.lock_product(product.id)?;
        self.fail_and_reset(&mut record, &product, "Abandoned after verifying nothing was committed")
            .await?;
        Ok(record)
    }

    /// Marks interrupted releases after an app restart: anything that may have committed
    /// requires recovery; everything earlier is cancelled (nothing left the machine).
    pub async fn reconcile_interrupted_releases(&self) -> Result<Vec<ReleaseRecord>> {
        let mut changed = Vec::new();
        for mut record in self.deps.releases.active_releases()? {
            let Some(target) = ReleaseStateMachine::state_after_interruption(record.state) else {
                continue;
            };
            if target == ReleaseState::Cancelled && record.working_copy_modified {
                if let Ok(product) = self.product(record.product_id) {
                    let backup = format!("{}-{}-interrupted.diff", product.slug, short_id(record.id));
                    let _ = self.reset_working_copy(&product, Some(backup)).await;
                    record.working_copy_modified = false;
                }
            }
            self.move_with(&mut record, target, "Interrupted by an app restart")?;
            changed.push(record);
        }
        Ok(changed)
    }

    // MARK: Working copy maintenance

    /// Explicit user action for a dirty working copy: backs up the diff and unversioned
    /// files, then reverts. Returns the backup location.
    async fn revert_working_copy_unscoped(&self, product_id: Id) -> Result<Option<PathBuf>> {
        let product = self.product(product_id)?;
        let _lock = self.lock_product(product.id)?;
        let backup = format!("{}-{}-revert.diff", product.slug, file_stamp());
        self.reset_working_copy(&product, Some(backup)).await
    }

    /// Reverts all local changes under the working copy and updates it.
    ///
    /// With `backup_name`, the status listing and diff are written to `Backups/` and every
    /// added, modified, replaced, conflicted, unversioned and ignored path is copied there
    /// first; if any copy fails nothing is reverted. Afterwards, files that were scheduled
    /// for addition are removed, and pre-existing unversioned/ignored files are removed
    /// only when they were backed up.
    async fn reset_working_copy(&self, product: &Product, backup_name: Option<String>) -> Result<Option<PathBuf>> {
        let wc = product.working_copy_dir();
        if !file_exists(&wc.join(".svn")) {
            return Ok(None);
        }
        let root = path_string(&wc);
        let status = self.svn().status(&wc, false).await?;
        let added: BTreeSet<String> = status
            .iter()
            .filter(|e| matches!(e.item, SvnItemStatus::Added | SvnItemStatus::Replaced))
            .map(|e| e.path.clone())
            .collect();
        let untracked: Vec<String> = status
            .iter()
            .filter(|e| matches!(e.item, SvnItemStatus::Unversioned | SvnItemStatus::Ignored))
            .map(|e| e.path.clone())
            .collect();

        let mut backup_url = None;
        if let Some(name) = &backup_name {
            use SvnItemStatus::*;
            let preserve = [
                Added,
                Modified,
                Replaced,
                Conflicted,
                Merged,
                Obstructed,
                Unversioned,
                Ignored,
            ];
            let copying: Vec<SvnStatusEntry> = status
                .iter()
                .filter(|e| preserve.contains(&e.item) || e.props == Modified)
                .cloned()
                .collect();
            backup_url = Some(self.back_up(&status, &copying, &wc, name).await?);
        }

        self.svn().revert(&wc).await?;
        let mut removable: Vec<String> = added.into_iter().collect();
        if backup_name.is_some() {
            for path in untracked {
                if !removable.contains(&path) {
                    removable.push(path);
                }
            }
        }
        removable.sort_by_key(|p| std::cmp::Reverse(p.chars().count()));
        for path in removable {
            let url = Path::new(&path);
            if !file_exists(url) || relative_to_root(&path, &root).is_none() || !path_safety::is_contained(url, &wc) {
                continue;
            }
            remove_item(url).map_err(io_error(url))?;
        }
        let _ = self.svn().update(&wc, None).await;
        Ok(backup_url)
    }

    /// Writes the status listing and diff to `Backups/<name>` and copies each of `entries`
    /// that exists into the folder next to it. Fails if any copy fails, so callers never
    /// discard what was not saved.
    async fn back_up(
        &self,
        status: &[SvnStatusEntry],
        entries: &[SvnStatusEntry],
        wc: &Path,
        name: &str,
    ) -> Result<PathBuf> {
        let root = path_string(wc);
        let diff = self.svn().diff(wc).await.unwrap_or_default();
        let listing = status
            .iter()
            .map(|e| format!("{} {} {}", raw(&e.item), raw(&e.props), e.path))
            .collect::<Vec<_>>()
            .join("\n");
        let url = self.deps.staging.write_backup(
            &format!(
                "# svn status\n{listing}\n\n# svn diff (text files only; full copies are in the folder next to this file)\n{diff}"
            ),
            name,
        )?;
        let folder = url.with_extension("files");
        let mut sorted: Vec<&SvnStatusEntry> = entries.iter().collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let mut copied: Vec<String> = Vec::new();
        for entry in sorted {
            let source = Path::new(&entry.path);
            if !file_exists(source) {
                continue;
            }
            let Some(relative) = relative_to_root(&entry.path, &root) else {
                return Err(ReleaseError::InvalidState(format!(
                    "Unexpected path outside the working copy: {}",
                    entry.path
                )));
            };
            if copied.iter().any(|c| relative_to_root(&entry.path, c).is_some()) {
                continue;
            }
            let destination = folder.join(&relative);
            // `FileManager.copyItem` never overwrites: an earlier backup with the same name
            // keeps its copy and this backup fails, so nothing is reverted.
            if fs::symlink_metadata(&destination).is_ok() {
                return Err(ReleaseError::Io(format!(
                    "{} couldn't be copied because an item with the same name already exists.",
                    destination.display()
                )));
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(io_error(parent))?;
            }
            BuildService::clone_tree(source, &destination).map_err(io_error(source))?;
            copied.push(entry.path.clone());
        }
        Ok(url)
    }

    /// Status of the product's working copy; local only, no network.
    pub async fn working_copy_state(&self, product_id: Id) -> Result<WorkingCopyState> {
        let wc = self.product(product_id)?.working_copy_dir();
        if !file_exists(&wc.join(".svn")) {
            return Ok(WorkingCopyState::new(false, None, Vec::new()));
        }
        let changes = working_copy::changes(&wc, self.svn()).await?;
        let revision = self.svn().info(&path_string(&wc)).await.ok().map(|i| i.revision);
        Ok(WorkingCopyState::new(true, revision, changes))
    }

    /// `svn update` (or the sparse checkout when there is no working copy yet). Local changes
    /// are kept; where they collide with the server, svn leaves conflicts to resolve.
    async fn update_working_copy_unscoped(&self, product_id: Id) -> Result<WorkingCopyState> {
        let product = self.product(product_id)?;
        self.ensure_no_staged_release(&product)?;
        {
            let _lock = self.lock_product(product.id)?;
            let wc = product.working_copy_dir();
            if file_exists(&wc) {
                if let Some(mismatch) =
                    working_copy::working_copy_mismatch(&wc, &product.svn_repository_url, self.svn()).await?
                {
                    return Err(ReleaseError::InvalidState(mismatch));
                }
            }
            self.strategy(product.product_type)
                .prepare_working_copy(&product, product.current_version.as_deref(), self.svn())
                .await?;
        }
        self.working_copy_state(product_id).await
    }

    /// Resolves the given conflicts (paths relative to the working copy). Taking the server
    /// version backs up the local files first. Returns the new state and the backup, if any.
    async fn resolve_conflicts_unscoped(
        &self,
        product_id: Id,
        paths: &[String],
        side: ConflictResolution,
    ) -> Result<(WorkingCopyState, Option<PathBuf>)> {
        let product = self.product(product_id)?;
        self.ensure_no_staged_release(&product)?;
        let backup = {
            let _lock = self.lock_product(product.id)?;
            self.resolve_locked(&product, paths, side).await?
        };
        Ok((self.working_copy_state(product_id).await?, backup))
    }

    async fn resolve_locked(
        &self,
        product: &Product,
        paths: &[String],
        side: ConflictResolution,
    ) -> Result<Option<PathBuf>> {
        let wc = product.working_copy_dir();
        let wanted: HashSet<PathBuf> = paths.iter().map(|p| standardized(&wc.join(p))).collect();
        let status = self.svn().status(&wc, false).await?;
        let conflicts: Vec<SvnStatusEntry> = status
            .iter()
            .filter(|e| e.is_conflict() && wanted.contains(&standardized(Path::new(&e.path))))
            .cloned()
            .collect();
        if conflicts.is_empty() {
            return Ok(None);
        }
        if let Some(entry) = conflicts
            .iter()
            .find(|e| !path_safety::is_contained(Path::new(&e.path), &wc))
        {
            return Err(ReleaseError::InvalidState(format!(
                "Unexpected path outside the working copy: {}",
                entry.path
            )));
        }
        let urls = |entries: &[&SvnStatusEntry]| entries.iter().map(|e| PathBuf::from(&e.path)).collect::<Vec<_>>();
        let trees: Vec<&SvnStatusEntry> = conflicts.iter().filter(|e| e.is_tree_conflict).collect();
        let contents: Vec<&SvnStatusEntry> = conflicts.iter().filter(|e| !e.is_tree_conflict).collect();
        match side {
            ConflictResolution::Mine => {
                // mine-full drops the conflict markers; tree conflicts only accept "working".
                if !contents.is_empty() {
                    self.svn()
                        .resolve(&urls(&contents), SvnConflictChoice::MineFull)
                        .await?;
                }
                if !trees.is_empty() {
                    self.svn().resolve(&urls(&trees), SvnConflictChoice::Working).await?;
                }
                Ok(None)
            }
            ConflictResolution::Server => {
                let name = format!("{}-{}-conflicts.diff", product.slug, file_stamp());
                let url = self.back_up(&status, &conflicts, &wc, &name).await?;
                if !contents.is_empty() {
                    self.svn()
                        .resolve(&urls(&contents), SvnConflictChoice::TheirsFull)
                        .await?;
                }
                // A tree conflict has no "theirs" text: revert it, drop what svn leaves behind
                // unversioned (it is backed up), then take the server's state of that path.
                for entry in trees {
                    let path = PathBuf::from(&entry.path);
                    self.svn().revert(&path).await?;
                    let left = self
                        .svn()
                        .status(&path, false)
                        .await
                        .ok()
                        .and_then(|s| s.into_iter().find(|e| e.path == entry.path));
                    if left.is_some_and(|l| matches!(l.item, SvnItemStatus::Unversioned | SvnItemStatus::Ignored)) {
                        remove_item(&path).map_err(io_error(&path))?;
                    }
                    let _ = self.svn().update(&path, None).await;
                }
                Ok(Some(url))
            }
        }
    }

    /// Updating or resolving under a staged release would change what was previewed.
    fn ensure_no_staged_release(&self, product: &Product) -> Result<()> {
        if self
            .deps
            .releases
            .active_releases()?
            .iter()
            .any(|r| r.product_id == product.id && r.working_copy_modified)
        {
            return Err(ReleaseError::InvalidState(format!(
                "A release of {} is staged in the working copy. Publish or cancel it first.",
                product.name
            )));
        }
        Ok(())
    }

    async fn fail_and_reset(&self, record: &mut ReleaseRecord, product: &Product, reason: &str) -> Result<()> {
        if record.working_copy_modified || record.state.may_have_committed() {
            let backup = format!("{}-{}-failed.diff", product.slug, short_id(record.id));
            let _ = self.reset_working_copy(product, Some(backup)).await;
            record.working_copy_modified = false;
        }
        record.errors.push(redact(reason));
        self.move_with(record, ReleaseState::Failed, reason)?;
        self.deps.staging.remove_workspace(record.id);
        self.deps
            .notifications
            .notify(
                &format!("Release failed: {}", product.name),
                reason,
                &record.id.to_string(),
            )
            .await;
        Ok(())
    }

    async fn fail(&self, record: &mut ReleaseRecord, product: &Product, error: &ReleaseError) -> Result<()> {
        if record.state.is_terminal()
            || !ReleaseStateMachine::can_transition(record.state, ReleaseState::Failed, record.mode)
        {
            return Ok(());
        }
        self.fail_and_reset(record, product, &error.to_string()).await
    }

    fn write_manifest(&self, record: &mut ReleaseRecord, strategy: &dyn RepositoryStrategy) -> Result<()> {
        let Some(version) = record.version.clone() else {
            return Ok(());
        };
        let manifest = ReleaseManifest::make(
            record,
            record.candidate.as_ref(),
            &strategy.release_path(&version),
            record.git_commit.as_deref(),
            &self.deps.generator,
            SwiftDate::now(),
        );
        let url = path_string(&self.deps.staging.write_manifest(
            &manifest,
            &record.product_slug,
            &version,
            record.id,
        )?);
        if record.manifest_path.as_deref() != Some(url.as_str()) {
            record.manifest_path = Some(url);
            self.deps.releases.save_release(record)?;
        }
        Ok(())
    }

    // MARK: Transcript

    /// Runs `body` with its svn commands recorded in the product's transcript.
    async fn transcribed<T>(
        &self,
        operation: &str,
        product_id: Id,
        release_id: Option<Id>,
        body: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        SvnTranscript::with_scope(TranscriptScope::new(product_id, release_id, operation), body).await
    }

    pub async fn prepare(&self, id: Id) -> Result<ReleaseRecord> {
        let product_id = self.load(id)?.product_id;
        self.transcribed("Prepare", product_id, Some(id), self.prepare_unscoped(id))
            .await
    }

    pub async fn approve(&self, id: Id, kind: ApprovalKind) -> Result<ReleaseRecord> {
        let product_id = self.load(id)?.product_id;
        self.transcribed("Approve", product_id, Some(id), self.approve_unscoped(id, kind))
            .await
    }

    pub async fn publish(&self, id: Id) -> Result<ReleaseRecord> {
        let product_id = self.load(id)?.product_id;
        self.transcribed("Publish", product_id, Some(id), self.publish_unscoped(id))
            .await
    }

    pub async fn recover(&self, id: Id) -> Result<ReleaseRecord> {
        let product_id = self.load(id)?.product_id;
        self.transcribed("Inspect repository", product_id, Some(id), self.recover_unscoped(id))
            .await
    }

    pub async fn cancel(&self, id: Id) -> Result<ReleaseRecord> {
        let product_id = self.load(id)?.product_id;
        self.transcribed("Cancel", product_id, Some(id), self.cancel_unscoped(id))
            .await
    }

    pub async fn abandon(&self, id: Id) -> Result<ReleaseRecord> {
        let product_id = self.load(id)?.product_id;
        self.transcribed("Abandon", product_id, Some(id), self.abandon_unscoped(id))
            .await
    }

    pub async fn revert_working_copy(&self, product_id: Id) -> Result<Option<PathBuf>> {
        self.transcribed(
            "Revert working copy",
            product_id,
            None,
            self.revert_working_copy_unscoped(product_id),
        )
        .await
    }

    pub async fn update_working_copy(&self, product_id: Id) -> Result<WorkingCopyState> {
        self.transcribed(
            "Update working copy",
            product_id,
            None,
            self.update_working_copy_unscoped(product_id),
        )
        .await
    }

    pub async fn resolve_conflicts(
        &self,
        product_id: Id,
        paths: &[String],
        side: ConflictResolution,
    ) -> Result<(WorkingCopyState, Option<PathBuf>)> {
        self.transcribed(
            "Resolve conflicts",
            product_id,
            None,
            self.resolve_conflicts_unscoped(product_id, paths, side),
        )
        .await
    }

    /// Swift's `mode` defaults to None (the product's release mode).
    pub async fn create_restore_release(
        &self,
        product_id: Id,
        old_version: &str,
        new_version: &str,
        changelog_notes: &str,
        mode: Option<ReleaseMode>,
    ) -> Result<ReleaseRecord> {
        self.transcribed(
            "Prepare restore",
            product_id,
            None,
            self.create_restore_release_unscoped(product_id, old_version, new_version, changelog_notes, mode),
        )
        .await
    }
}
