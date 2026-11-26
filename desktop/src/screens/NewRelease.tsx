import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import {
  errorText, needsCredentials, PipelineEvent, Product, ReleaseKind, ReleaseMode, ReleaseRecord, releases, UiTestRelease,
} from "../api";
import { ProductIcon } from "../components/ProductIcon";
import { shortName } from "../components/Sidebar";
import { stateLabel, stateTone } from "./format";

const TEMPLATES = ["Release {name} {version}", "Release {version}", "Tagging version {version}", "Update to {version}"];

type Stage = "package" | "running" | "review" | "publishing" | "done";

interface Props {
  products: Product[];
  initialProductId?: string;
  onChanged: () => void;
  /** Debug builds: run (and maybe publish) this release automatically. */
  autoRun?: UiTestRelease;
}

/** Package → Checks → Review → Publish, on the Rust release pipeline. */
export function NewRelease({ products, initialProductId, onChanged, autoRun }: Props) {
  const [productId, setProductId] = useState(initialProductId ?? products[0]?.id);
  const product = products.find((p) => p.id === productId);
  const [kind, setKind] = useState<ReleaseKind>("version");
  const [mode, setMode] = useState<ReleaseMode>(product?.releaseMode ?? "assisted");
  const [template, setTemplate] = useState("");
  const [source, setSource] = useState<{ path: string; kind: "zip" | "folder" }>();
  const [dropping, setDropping] = useState(false);
  const [stage, setStage] = useState<Stage>("package");
  const [log, setLog] = useState<PipelineEvent[]>([]);
  const [record, setRecord] = useState<ReleaseRecord>();
  const [error, setError] = useState<string>();
  const [askCredentials, setAskCredentials] = useState(false);

  useEffect(() => { if (!productId && products[0]) setProductId(products[0].id); }, [products, productId]);
  useEffect(() => { if (product?.type !== "plugin") setKind("version"); }, [product]);

  // Progress from the pipeline.
  useEffect(() => {
    const off = listen<PipelineEvent>("pipeline-event", (e) => setLog((l) => [...l, e.payload]));
    return () => { off.then((f) => f()); };
  }, []);

  // Drag a ZIP or folder onto the window.
  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent(async (event) => {
      if (stage !== "package") return;
      if (event.payload.type === "over" || event.payload.type === "enter") setDropping(true);
      else if (event.payload.type === "leave") setDropping(false);
      else if (event.payload.type === "drop") {
        setDropping(false);
        const path = event.payload.paths[0];
        if (path) await chooseSource(path);
      }
    });
    return () => { off.then((f) => f()); };
  }, [stage]);

  // Debug builds: run the requested release once the product list is loaded.
  // A ref, not state: React's development mode runs effects twice on mount.
  const autoStarted = useRef(false);
  useEffect(() => {
    if (!autoRun || autoStarted.current || !product) return;
    autoStarted.current = true;
    releases.sourceKind(autoRun.sourcePath).then((kind) => {
      if (!kind) return setError(`Test source not found: ${autoRun.sourcePath}`);
      setMode(autoRun.mode);
      setSource({ path: autoRun.sourcePath, kind });
      prepare({ path: autoRun.sourcePath, kind }, autoRun.mode).then((r) => { if (r && autoRun.publish) publish(r, true); });
    });
  }, [autoRun, product]);

  async function chooseSource(path: string) {
    const kind = await releases.sourceKind(path);
    if (kind) { setSource({ path, kind }); setError(undefined); }
    else setError("Drop a ZIP file or a folder.");
  }

  async function pick(zip: boolean) {
    const path = await open(zip ? { filters: [{ name: "ZIP package", extensions: ["zip"] }] } : { directory: true });
    if (typeof path === "string") await chooseSource(path);
  }

  async function prepare(chosen = source, chosenMode = mode): Promise<ReleaseRecord | undefined> {
    if (!product || !chosen) return;
    setStage("running"); setLog([]); setError(undefined); setRecord(undefined);
    try {
      const result = await releases.prepare({ productId: product.id, sourcePath: chosen.path, mode: chosenMode, kind, commitTemplate: template || undefined });
      setRecord(result);
      setStage(result.state === "previewed" || result.state === "approved" ? "review" : "done");
      return result;
    } catch (e) {
      setError(errorText(e));
      setStage("package");
    } finally {
      onChanged();
    }
  }

  async function publish(target = record, skipConfirm = false) {
    if (!target) return;
    const record = target;
    if (!skipConfirm && !window.confirm(`Publish ${shortName(record.productName)} ${record.version ?? ""} to WordPress.org?\n\nThis commits the previewed changes in one SVN commit.`)) return;
    setStage("publishing"); setError(undefined);
    try {
      setRecord(await releases.approveAndPublish(record.id));
      setStage("done");
    } catch (e) {
      if (needsCredentials(e)) setAskCredentials(true);
      setError(errorText(e));
      setStage("review");
    } finally {
      onChanged();
    }
  }

  async function cancel() {
    if (!record) return;
    try { setRecord(await releases.cancel(record.id)); } catch (e) { setError(errorText(e)); }
    setStage("package"); setRecord(undefined); onChanged();
  }

  const exampleSummary = useMemo(() => {
    const t = template.trim() || (kind === "readmeUpdate" ? "Update readme for {name} {version}" : "Release {name} {version}");
    return t.replaceAll("{name}", product?.name ?? "Plugin").replaceAll("{slug}", product?.slug ?? "plugin")
      .replaceAll("{version}", "1.2.0").replaceAll("{previous}", product?.currentVersion ?? "1.1.0");
  }, [template, kind, product]);

  return (
    <div className="pad new-release">
      <Steps stage={stage} />
      {error && <div className="card problem"><b>{stage === "package" ? "Could not prepare the release" : "Something stopped the release"}</b><p className="selectable">{error}</p></div>}

      {stage === "package" && (
        <>
          <div className="h-sec">Product</div>
          <div className="card picker">
            {product && <ProductIcon id={product.id} type={product.type} size={36} />}
            <select value={productId} onChange={(e) => setProductId(e.target.value)}>
              {products.map((p) => <option key={p.id} value={p.id}>{shortName(p.name)}{p.currentVersion ? ` — live ${p.currentVersion}` : ""}</option>)}
            </select>
          </div>
          {product?.type === "plugin" && (
            <>
              <div className="h-sec">Publish</div>
              <div className="cards">
                <Choice on={kind === "version"} title="New Version" detail="Update trunk and create tags/<version>." onClick={() => setKind("version")} />
                <Choice on={kind === "readmeUpdate"} title="Readme & Assets Update" detail={`Change readme.txt of the released ${product.currentVersion ?? "version"}, e.g. raise “Tested up to”. Code changes are refused.`} onClick={() => setKind("readmeUpdate")} />
              </div>
            </>
          )}
          <div className="h-sec">Mode</div>
          <div className="cards three">
            <Choice on={mode === "assisted"} title="Assisted" detail="You approve the exact SVN preview before anything is committed." onClick={() => setMode("assisted")} />
            <Choice on={mode === "dryRun"} title="Dry Run" detail="Run every check and build the preview. Never commits." onClick={() => setMode("dryRun")} />
            <Choice on={mode === "automatic"} title="Automatic" detail="Publishes without asking when every condition holds." onClick={() => setMode("automatic")} />
          </div>
          <div className="h-sec">Commit Message</div>
          <div className="template-row">
            <input className="field mono" value={template} placeholder={kind === "readmeUpdate" ? "Update readme for {name} {version}" : "Release {name} {version}"} onChange={(e) => setTemplate(e.target.value)} />
            <select className="field" value="" onChange={(e) => e.target.value && setTemplate(e.target.value === TEMPLATES[0] ? "" : e.target.value)}>
              <option value="">Templates</option>
              {TEMPLATES.map((t) => <option key={t} value={t}>{t}</option>)}
            </select>
          </div>
          <p className="hint">For example “{exampleSummary}”. Tokens: {"{name} {slug} {version} {previous}"}. ReleaseDock adds its release ID on the next line so an interrupted commit can be found.</p>
          <div className="h-sec">Package</div>
          {source ? (
            <div className="card picker">
              <span className={`tag ${source.kind === "zip" ? "blue" : "gray"}`}>{source.kind === "zip" ? "ZIP" : "Folder"}</span>
              <span className="mono selectable path">{source.path}</span>
              <button className="btn sm" onClick={() => setSource(undefined)}>Change</button>
            </div>
          ) : (
            <div className={`drop ${dropping ? "over" : ""}`}>
              <div><div className="t">Drop a ZIP package or an updated folder</div><div className="s">The source is read-only. Only a cleaned copy is ever published.</div></div>
              <div className="btns">
                <button className="btn" onClick={() => pick(false)}>Choose Folder…</button>
                {product?.localDevelopmentPath && <button className="btn" onClick={() => chooseSource(product.localDevelopmentPath!)}>Use Development Folder</button>}
                <button className="btn blue" onClick={() => pick(true)}>Choose ZIP…</button>
              </div>
            </div>
          )}
          <div className="actions">
            <button className="btn blue" disabled={!product || !source} onClick={() => prepare()}>{mode === "dryRun" ? "Run Dry Run" : "Inspect & Prepare"}</button>
          </div>
        </>
      )}

      {(stage === "running" || stage === "publishing") && <Progress log={log} title={stage === "running" ? "Checking the release…" : "Publishing to WordPress.org…"} />}

      {stage === "review" && record && <Review record={record} onPublish={() => publish()} onCancel={cancel} />}

      {stage === "done" && record && (
        <Outcome record={record} onAnother={() => { setStage("package"); setRecord(undefined); setSource(undefined); }} />
      )}

      {askCredentials && product && (
        <Credentials product={product} onClose={() => setAskCredentials(false)} onSaved={() => { setAskCredentials(false); setError(undefined); publish(record, true); }} />
      )}
    </div>
  );
}

