// "New instance": ask what the machine must have (vCPUs, memory, a GPU or not, the OS, the key pair, the hours a month it
// runs) and offer the types that fit, cheapest first, priced at those hours. Every value is the user's to type; what the app
// cannot vouch for becomes a warning, never a refusal. The same dialog changes an existing instance ("Recommend a type…").
import { useEffect, useState } from "react";
import { api, errText, type AppState, type Candidate, type KeyPair } from "./api";

export const HOUR_PRESETS: [number, string][] = [
  [730, "always on (730 h)"],
  [176, "working hours: 8 h × 22 days (176 h)"],
  [320, "about 10 h a day (320 h)"],
  [60, "a few evenings (60 h)"],
  [8, "one day (8 h)"],
];
export const hoursText = (h: number) => HOUR_PRESETS.find(([v]) => v === h)?.[1] ?? `${h} h a month`;

export interface Wish { vcpu: number; ram: number; gpu: boolean; os: string; hours: number; key: string }
export interface Pick { type: string; os: string; hours: number; key: string }

export function NewInstance({ st, region, title, initial, keyNames, onPick, onClose, toast }: {
  st: AppState; region: string; title: string; initial?: Partial<Wish>; keyNames: string[];
  onPick: (c: Pick) => void; onClose: () => void; toast: (m: string) => void;
}) {
  const [w, setW] = useState<Wish>({ vcpu: 2, ram: 1, gpu: false, os: "al2023", hours: 730, key: "", ...initial });
  const [list, setList] = useState<Candidate[] | null>(null);
  const [byHand, setByHand] = useState("");
  const [pairs, setPairs] = useState<KeyPair[] | null>(null);
  const [busy, setBusy] = useState(false);
  const custom = !HOUR_PRESETS.some(([v]) => v === w.hours);
  const set = <K extends keyof Wish>(k: K, v: Wish[K]) => setW((x) => ({ ...x, [k]: v }));
  const osInfo = st.os.find((o) => o.key === w.os);
  const known = st.instance_types.find((t) => t.type === byHand.trim());
  const names = Array.from(new Set([...keyNames, ...(pairs?.map((p) => p.name) ?? [])])).sort();

  useEffect(() => {
    const t = window.setTimeout(() => api.instanceRecommend(Math.max(1, Math.round(w.vcpu)), Math.max(0.25, w.ram), w.gpu, w.hours, region).then(setList).catch((e) => toast(errText(e))), 120);
    return () => window.clearTimeout(t);
  }, [w.vcpu, w.ram, w.gpu, w.hours, region, toast]);

  // what the values would mean, said before the choice, never enforced
  const warnings: string[] = [];
  if (w.vcpu < 1 || w.ram < 0.25) warnings.push("An instance has at least 1 vCPU and 0.5 GiB.");
  if (w.vcpu > 0 && w.vcpu % 2 === 1 && w.vcpu > 1) warnings.push(`AWS sells even vCPU counts (2, 4, 8…): ${w.vcpu} rounds up to ${w.vcpu + 1}.`);
  if (w.os.startsWith("dlami") && !w.gpu) warnings.push("The Deep Learning AMI is meant for GPU machines: tick GPU, or choose Ubuntu for a CPU box.");
  if (osInfo?.arch === "x86_64") warnings.push(`${osInfo.title} runs on x86_64 only: the Graviton (arm64) types are left out.`);
  if (osInfo && osInfo.min_disk > 8) warnings.push(`${osInfo.title} needs a root volume of at least ${osInfo.min_disk} GiB: the block is made with that.`);
  if (w.key && !names.includes(w.key)) warnings.push(`No key pair "${w.key}" is known here (not in ~/.ssh${pairs ? " nor in AWS" : ""}). It must exist in ${region} before Apply, or make it in the Terminal tab.`);
  if (byHand.trim() && !known) warnings.push(`"${byHand.trim()}" is not in the app's table: its price stays unknown and the app cannot check that it exists in ${region}. It is used as written.`);
  const shown = (list ?? []).filter((c) => osInfo?.arch !== "x86_64" || c.arch === "x86_64");

  const pick = (type: string) => onPick({ type, os: w.os, hours: w.hours, key: w.key.trim() });
  const listPairs = () => { setBusy(true); api.awsKeyPairs(region).then((r) => setPairs(r.pairs)).catch((e) => toast(errText(e))).finally(() => setBusy(false)); };

  return (
    <div className="modal" onClick={onClose}>
      <div className="card new-inst" onClick={(e) => e.stopPropagation()}>
        <h3 style={{ marginTop: 0 }}>{title}</h3>
        <div className="row">
          <label style={{ flex: 1 }}>vCPUs (at least)<input type="number" min={1} max={192} step={1} value={w.vcpu} onChange={(e) => set("vcpu", Number(e.target.value))} /></label>
          <label style={{ flex: 1 }}>Memory, GiB (at least)<input type="number" min={0.5} max={1024} step={0.5} value={w.ram} onChange={(e) => set("ram", Number(e.target.value))} /></label>
          <label className="inline" style={{ flex: 1, alignSelf: "flex-end" }}><input type="checkbox" checked={w.gpu} onChange={(e) => set("gpu", e.target.checked)} />GPU (CUDA, training)</label>
        </div>
        <label>Operating system / image<select value={w.os} onChange={(e) => set("os", e.target.value)}>{st.os.map((o) => <option key={o.key} value={o.key}>{o.title}</option>)}</select></label>
        {osInfo?.note && <div className="small muted" style={{ marginTop: -2 }}>{osInfo.note}.</div>}
        <div className="row" style={{ alignItems: "flex-end" }}>
          <label style={{ flex: 1 }}>Key pair name (empty = none, use SSM)
            <input list="aib-keynames" value={w.key} onChange={(e) => set("key", e.target.value)} placeholder={names[0] ?? "my_key"} />
            <datalist id="aib-keynames">{names.map((n) => <option key={n} value={n} />)}</datalist>
          </label>
          <button className="small" disabled={busy} onClick={listPairs} title="ec2:DescribeKeyPairs, read-only">{pairs ? `${pairs.length} in AWS` : "Names from AWS"}</button>
        </div>
        <div className="small muted">Suggested: the key files in ~/.ssh{pairs ? " and the pairs in the account" : ""}. A new pair can be made in the Terminal tab.</div>
        <label>Hours a month it will run (the price follows this)
          <select value={custom ? "custom" : w.hours} onChange={(e) => set("hours", e.target.value === "custom" ? 100 : Number(e.target.value))}>
            {HOUR_PRESETS.map(([v, t]) => <option key={v} value={v}>{t}</option>)}<option value="custom">custom…</option>
          </select>
        </label>
        {custom && <label>Hours (1 to 744)<input type="number" min={1} max={744} value={w.hours} onChange={(e) => set("hours", Number(e.target.value))} /></label>}
        {warnings.map((m) => <div key={m} className="msg warn small">{m}</div>)}
        <h4>Types that fit, cheapest first <span className="small muted">({region}, on-demand; Spot about 30 % of this)</span></h4>
        {list === null && <p className="small muted">Looking…</p>}
        {list && shown.length === 0 && <p className="msg warn small">Nothing in the app's table fits: type the instance type below; it is used as written.</p>}
        {shown.length > 0 && shown.some((c) => !c.fits) && <p className="msg warn small">Nothing in the app's table has that much; these are the biggest it knows. A bigger type can be typed below.</p>}
        {shown.length > 0 && <table className="grid small"><thead><tr><th>Type</th><th>vCPU</th><th>GiB</th><th>Arch</th><th>USD/h</th><th>USD / month at {w.hours} h</th><th></th></tr></thead><tbody>
          {shown.map((c, k) => <tr key={c.type} className={k === 0 && c.fits ? "best" : ""}>
            <td><strong>{c.type}</strong><div className="small muted">{c.note}</div></td><td>{c.vcpu}</td><td>{c.memory}</td><td>{c.arch}</td><td>{c.hourly.toFixed(4)}</td><td><strong>{c.monthly.toFixed(2)}</strong></td>
            <td><button className={`small ${k === 0 && c.fits ? "primary" : ""}`} onClick={() => pick(c.type)}>Use</button></td>
          </tr>)}
        </tbody></table>}
        <div className="row" style={{ alignItems: "flex-end" }}>
          <label style={{ flex: 1 }}>Or an instance type by hand (any, such as m7i.large)<input value={byHand} onChange={(e) => setByHand(e.target.value)} placeholder="family.size" /></label>
          <button className="small" disabled={!/^[a-z0-9-]+\.[a-z0-9]+$/.test(byHand.trim())} onClick={() => pick(byHand.trim())}>Use it{known ? ` (USD ${((region === "ap-south-1" ? known.ap_south_1 : known.us_east_1) * w.hours).toFixed(2)}/month)` : ""}</button>
        </div>
        <p className="small muted">Every price is an estimate: the root volume (USD 0.08 per GiB a month) and a public address (USD 0.005/h) come on top; a stopped instance pays only for its volume.</p>
        <div className="row" style={{ justifyContent: "flex-end" }}><button onClick={onClose}>Cancel</button></div>
      </div>
    </div>
  );
}
