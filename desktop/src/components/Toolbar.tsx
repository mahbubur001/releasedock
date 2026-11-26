import { ReactNode } from "react";

interface Props {
  title: string;
  subtitle?: ReactNode;
  stale?: boolean;
  children?: ReactNode;
}

/** Window toolbar: title with a freshness line, then screen controls. Draggable on macOS. */
export function Toolbar({ title, subtitle, stale, children }: Props) {
  return (
    <div className="toolbar" data-tauri-drag-region>
      <div className="tb-title" data-tauri-drag-region>
        <div className="t">{title}</div>
        {subtitle && <div className={`s ${stale ? "stale" : ""}`}><span className="dot" />{subtitle}</div>}
      </div>
      {children}
    </div>
  );
}
