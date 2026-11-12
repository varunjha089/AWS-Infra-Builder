// Connection: which AWS credentials the app uses. A profile from ~/.aws (the SDK reads the files; SSO works after `aws sso login`),
// or keys pasted here and kept in the keychain. "Test" calls sts:GetCallerIdentity, nothing else.
import { useEffect, useState } from "react";
import { api, errText, type AppState, type AwsState, type Source } from "./api";

export function ConnectionTab({ st, toast }: { st: AppState; toast: (m: string) => void }) {
  const [aws, setAws] = useState<AwsState | null>(null);
  const [id, setId] = useState(""), [secret, setSecret] = useState(""), [token, setToken] = useState("");
  const [region, setRegion] = useState("");
  const [busy, setBusy] = useState(false);
  const reload = () => api.awsState().then((a) => { setAws(a); setRegion((r) => r || a.region || a.identity?.region || "ap-south-1"); }).catch((e) => toast(errText(e)));
  useEffect(() => { reload(); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  if (!aws) return <div className="page muted">Loading…</div>;

  const select = (s: Source) => api.awsSelect(s, region).then(reload).catch((e) => toast(errText(e)));
  const test = () => { setBusy(true); api.awsTest(region).then((i) => { toast(`Connected: account ${i.account} (${i.arn})`); reload(); }).catch((e) => toast(errText(e))).finally(() => setBusy(false)); };
  const same = (s: Source) => JSON.stringify(s) === JSON.stringify(aws.source);
  const kindText: Record<string, string> = { keys: "access key", sso: "SSO (run `aws sso login` first)", role: "assumes a role", process: "credential process", web_identity: "web identity", unknown: "no credentials of its own" };

  return (
    <div className="page conn">
      <h2>Connection</h2>
      <p className="muted">This version only reads: <code>sts:GetCallerIdentity</code> to test, <code>ec2:Describe*</code> to list and import. Nothing is created, changed or deleted in the account.</p>

      <div className="card">
        <h3>1. Credentials</h3>
        <h4>Profiles in {aws.aws_dir}</h4>
        {aws.profiles.length === 0 && <p className="muted">No profiles found. The AWS CLI writes them with <code>aws configure</code> or <code>aws configure sso</code>; or paste keys below.</p>}
        {aws.profiles.map((p) => (
          <label key={p.name} className="row" style={{ margin: "4px 0" }}>
            <input type="radio" name="src" checked={same({ kind: "profile", name: p.name })} onChange={() => select({ kind: "profile", name: p.name })} />
            <strong>{p.name}</strong><span className="muted">{kindText[p.kind] ?? p.kind}{p.key_hint ? ` ${p.key_hint}` : ""}{p.region ? ` · region ${p.region}` : ""}</span>
          </label>))}
        <h4>Keys pasted into the app</h4>
        <label className="row" style={{ margin: "4px 0" }}>
          <input type="radio" name="src" checked={same({ kind: "keys" })} disabled={!aws.keys} onChange={() => select({ kind: "keys" })} />
          {aws.keys ? <><strong>Pasted keys</strong><span className="muted">{aws.keys.hint}{aws.keys.temporary ? " · temporary (with session token)" : ""} · kept in the keychain</span>
            <button className="small danger" onClick={() => api.awsClearKeys().then(reload).catch((e) => toast(errText(e)))}>Remove</button></>
            : <span className="muted">none yet</span>}
        </label>
        <div className="form">
          <label>Access key id<input value={id} onChange={(e) => setId(e.target.value)} placeholder="AKIA… or ASIA…" autoComplete="off" /></label>
          <label>Secret access key<input type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="off" /></label>
          <label>Session token (temporary keys only)<input type="password" value={token} onChange={(e) => setToken(e.target.value)} autoComplete="off" /></label>
        </div>
        <div className="row">
          <button className="primary" disabled={!id.trim() || !secret.trim()} onClick={() => api.awsSetKeys(id, secret, token).then(() => { setId(""); setSecret(""); setToken(""); toast("Keys saved in the keychain and selected."); reload(); }).catch((e) => toast(errText(e)))}>Save keys in the keychain</button>
          <span className="muted small">They are handed to the AWS SDK for one call at a time and never written to a project, a file or a log.</span>
        </div>
      </div>

      <div className="card">
        <h3>2. Region and test</h3>
        <div className="row">
          <label className="row" style={{ margin: 0 }}>Region
            <select value={region} onChange={(e) => setRegion(e.target.value)}>{st.regions.map((r) => <option key={r.id} value={r.id}>{r.id} · {r.name}</option>)}</select></label>
          <button className="primary" disabled={busy || aws.source.kind === "none"} onClick={test}>{busy ? "Asking AWS…" : "Test connection"}</button>
          <span className="muted small">calls sts:GetCallerIdentity with the selected credentials</span>
        </div>
        {aws.identity ? <div className="msg ok">Account <strong>{aws.identity.account}</strong> · {aws.identity.arn} · region {aws.identity.region}</div>
          : <div className="msg info">{aws.source.kind === "none" ? "Choose a profile or paste keys, then test." : "Not tested yet."}</div>}
      </div>

      <div className="card">
        <h3>3. Least privilege</h3>
        <p className="muted">Give the key or role only this read-only policy (IAM → Policies → Create → JSON). The deploy policy of later versions is generated from the design, so it never covers more than the design uses.</p>
        <pre>{aws.policy}</pre>
        <div className="row"><button className="small" onClick={() => navigator.clipboard.writeText(aws.policy).then(() => toast("Policy copied."))}>Copy</button></div>
      </div>
    </div>
  );
}