function Steps({ stage }: { stage: Stage }) {
  const order: [Stage[], string][] = [[["package"], "Package"], [["running"], "Checks"], [["review"], "Review"], [["publishing", "done"], "Publish"]];
  const current = order.findIndex(([s]) => s.includes(stage));
  return (
    <div className="stepper">
      {order.map(([, label], i) => (
        <div key={label} className={`step ${i < current ? "done" : i === current ? "cur" : ""}`}><span className="n">{i < current ? "✓" : i + 1}</span>{label}</div>
      ))}
    </div>
  );
}

function Choice({ on, title, detail, onClick }: { on: boolean; title: string; detail: string; onClick: () => void }) {
  return (
    <button className={`card choice ${on ? "on" : ""}`} onClick={onClick}>
      <b>{title}</b><span className="muted">{detail}</span>
    </button>
  );
}

function Progress({ log, title }: { log: PipelineEvent[]; title: string }) {
  return (
    <div className="card">
      <div className="progress-head"><span className="spinner" />{title}</div>
      <ul className="event-log">
        {log.map((e, i) => <li key={i}><span className={`tag ${stateTone(e.state)}`}>{stateLabel[e.state] ?? e.state}</span>{e.message}</li>)}
      </ul>
    </div>
  );
}

function Review({ record, onPublish, onCancel }: { record: ReleaseRecord; onPublish: () => void; onCancel: () => void }) {
  const results = record.validation?.results ?? [];
  const problems = results.filter((r) => r.status === "failed" || r.status === "warning");
  const changes = record.preview?.changes ?? [];
  const blocking = results.some((r) => r.status === "failed");
  return (
    <>
      <div className="card hero">
        <div>
          <h2>{shortName(record.productName)} {record.previousVersion ? `${record.previousVersion} → ` : ""}{record.version}</h2>
          <div className="meta"><span>{changes.length} SVN change(s)</span><span>{results.filter((r) => r.status === "passed").length} checks passed</span>{problems.length > 0 && <span className="tag orange">{problems.length} to look at</span>}</div>
        </div>
      </div>
      {problems.length > 0 && (
        <>
          <div className="h-sec">Checks</div>
          <div className="card">
            {problems.map((r, i) => (
              <div key={i} className="check-row">
                <span className={`tag ${r.status === "failed" ? "red" : "orange"}`}>{r.status === "failed" ? "Blocks" : "Warning"}</span>
                <div><b>{r.name}</b> — {r.message}{r.filePath && <span className="mono faint"> {r.filePath}</span>}{r.suggestion && <div className="muted">{r.suggestion}</div>}</div>
              </div>
            ))}
          </div>
        </>
      )}
      <div className="h-sec">Changes</div>
      <div className="card change-list">
        {changes.map((c) => (
          <div key={c.path} className="change"><span className={`st ${c.kind === "added" ? "A" : c.kind === "deleted" ? "D" : "M"}`}><i>{c.kind === "added" ? "A" : c.kind === "deleted" ? "D" : "M"}</i></span><span className="mono">{c.path}{c.isDirectory ? "/" : ""}</span>{c.isCopy && <span className="tag green">copy</span>}</div>
        ))}
      </div>
      {record.preview?.diffExcerpt && (
        <details className="card"><summary>Diff{record.preview.diffTruncated ? " (excerpt)" : ""}</summary><pre className="diff selectable">{record.preview.diffExcerpt}</pre></details>
      )}
      <div className="h-sec">Commit message</div>
      <div className="card"><pre className="mono selectable commit-msg">{record.preview?.commitMessage}</pre></div>
      <div className="actions">
        <button className="btn danger" onClick={onCancel}>Cancel Release</button>
        <button className="btn blue" disabled={blocking} onClick={onPublish}>Approve & Publish…</button>
      </div>
    </>
  );
}

