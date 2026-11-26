import { ReleaseRow } from "../api";
import { ago, releaseTitle, stateLabel, stateTone } from "./format";

export function History({ releases }: { releases: ReleaseRow[] }) {
  return (
    <div className="tbl-wrap" style={{ paddingTop: 10 }}>
      <table className="table">
        <thead><tr><th>Release</th><th>State</th><th>Kind</th><th>Revision</th><th>When</th></tr></thead>
        <tbody>
          {releases.map((r) => (
            <tr key={r.id}>
              <td><b>{releaseTitle(r)}</b>{r.previousVersion && <span className="faint"> from {r.previousVersion}</span>}</td>
              <td><span className={`tag ${stateTone(r.state)}`}>{stateLabel[r.state] ?? r.state}</span></td>
              <td>{r.mode === "dryRun" ? "Dry run" : r.kind === "readmeUpdate" ? "Readme update" : "Version"}</td>
              <td className="num">{r.committedRevision ? `r${r.committedRevision}` : ""}</td>
              <td>{ago(r.createdAt)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
