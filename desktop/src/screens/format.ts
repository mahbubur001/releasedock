import type { ReleaseRow } from "../api";

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

/** "3 minutes ago", "yesterday", "5 days ago". */
export function ago(date: Date | number): string {
  const seconds = (new Date(date).getTime() - Date.now()) / 1000;
  const steps: [number, Intl.RelativeTimeFormatUnit][] = [[60, "second"], [60, "minute"], [24, "hour"], [7, "day"], [4.35, "week"], [12, "month"]];
  let value = seconds;
  for (const [size, unit] of steps) {
    if (Math.abs(value) < size) return relative.format(Math.round(value), unit);
    value /= size;
  }
  return relative.format(Math.round(value), "year");
}

export const stateLabel: Record<string, string> = {
  detected: "Detected", inspected: "Inspected", validated: "Validated", staged: "Staged", previewed: "Previewed",
  approved: "Approved", publishing: "Publishing", verifying: "Verifying", completed: "Completed",
  failed: "Failed", cancelled: "Cancelled", recoveryRequired: "Needs recovery",
};

export function stateTone(state: string): string {
  if (state === "completed") return "green";
  if (state === "failed") return "red";
  if (state === "recoveryRequired") return "orange";
  if (state === "cancelled") return "gray";
  return "blue";
}

export function releaseTitle(r: ReleaseRow): string {
  return `${r.productName.split(/\s[–-]\s/)[0]} ${r.version ?? ""}`.trim();
}