function Outcome({ record, onAnother }: { record: ReleaseRecord; onAnother: () => void }) {
  const v = record.verification;
  const clean = v && v.missingFiles.length === 0 && v.unexpectedFiles.length === 0 && v.checksumMismatches.length === 0;
  return (
    <div className="card outcome">
      <span className={`tag ${stateTone(record.state)}`}>{stateLabel[record.state] ?? record.state}</span>
      <h2>
        {record.state === "completed" && record.mode === "dryRun" && "Dry run complete. Nothing was committed."}
        {record.state === "completed" && record.mode !== "dryRun" && `Published ${shortName(record.productName)} ${record.version} in r${record.committedRevision}`}
        {record.state !== "completed" && `${stateLabel[record.state] ?? record.state}`}
      </h2>
      {v && <p className="muted">{clean ? "Every committed file matches the release (SHA-256)." : "Verification found differences — see the release in History."}</p>}
      {record.errors.length > 0 && <ul className="selectable">{record.errors.map((e, i) => <li key={i}>{e}</li>)}</ul>}
      {record.guidance.length > 0 && <ul className="muted">{record.guidance.map((g, i) => <li key={i}>{g}</li>)}</ul>}
      <div className="actions"><button className="btn" onClick={onAnother}>New Release</button></div>
    </div>
  );
}

function Credentials({ product, onClose, onSaved }: { product: Product; onClose: () => void; onSaved: () => void }) {
  const [username, setUsername] = useState(product.svnUsername ?? "");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string>();
  async function save() {
    try { await releases.storeCredentials(product.id, username, password); onSaved(); } catch (e) { setError(errorText(e)); }
  }
  return (
    <div className="scrim" onClick={onClose}>
      <div className="card sheet" onClick={(e) => e.stopPropagation()}>
        <h3>WordPress.org SVN credentials</h3>
        <p className="muted">Use your WordPress.org username and the SVN password from Profile → Account &amp; Security. It's stored in your system's credential store.</p>
        <input className="field" placeholder="Username" value={username} onChange={(e) => setUsername(e.target.value)} />
        <input className="field" type="password" placeholder="SVN password" value={password} onChange={(e) => setPassword(e.target.value)} />
        {error && <p className="problem-text">{error}</p>}
        <div className="actions"><button className="btn" onClick={onClose}>Cancel</button><button className="btn blue" disabled={!username || !password} onClick={save}>Save & Publish</button></div>
      </div>
    </div>
  );
}
