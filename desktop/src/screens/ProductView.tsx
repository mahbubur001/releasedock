import { useState } from "react";
import { Product, ReleaseRow, swiftDate } from "../api";
import { ProductIcon } from "../components/ProductIcon";
import { ago, stateLabel, stateTone } from "./format";

type Tab = "overview" | "activity" | "releases" | "files";

export function ProductTabs({ tab, onChange }: { tab: Tab; onChange: (tab: Tab) => void }) {
  const tabs: [Tab, string][] = [["overview", "Overview"], ["activity", "Activity"], ["releases", "Releases"], ["files", "Files"]];
  return (
    <div className="seg">
      {tabs.map(([key, label]) => (
        <button key={key} className={tab === key ? "on" : ""} onClick={() => onChange(key)}>{label}</button>
      ))}
    </div>
  );
}

export function useProductTab() {
  return useState<Tab>("overview");
}

export function ProductView({ product, releases, tab }: { product: Product; releases: ReleaseRow[]; tab: Tab }) {
  if (tab !== "overview") {
    return (
      <div className="states" style={{ gridTemplateColumns: "1fr" }}>
        <div className="card state">
          <h4>{tab[0].toUpperCase() + tab.slice(1)} is coming next</h4>
          <p>This screen is being built from the approved Layout v2 design.</p>
        </div>
      </div>
    );
  }
  const last = product.lastSuccessfulRelease;
  return (
    <div className="pad">
      <div className="card">
        <div className="hero">
          <ProductIcon id={product.id} type={product.type} size={64} />
          <div>
            <h2 className="selectable">{product.name}</h2>
            <div className="meta">
              <span>{product.type === "plugin" ? "Plugin" : "Theme"} · {product.slug}</span>
              {product.currentVersion && <span className="tag green">● Live {product.currentVersion}</span>}
            </div>
          </div>
          <div className="big-ver">
            <div className="v">{product.currentVersion ?? "—"}</div>
            {last && <div className="l">Released {ago(swiftDate(last.date))}{last.revision ? ` · r${last.revision}` : ""}</div>}
          </div>
        </div>
      </div>
      <div className="grid2" style={{ marginTop: 14 }}>
        <div>
          <div className="h-sec">History</div>
          <div className="card mini-feed">
            {releases.length === 0 && <div className="muted">No releases yet.</div>}
            {releases.slice(0, 6).map((r) => (
              <div className="row" key={r.id}>
                <span className="r">{r.version ?? "—"}</span>
                <div style={{ flex: 1 }}>
                  <span className={`tag ${stateTone(r.state)}`}>{stateLabel[r.state] ?? r.state}</span>
                  <div className="muted" style={{ fontSize: 11.5 }}>{r.committedRevision ? `r${r.committedRevision} · ` : ""}{ago(r.createdAt)}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
        <div>
          <div className="h-sec">Repository</div>
          <div className="card">
            <dl className="kv selectable">
              <dt>SVN</dt><dd className="mono">{product.svnRepositoryURL.replace("https://", "")}</dd>
              <dt>Working copy</dt><dd className="mono">{product.workingCopyPath}</dd>
              <dt>Mode</dt><dd>{product.releaseMode === "dryRun" ? "Dry run" : product.releaseMode === "automatic" ? "Automatic" : "Assisted"}</dd>
              <dt>Account</dt><dd>{product.svnUsername ?? <span className="faint">Not set</span>}</dd>
            </dl>
          </div>
        </div>
      </div>
    </div>
  );
}
