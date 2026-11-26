import { useEffect, useState } from "react";
import { productTabs, TranscriptEntry } from "../api";

/** The svn commands ReleaseDock ran for this product, newest operation first. */
export function TranscriptDrawer({ productId, open, onClose }: { productId?: string; open: boolean; onClose: () => void }) {
  const [entries, setEntries] = useState<TranscriptEntry[]>([]);
  useEffect(() => {
    if (open && productId) productTabs.transcript(productId).then((e) => setEntries([...e].reverse()), () => setEntries([]));
  }, [open, productId]);
  const copy = () => navigator.clipboard.writeText(entries.map((e) => `$ ${e.command}\n  ${status(e)}\n${e.output}`).join("\n\n"));
  return (
    <div className={`drawer ${open ? "on" : ""}`}>
      <div className="drawer-head">
        <b>Transcript</b>
        <span className="muted">{entries.length ? `${entries.length} svn command(s). Passwords go through stdin and never appear.` : "No svn commands recorded yet."}</span>
        <span className="spacer" />
        <button className="btn sm" onClick={copy} disabled={!entries.length}>Copy</button>
        <button className="btn sm" onClick={onClose}>Close</button>
      </div>
      <div className="drawer-body selectable">
        {entries.map((e) => (
          <div key={e.id}>
            <div className="cmd"><span className="p">$</span><span className="c">{e.command}</span><span className={`x ${ok(e) ? "ok" : "err"}`}>{e.operation} · {status(e)}</span></div>
            {e.output && <div className="out">{e.output}</div>}
          </div>
        ))}
      </div>
    </div>
  );
}

const ok = (e: TranscriptEntry) => e.exitCode === 0 && !e.failure;
function status(e: TranscriptEntry) {
  const d = e.duration < 1 ? `${Math.round(e.duration * 1000)} ms` : `${e.duration.toFixed(1)} s`;
  return e.exitCode !== undefined && !e.failure ? `exit ${e.exitCode} · ${d}` : `stopped · ${d}`;
}
