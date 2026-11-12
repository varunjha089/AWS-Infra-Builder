// Account: what the AWS account has (read-only), and Import of one VPC onto the canvas as a new unsaved project.
import { useEffect, useState } from "react";
import { api, errText, type AppState, type AwsState, type VpcInfo } from "./api";

export function AccountTab({ st, toast, onImported }: { st: AppState; toast: (m: string) => void; onImported: (notes: string[]) => void }) {
  const [aws, setAws] = useState<AwsState | null>(null);
  const [region, setRegion] = useState("");
  const [vpcs, setVpcs] = useState<VpcInfo[] | null>(null);
  const [busy, setBusy] = useState("");
  useEffect(() => { api.awsState().then((a) => { setAws(a); setRegion(a.region || a.identity?.region || "ap-south-1"); }).catch((e) => toast(errText(e))); }, [toast]);
  if (!aws) return <div className="page muted">Loading…</div>;
  const list = () => { setBusy("list"); api.awsListVpcs(region).then((r) => setVpcs(r.vpcs)).catch((e) => toast(errText(e))).finally(() => setBusy("")); };
  const imp = (v: VpcInfo) => {
    setBusy(v.id);
    api.awsImport(v.id, region).then((r) => { toast(`Imported ${r.project.name} as an unsaved project.`); onImported(r.notes); }).catch((e) => toast(errText(e))).finally(() => setBusy(""));
  };
  return (
    <div className="page">
      <h2>Account</h2>
      <p className="muted">Everything here is read-only (<code>ec2:Describe*</code>). Importing draws an existing VPC on the canvas, with its subnets in their AZs, the tiers worked out from the routes, and the cost it already incurs each month; the account is not touched.</p>
      {aws.source.kind === "none" ? <div className="msg warn">Choose credentials in the Connection tab first.</div>
        : <div className="msg info">Using {aws.source.kind === "profile" ? `profile ${aws.source.name}` : "the pasted keys"}{aws.identity ? ` · account ${aws.identity.account}` : " · not tested yet (Connection → Test)"}</div>}
      <div className="row">
        <label className="row" style={{ margin: 0 }}>Region
          <select value={region} onChange={(e) => { setRegion(e.target.value); setVpcs(null); }}>{st.regions.map((r) => <option key={r.id} value={r.id}>{r.id} · {r.name}</option>)}</select></label>
        <button className="primary" disabled={!!busy || aws.source.kind === "none"} onClick={list}>{busy === "list" ? "Asking AWS…" : "List VPCs"}</button>
      </div>
      {vpcs && (vpcs.length === 0 ? <p className="muted">No VPCs in {region}.</p> : (
        <table className="grid"><thead><tr><th>VPC</th><th>Name</th><th>CIDR</th><th>Subnets</th><th></th></tr></thead>
          <tbody>{vpcs.map((v) => (
            <tr key={v.id}><td><code>{v.id}</code>{v.is_default ? <span className="pill" style={{ marginLeft: 6 }}>default</span> : null}</td><td>{v.name ?? <span className="muted">—</span>}</td><td>{v.cidr}</td><td>{v.subnets}</td>
              <td><button className="small" disabled={!!busy} onClick={() => imp(v)}>{busy === v.id ? "Importing…" : "Import →"}</button></td></tr>))}
          </tbody></table>))}
      <h3>What import does and does not do</h3>
      <ul className="muted small">
        <li>Reads the VPC, its subnets, Internet Gateways, NAT Gateways and route tables with their routes and associations.</li>
        <li>A subnet whose table sends 0.0.0.0/0 to an Internet Gateway is drawn public, to a NAT Gateway private, otherwise isolated.</li>
        <li>Routes to peering connections, endpoints, instances or transit gateways are listed in the import notes and left off the canvas for now.</li>
        <li>The project remembers the account and region it came from; later versions refuse to apply it to another account.</li>
      </ul>
    </div>
  );
}
