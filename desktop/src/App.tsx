import { useCallback, useEffect, useState } from "react";
import { api, AppInfo, Product, ReleaseRow, SvnStatus, UiTestRelease } from "./api";
import { Sidebar, shortName } from "./components/Sidebar";
import { Toolbar } from "./components/Toolbar";
import { RefreshIcon, TerminalIcon, UpdateIcon } from "./components/Icons";
import { Overview } from "./screens/Overview";
import { ProductTabs, ProductView, useProductTab } from "./screens/ProductView";
import { History } from "./screens/History";
import { ActivityTab, FilesTab, ReleasesTab } from "./screens/ProductTabs";
import { TranscriptDrawer } from "./components/Transcript";
import { NewRelease } from "./screens/NewRelease";
import { ago } from "./screens/format";

export type Selection =
  | { kind: "overview" }
  | { kind: "newRelease"; productId?: string }
  | { kind: "history" }
  | { kind: "product"; id: string };

export default function App() {
  const [info, setInfo] = useState<AppInfo>();
  const [svn, setSvn] = useState<SvnStatus>();
  const [products, setProducts] = useState<Product[]>([]);
  const [releases, setReleases] = useState<ReleaseRow[]>([]);
  const [error, setError] = useState<string>();
  const [refreshedAt, setRefreshedAt] = useState<number>();
  const [selection, setSelection] = useState<Selection>({ kind: "overview" });
  const [tab, setTab] = useProductTab();
  const [, tick] = useState(0);
  const [autoRun, setAutoRun] = useState<UiTestRelease>();
  const [transcriptOpen, setTranscriptOpen] = useState(false);

  const reload = useCallback(async () => {
    try {
      const [p, r] = await Promise.all([api.products(), api.releases()]);
      setProducts(p);
      setReleases(r);
      setError(undefined);
      setRefreshedAt(Date.now());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    api.appInfo().then(setInfo);
    api.svnStatus().then(setSvn);
    reload();
    api.uiTestView().then((view) => {
      if (!view) return;
      setSelection({ kind: "product", id: view.productId });
      setTab(view.tab as "overview" | "activity" | "releases" | "files");
      setTranscriptOpen(view.transcript);
    });
    api.uiTestRelease().then((test) => {
      if (!test) return;
      setAutoRun(test);
      setSelection({ kind: "newRelease", productId: test.productId });
    });
    const timer = setInterval(() => tick((n) => n + 1), 30_000);
    return () => clearInterval(timer);
  }, [reload]);

  const product = selection.kind === "product" ? products.find((p) => p.id === selection.id) : undefined;
  const fresh = refreshedAt ? `Refreshed ${ago(refreshedAt)}` : "Loading…";
  const title = product ? shortName(product.name) : selection.kind === "history" ? "History" : selection.kind === "newRelease" ? "New Release" : "Overview";

  return (
    <div className={`window platform-${info?.platform ?? "unknown"}`}>
      <Sidebar products={products} selection={selection} onSelect={(s) => { setSelection(s); setTab("overview"); }} svn={svn} info={info} />
      <section className="main">
        <Toolbar title={title} subtitle={fresh}>
          {product && <ProductTabs tab={tab} onChange={setTab} />}
          <span className="spacer" data-tauri-drag-region />
          <div className="tb-group">
            <button className="tb-btn" title="Refresh" onClick={reload}><RefreshIcon /></button>
            {product && <button className="tb-btn" title="Working copy" onClick={() => setTab("files")}><UpdateIcon />Files</button>}
            {product && <button className={`tb-btn ${transcriptOpen ? "on" : ""}`} title="Transcript" onClick={() => setTranscriptOpen((o) => !o)}><TerminalIcon /></button>}
          </div>
          <button className="btn blue" onClick={() => setSelection({ kind: "newRelease", productId: product?.id })}>New Release…</button>
        </Toolbar>
        <div className="content">
          {error && <div className="pad"><div className="card"><b>Couldn't read ReleaseDock's data.</b><p className="muted selectable">{error}</p></div></div>}
          {!error && selection.kind === "overview" && <Overview products={products} releases={releases} onOpenProduct={(id) => setSelection({ kind: "product", id })} />}
          {!error && selection.kind === "history" && <History releases={releases} />}
          {!error && selection.kind === "newRelease" && products.length > 0 && (
            <NewRelease key={selection.productId ?? "any"} products={products} initialProductId={selection.productId} onChanged={reload} autoRun={autoRun} />
          )}
          {!error && product && tab === "overview" && <ProductView product={product} releases={releases.filter((r) => r.productId === product.id)} tab={tab} />}
          {!error && product && tab === "activity" && <ActivityTab key={product.id} product={product} />}
          {!error && product && tab === "releases" && <ReleasesTab key={product.id} product={product} />}
          {!error && product && tab === "files" && <FilesTab key={product.id} product={product} onChanged={reload} />}
        </div>
        <TranscriptDrawer productId={product?.id} open={transcriptOpen && !!product} onClose={() => setTranscriptOpen(false)} />
      </section>
    </div>
  );
}
