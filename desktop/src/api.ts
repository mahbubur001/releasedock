// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";

export type ProductType = "plugin" | "theme";
export type ReleaseMode = "dryRun" | "assisted" | "automatic";

/** A product as stored by ReleaseDock (rd_model::Product). */
export interface Product {
  id: string;
  name: string;
  slug: string;
  type: ProductType;
  svnRepositoryURL: string;
  workingCopyPath: string;
  currentVersion?: string;
  releaseMode: ReleaseMode;
  lastSuccessfulRelease?: { releaseID: string; version: string; revision?: number; date: number };
  assetsPath?: string;
  localDevelopmentPath?: string;
  svnUsername?: string;
}

export interface ReleaseRow {
  id: string;
  productId: string;
  productName: string;
  version?: string;
  previousVersion?: string;
  state: string;
  kind: string;
  mode: ReleaseMode;
  committedRevision?: number;
  /** Milliseconds since 1970. */
  createdAt: number;
}

export interface AppInfo {
  version: string;
  platform: string;
  dataFolder?: string;
  storeError?: string;
}

export interface SvnStatus {
  version?: string;
  executable?: string;
  problem?: string;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  products: () => invoke<Product[]>("products"),
  productIcon: (productId: string) => invoke<string | null>("product_icon", { productId }),
  releases: (productId?: string) => invoke<ReleaseRow[]>("releases", { productId }),
  svnStatus: () => invoke<SvnStatus>("svn_status"),
  uiTestRelease: () => invoke<UiTestRelease | null>("ui_test_release"),
  uiTestView: () => invoke<{ productId: string; tab: string; transcript: boolean } | null>("ui_test_view"),
};

/** Seconds since 2001-01-01 (Swift `Date`) to a JavaScript Date. */
export function swiftDate(seconds: number): Date {
  return new Date((seconds + 978_307_200) * 1000);
}

// MARK: Releases

export type ReleaseKind = "version" | "readmeUpdate";

export interface ValidationResult {
  check: string;
  name: string;
  status: "passed" | "warning" | "failed" | "skipped";
  severity: "info" | "warning" | "error";
  message: string;
  filePath?: string;
  suggestion?: string;
}

export interface PreviewChange {
  path: string;
  kind: "added" | "modified" | "deleted" | "replaced" | "propertyChanged";
  isDirectory: boolean;
  isCopy?: boolean;
}

/** The parts of a release record (rd_records::ReleaseRecord) the UI shows. */
export interface ReleaseRecord {
  id: string;
  productID: string;
  productName: string;
  state: string;
  mode: ReleaseMode;
  version?: string;
  previousVersion?: string;
  errors: string[];
  guidance: string[];
  committedRevision?: number;
  validation?: { results: ValidationResult[]; blockOnWarnings: boolean };
  preview?: { changes: PreviewChange[]; diffExcerpt: string; diffTruncated: boolean; commitMessage: string };
  verification?: {
    committedRevision?: number;
    tagExists: boolean;
    tagURL: string;
    missingFiles: string[];
    unexpectedFiles: string[];
    checksumMismatches: string[];
    notes: string[];
  };
}

export interface PipelineEvent {
  releaseId: string;
  state: string;
  message: string;
  at: number;
}

export interface CommandError {
  message: string;
  needsCredentials: boolean;
}

export const releases = {
  sourceKind: (path: string) => invoke<"zip" | "folder" | null>("source_kind", { path }),
  prepare: (args: { productId: string; sourcePath: string; mode: ReleaseMode; kind: ReleaseKind; commitTemplate?: string }) =>
    invoke<ReleaseRecord>("prepare_release", args),
  approveAndPublish: (releaseId: string) => invoke<ReleaseRecord>("approve_and_publish", { releaseId }),
  cancel: (releaseId: string) => invoke<ReleaseRecord>("cancel_release", { releaseId }),
  storeCredentials: (productId: string, username: string, password: string) =>
    invoke<void>("store_credentials", { productId, username, password }),
};

/** The message of a rejected command, whether a CommandError or a plain string. */
export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) return String((error as CommandError).message);
  return String(error);
}

export function needsCredentials(error: unknown): boolean {
  return !!error && typeof error === "object" && (error as CommandError).needsCredentials === true;
}

/** Debug builds: a release to run automatically (see commands::ui_test_release). */
export interface UiTestRelease {
  productId: string;
  sourcePath: string;
  mode: ReleaseMode;
  publish: boolean;
}

// MARK: Product tabs

export interface ActivityEntry {
  revision: number;
  author?: string;
  date?: number;
  message: string;
  paths: { action: string; path: string }[];
  tag?: string;
  byReleaseDock: boolean;
}

export interface TagRow {
  version: string;
  revision?: number;
  date?: number;
  author?: string;
  byReleaseDock: boolean;
}

export interface WorkingCopyChange {
  path: string;
  status: string;
  isConflict: boolean;
  isTreeConflict: boolean;
}

export interface WorkingCopyState {
  exists: boolean;
  revision?: number;
  changes: WorkingCopyChange[];
}

export interface TranscriptEntry {
  id: string;
  date: string;
  productID: string;
  releaseID?: string;
  operation: string;
  command: string;
  exitCode?: number;
  duration: number;
  output: string;
  failure?: string;
}

export const productTabs = {
  activity: (productId: string, limit = 50) => invoke<ActivityEntry[]>("activity", { productId, limit }),
  releasedVersions: (productId: string) => invoke<{ tags: TagRow[]; stableTag?: string }>("released_versions", { productId }),
  workingCopy: (productId: string) => invoke<WorkingCopyState>("working_copy", { productId }),
  update: (productId: string) => invoke<WorkingCopyState>("update_working_copy", { productId }),
  revert: (productId: string) => invoke<string | null>("revert_working_copy", { productId }),
  resolve: (productId: string, paths: string[], keeping: "server" | "mine") =>
    invoke<WorkingCopyState>("resolve_conflicts", { productId, paths, keeping }),
  transcript: (productId: string, releaseId?: string) => invoke<TranscriptEntry[]>("transcript", { productId, releaseId }),
};
