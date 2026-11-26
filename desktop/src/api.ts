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
};

/** Seconds since 2001-01-01 (Swift `Date`) to a JavaScript Date. */
export function swiftDate(seconds: number): Date {
  return new Date((seconds + 978_307_200) * 1000);
}
