import { useEffect, useState } from "react";
import { ActivityEntry, errorText, Product, productTabs, TagRow, WorkingCopyState } from "../api";

/** Loads `load()` when `key` changes; returns [data, error, reload]. */
function useLoad<T>(load: () => Promise<T>, key: string): [T | undefined, string | undefined, () => void] {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<string>();
  const [version, setVersion] = useState(0);
  useEffect(() => {
    let live = true;
    setError(undefined);
    load().then((d) => live && setData(d), (e) => live && setError(errorText(e)));
    return () => { live = false; };
  }, [key, version]);
  return [data, error, () => setVersion((v) => v + 1)];
}

function Problem({ text }: { text: string }) {
  return <div className="pad"><div className="card problem"><b>Couldn't reach the SVN repository.</b><p className="selectable">{text}</p></div></div>;
}

function Loading({ text }: { text: string }) {
  return <div className="pad"><div className="progress-head"><span className="spinner" />{text}</div></div>;
}

const dayFormat = new Intl.DateTimeFormat(undefined, { weekday: "long", day: "numeric", month: "short", year: "numeric" });
const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

/** Commits to this product, grouped by day, newest first. */
export function ActivityTab({ product }: { product: Product }) {
  const [entries, error] = useLoad(() => productTabs.activity(product.id), product.id);
  if (error) return <Problem text={error} />;
  if (!entries) return <Loading text="Reading the SVN log…" />;
  if (entries.length === 0) return <div className="pad muted">No commits yet.</div>;
  const days = new Map<string, ActivityEntry[]>();
  for (const e of entries) {
    const day = e.date ? dayFormat.format(e.date) : "Unknown date";
    days.set(day, [...(days.get(day) ?? []), e]);
  }
  return (
    <div>
      {[...days].map(([day, commits]) => (
        <div key={day}>
          <div className="day">{day}<span className="sum">{commits.length} commit{commits.length > 1 ? "s" : ""}{commits.some((c) => c.tag) ? ` · ${commits.filter((c) => c.tag).length} release` : ""}</span></div>
          <div className="commits">
            {commits.map((c) => (
              <div key={c.revision} className={`commit ${c.tag ? "release" : ""}`}>
                <div className="line1">
                  <span className="r">r{c.revision}</span> by {c.author ?? "unknown"}{c.date ? ` · ${timeFormat.format(c.date)}` : ""}
                  {c.tag && <span className="tag green">tags/{c.tag}</span>}
                  {c.byReleaseDock && <span className="tag blue">ReleaseDock</span>}
                </div>
                <div className="msg selectable">{c.message || <span className="faint">No message</span>}</div>
                <div className="chips">
                  {c.paths.slice(0, 6).map((p) => <span key={p.path} className="fchip"><i className={p.action}>{p.action}</i>{p.path}</span>)}
                  {c.paths.length > 6 && <span className="fchip more">+ {c.paths.length - 6} files</span>}
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
      <div className="feed-end">Showing the last {entries.length} commits to this plugin only</div>
    </div>
  );
}

const dateFormat = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });

/** Every released version (tags/), with the Stable tag marked. */
export function ReleasesTab({ product }: { product: Product }) {
  const [data, error] = useLoad(() => productTabs.releasedVersions(product.id), product.id);
  if (error) return <Problem text={error} />;
  if (!data) return <Loading text="Listing tags…" />;
  return (
    <div className="tbl-wrap" style={{ paddingTop: 10 }}>
      <table className="table">
        <thead><tr><th>Version</th><th>Released</th><th>Revision</th><th>By</th></tr></thead>
        <tbody>
          {data.tags.map((t: TagRow) => (
            <tr key={t.version}>
              <td><b>{t.version}</b> {t.version === data.stableTag && <span className="tag green">Stable</span>}</td>
              <td>{t.date ? dateFormat.format(t.date) : ""}</td>
              <td className="num">{t.revision ? `r${t.revision}` : ""}</td>
              <td>{t.author ?? ""} {t.byReleaseDock && <span className="tag blue">ReleaseDock</span>}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="faint" style={{ fontSize: 11.5, margin: "10px 10px 24px" }}>
        {data.tags.length} versions in tags/, newest first (PHP version_compare). Stable tag from trunk/readme.txt: {data.stableTag ?? "not set"}.
      </p>
    </div>
  );
}

const statusLetter: Record<string, string> = { modified: "M", added: "A", deleted: "D", unversioned: "?", missing: "!", conflicted: "C", replaced: "R" };

/** Local changes in the working copy, with Update, Revert and conflict resolution. */
export function FilesTab({ product, onChanged }: { product: Product; onChanged: () => void }) {
  const [state, error, reload] = useLoad(() => productTabs.workingCopy(product.id), product.id);
  const [busy, setBusy] = useState<string>();
  const [message, setMessage] = useState<string>();

  async function run(label: string, action: () => Promise<WorkingCopyState | string | null>) {
    setBusy(label); setMessage(undefined);
    try {
      const result = await action();
      if (typeof result === "string") setMessage(`Backed up to ${result}`);
      reload(); onChanged();
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setBusy(undefined);
    }
  }

  if (error) return <Problem text={error} />;
  if (!state) return <Loading text="Reading the working copy…" />;
  const conflicts = state.changes.filter((c) => c.isConflict || c.isTreeConflict);
  return (
    <div className="pad">
      <div className="card wc-head">
        <div>
          <b>{!state.exists ? "Not checked out yet" : state.changes.length === 0 ? "Clean" : `${state.changes.length} local change(s)`}</b>
          <div className="muted mono selectable">{product.workingCopyPath}{state.revision ? ` · r${state.revision}` : ""}</div>
        </div>
        <span className="spacer" />
        {busy ? <span className="progress-head"><span className="spinner" />{busy}…</span> : (
          <>
            <button className="btn" onClick={() => run("Updating", () => productTabs.update(product.id))}>{state.exists ? "Update" : "Check Out"}</button>
            <button className="btn danger" disabled={!state.exists || state.changes.length === 0} onClick={() => {
              if (window.confirm("Revert all local changes in the working copy?\n\nEvery changed, added and unversioned file plus the svn diff is copied to the Backups folder first. If any backup step fails, nothing is reverted.")) {
                run("Reverting", () => productTabs.revert(product.id));
              }
            }}>Revert…</button>
          </>
        )}
      </div>
      {message && <div className="card" style={{ marginTop: 10 }}><p className="muted selectable">{message}</p></div>}
      {conflicts.length > 0 && (
        <div className="card problem" style={{ marginTop: 14 }}>
          <b>{conflicts.length} conflict(s) after the last update</b>
          <p>Keep the WordPress.org version (your file is backed up first) or keep yours.</p>
          <div className="actions" style={{ marginTop: 8 }}>
            <button className="btn" onClick={() => run("Resolving", () => productTabs.resolve(product.id, conflicts.map((c) => c.path), "mine"))}>Keep Mine</button>
            <button className="btn blue" onClick={() => run("Resolving", () => productTabs.resolve(product.id, conflicts.map((c) => c.path), "server"))}>Use WordPress.org Version</button>
          </div>
        </div>
      )}
      {state.changes.length > 0 && (
        <table className="table" style={{ marginTop: 14 }}>
          <thead><tr><th>Path</th><th>Status</th></tr></thead>
          <tbody>
            {state.changes.map((c) => (
              <tr key={c.path}>
                <td className="mono selectable">{c.path}</td>
                <td><span className={`st ${statusLetter[c.status] === "A" ? "A" : statusLetter[c.status] === "D" ? "D" : statusLetter[c.status] === "?" ? "U" : "M"}`}><i>{statusLetter[c.status] ?? "M"}</i>{c.status}</span></td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
