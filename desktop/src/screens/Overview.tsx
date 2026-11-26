import { Product, ReleaseRow } from "../api";
import { ProductIcon } from "../components/ProductIcon";
import { shortName } from "../components/Sidebar";
import { ago, releaseTitle, stateLabel, stateTone } from "./format";

interface Props {
  products: Product[];
  releases: ReleaseRow[];
  onOpenProduct: (id: string) => void;
}

/** What needs you now, recent outcomes, and every product at a glance. */
export function Overview({ products, releases, onOpenProduct }: Props) {
  const week = Date.now() - 7 * 86_400_000;
  const releasedThisWeek = releases.filter((r) => r.state === "completed" && r.mode !== "dryRun" && r.createdAt > week).length;
  const waiting = releases.filter((r) => r.state === "previewed" || r.state === "approved");
  const attention = releases.filter((r) => r.state === "recoveryRequired");
  return (
    <div className="pad">
      <div className="big">{attention.length ? "Needs attention." : "Good to ship."}</div>
      <div className="stats">
        <span>{releasedThisWeek} released this week</span>
        <span>{waiting.length} waiting for approval</span>
        <span>{attention.length} need attention</span>
      </div>
      <div className="grid2" style={{ marginTop: 22 }}>
        <div>
          <div className="h-sec">Waiting for you</div>
          <div className="card mini-feed">
            {waiting.length === 0 && <div className="muted">Nothing waits for approval.</div>}
            {waiting.map((r) => (
              <div className="row" key={r.id}><span className="r">{releaseTitle(r)}</span><span className="muted">{stateLabel[r.state]}</span></div>
            ))}
          </div>
        </div>
        <div>
          <div className="h-sec">Recent</div>
          <div className="card mini-feed">
            {releases.slice(0, 5).map((r) => (
              <div className="row" key={r.id}>
                <div style={{ flex: 1 }}>
                  <div>{releaseTitle(r)} <span className={`tag ${stateTone(r.state)}`}>{stateLabel[r.state] ?? r.state}</span></div>
                  <div className="muted" style={{ fontSize: 11.5 }}>
                    {r.committedRevision ? `r${r.committedRevision} · ` : ""}{ago(r.createdAt)}
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
      <div className="h-sec">Products</div>
      <div className="grid2" style={{ gridTemplateColumns: "1fr 1fr" }}>
        {products.map((p) => (
          <div key={p.id} className="card prod-card" onClick={() => onOpenProduct(p.id)}>
            <ProductIcon id={p.id} type={p.type} size={36} />
            <div className="txt">
              <div className="n">{shortName(p.name)}</div>
              <div className="m muted">{p.type === "plugin" ? "Plugin" : "Theme"} · {p.slug}</div>
            </div>
            <div className="v">{p.currentVersion ?? "—"}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
