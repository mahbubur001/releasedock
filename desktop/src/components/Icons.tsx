// Line icons from the Layout v2 preview.
type Props = { className?: string };
const stroke = { fill: "none", stroke: "currentColor", strokeWidth: 1.4 } as const;

export const OverviewIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" {...stroke} className={p.className}><rect x="2" y="2" width="5" height="5" rx="1" /><rect x="9" y="2" width="5" height="5" rx="1" /><rect x="2" y="9" width="5" height="5" rx="1" /><rect x="9" y="9" width="5" height="5" rx="1" /></svg>
);
export const ReleaseIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" {...stroke} className={p.className}><path d="M14 2 7 9M14 2l-4.5 12L7 9 2 6.5z" /></svg>
);
export const HistoryIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" {...stroke} className={p.className}><circle cx="8" cy="8" r="6" /><path d="M8 4.5V8l2.5 1.5" /></svg>
);
export const PlusIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.6} className={p.className}><path d="M8 3v10M3 8h10" /></svg>
);
export const GearIcon = (p: Props) => (
  <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.5} className={p.className}><circle cx="10" cy="10" r="2.6" /><circle cx="10" cy="10" r="5.4" /><path d="M10 1.8v2.1M10 16.1v2.1M18.2 10h-2.1M3.9 10H1.8M15.8 4.2l-1.5 1.5M5.7 14.3l-1.5 1.5M15.8 15.8l-1.5-1.5M5.7 5.7 4.2 4.2" /></svg>
);
export const RefreshIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.5} className={p.className}><path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3" /></svg>
);
export const UpdateIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.5} className={p.className}><path d="M8 2.5v9M4.5 8 8 11.5 11.5 8M3 13.5h10" /></svg>
);
export const TerminalIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.5} className={p.className}><rect x="2" y="2.5" width="12" height="11" rx="2" /><path d="m5 6 2 2-2 2M8.5 10.5H11" /></svg>
);
export const PlugIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" fill="currentColor" className={p.className}><path d="M5 1.5h1.5V5h3V1.5H11V5h1v3a4 4 0 0 1-3.25 3.93V14.5h-1.5v-2.57A4 4 0 0 1 4 8V5h1z" /></svg>
);
export const PaletteIcon = (p: Props) => (
  <svg viewBox="0 0 16 16" fill="currentColor" className={p.className}><path d="M8 1.5a6.5 6.5 0 1 0 0 13c.9 0 1.4-.7 1.1-1.5-.3-.9.3-1.7 1.2-1.7H12a2.5 2.5 0 0 0 2.5-2.5C14.5 4.5 11.6 1.5 8 1.5ZM4.5 8.5a1 1 0 1 1 0-2 1 1 0 0 1 0 2Zm2-3a1 1 0 1 1 0-2 1 1 0 0 1 0 2Zm3 0a1 1 0 1 1 0-2 1 1 0 0 1 0 2Zm2 3a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z" /></svg>
);
