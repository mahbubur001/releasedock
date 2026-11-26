import { AppInfo, Product, SvnStatus } from "../api";
import { GearIcon, HistoryIcon, OverviewIcon, PlusIcon, ReleaseIcon } from "./Icons";
import { ProductIcon } from "./ProductIcon";
import type { Selection } from "../App";

interface Props {
  products: Product[];
  selection: Selection;
  onSelect: (selection: Selection) => void;
  svn?: SvnStatus;
  info?: AppInfo;
}

export function Sidebar({ products, selection, onSelect, svn, info }: Props) {
  const nav = (key: Selection["kind"], label: string, Icon: typeof OverviewIcon) => (
    <li className={selection.kind === key ? "sel" : ""} onClick={() => onSelect({ kind: key } as Selection)}>
      <Icon />{label}
    </li>
  );
  return (
    <aside className="sidebar">
      <div className="traffic" data-tauri-drag-region />
      <div className="side-scroll">
        <ul className="nav">
          {nav("overview", "Overview", OverviewIcon)}
          {nav("newRelease", "New Release", ReleaseIcon)}
          {nav("history", "History", HistoryIcon)}
        </ul>
        <div className="side-head">Products <span>{products.length}</span></div>
        <ul className="nav">
          {products.map((p) => (
            <li key={p.id} className={`prod ${selection.kind === "product" && selection.id === p.id ? "sel" : ""}`} onClick={() => onSelect({ kind: "product", id: p.id })}>
              <ProductIcon id={p.id} type={p.type} />
              <div className="txt">
                <div className="n">{shortName(p.name)}</div>
                <div className="s">{p.currentVersion ? `${p.currentVersion} · live` : "Not released yet"}</div>
              </div>
            </li>
          ))}
        </ul>
      </div>
      <div className="side-foot">
        <div className="svn">
          <i style={{ background: svn?.problem ? "var(--orange)" : "var(--green)" }} />
          {svn === undefined ? "Checking svn…" : svn.version ? `svn ${svn.version}` : "svn unavailable"}
        </div>
        <div className="foot-row">
          <div className="ident">
            <div><div className="t">ReleaseDock</div><div className="v">Version {info?.version ?? ""}</div></div>
          </div>
          <button className="icon-btn" title="Add Product…"><PlusIcon /></button>
          <button className="icon-btn" title="Settings…"><GearIcon /></button>
        </div>
      </div>
    </aside>
  );
}

/** "Radius Booking - AI-Powered …" → "Radius Booking", as the preview shows. */
export function shortName(name: string): string {
  return name.split(/\s[–-]\s/)[0];
}
