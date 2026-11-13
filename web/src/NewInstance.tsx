// "New instance": ask what the machine must have (vCPUs, memory, a GPU or not, the OS, the hours a month it runs) and offer the
// types that fit, cheapest first, priced at those hours. The same dialog changes an existing instance ("Recommend a type…").
import { useEffect, useState } from "react";
import { api, errText, type AppState, type Candidate } from "./api";

export const HOUR_PRESETS: [number, string][] = [
  [730, "always on (730 h)"],
  [176, "working hours: 8 h × 22 days (176 h)"],
  [320, "about 10 h a day (320 h)"],
  [60, "a few evenings (60 h)"],
  [8, "one day (8 h)"],
];
export const hoursText = (h: number) => HOUR_PRESETS.find(([v]) => v === h)?.[1] ?? `${h} h a month`;

export interface Wish { vcpu: number; ram: number; gpu: boolean; os: string; hours: number }

export function NewInstance({ st, region, title, initial, onPick, onClose, toast }: {
  st: AppState; region: string; title: string; initial?: Partial<Wish>;
  onPick: (c: { type: string; os: string; hours: number }) => void; onClose: () => void; toast: (m: string) => void;
}) {
  const [w, setW] = useState<Wish>({ vcpu: 2, ram: 1, gpu: false, os: "al2023", hours: 730, ...initial });
  const [list, setList] = useState<Candidate[] | null>(null);
  const custom = !HOUR_PRESETS.some(([v]) => v === w.hours);
  const set = <K extends keyof Wish>(k: K, v: Wish[K]) => setW((x) => ({ ...x, [k]: v }));

  useEffect(() => {
    const t = window.setTimeout(() => api.instanceRecommend(w.vcpu, w.ram, w.gpu, w.hours, region).then(setList).catch((e) => toast(errText(e))), 120);
    return () => window.clearTimeout(t);
  }, [w.vcpu, w.ram, w.gpu, w.hours, region, toast]);

  return (
    <div className="modal" onClick={onClose}>
      <div className="card new-inst" onClick={(e) => e.stopPropagation()}>
        <h3 style={{ marginTop: 0 }}>{title}</h3>
        <div className="row">
          <label style={{ flex: 1 }}>vCPUs (at least)<select value={w.vcpu} onChange={(e) => set("vcpu", Number(e.target.value))}>{[1, 2, 4, 8].map((n) => <option key={n} value={n}>{n}</option>)}</select></label>
          <label style={{ flex: 1 }}>Memory, GiB (at least)<select value={w.ram} onChange={(e) => set("ram", Number(e.target.value))}>{[0.5, 1, 2, 4, 8, 16, 32, 64].map((n) => <option key={n} value={n}>{n}</option>)}</select></label>
        </div>
        <div className="row">
          <label className="inline" style={{ flex: 1 }}><input type="checkbox" checked={w.gpu} onChange={(e) => set("gpu", e.target.checked)} />GPU (training, CUDA)</label>
          <label style={{ flex: 1 }}>Operating system<select value={w.os} onChange={(e) => set("os", e.target.value)}>{st.os.map((o) => <option key={o.key} value={o.key}>{o.title}</option>)}</select></label>
        </div>
        <label>Hours a month it will run (the price follows this)
          <select value={custom ? "custom" : w.hours} onChange={(e) => set("hours", e.target.value === "custom" ? 100 : Number(e.target.value))}>
            {HOUR_PRESETS.map(([v, t]) => <option key={v} value={v}>{t}</option>)}<option value="custom">custom…</option>
          </select>
        </label>
        {custom && <label>Hours (1 to 744)<input type="number" min={1} max={744} value={w.hours} onChange={(e) => set("hours", Math.max(1, Math.min(744, Number(e.target.value) || 1)))} /></label>}
        <h4>Types that fit, cheapest first <span className="small muted">({region}, on-demand; Spot about 30 % of this)</span></h4>
        {list === null && <p className="small muted">Looking…</p>}
        {list && list.length === 0 && <p className="msg warn small">Nothing in the table has that much{w.gpu ? " with a GPU" : ""}: ask for less, or {w.gpu ? "" : "tick GPU for the bigger machines, or "}type the instance type by hand on the block afterwards.</p>}
        {list && list.length > 0 && <table className="grid small"><thead><tr><th>Type</th><th>vCPU</th><th>GiB</th><th>Arch</th><th>USD/h</th><th>USD / month at {w.hours} h</th><th></th></tr></thead><tbody>
          {list.map((c, k) => <tr key={c.type} className={k === 0 ? "best" : ""}>
            <td><strong>{c.type}</strong><div className="small muted">{c.note}</div></td><td>{c.vcpu}</td><td>{c.memory}</td><td>{c.arch}</td><td>{c.hourly.toFixed(4)}</td><td><strong>{c.monthly.toFixed(2)}</strong></td>
            <td><button className={`small ${k === 0 ? "primary" : ""}`} onClick={() => onPick({ type: c.type, os: w.os, hours: w.hours })}>Use</button></td>
          </tr>)}
        </tbody></table>}
        <p className="small muted">Every price is an estimate: the root volume (USD 0.08 per GiB a month) and a public address (USD 0.005/h) come on top; a stopped instance pays only for its volume.</p>
        <div className="row" style={{ justifyContent: "flex-end" }}><button onClick={onClose}>Cancel</button></div>
      </div>
    </div>
  );
}
