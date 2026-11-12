// Costs: what the design costs a month while idle, block by block, and how to pay less. Every number is an estimate and says so.
import type { Analysis, Design } from "./api";

const usd = (v: number) => `USD ${v.toFixed(2)}`;

export function CostsTab({ design, analysis }: { design: Design; analysis: Analysis }) {
  const c = analysis.cost;
  const free = design.nodes.filter((n) => !c.items.some((i) => i.id === n.id));
  return (
    <div className="page">
      <h2>Costs <span className="muted" style={{ fontSize: 15, fontWeight: 400 }}>estimate, {c.region}</span></h2>
      <div className="card">
        <div className="row"><span style={{ fontSize: 26, fontWeight: 700 }}>{usd(c.total)}</span><span className="muted">a month while idle, before any traffic</span></div>
        <p className="muted small">The cheapest design that does the job is the default here: VPCs, subnets, Internet Gateways and route tables are free, so a network costs nothing until a NAT Gateway, a load balancer or an instance is added.</p>
      </div>
      <h3>Per block</h3>
      <table className="grid"><thead><tr><th>Block</th><th>Kind</th><th className="num">USD / month</th><th>How</th></tr></thead>
        <tbody>
          {c.items.map((i) => <tr key={i.id}><td>{i.label}</td><td>{i.kind === "nat" ? "NAT Gateway" : i.kind}</td><td className="num">{i.monthly.toFixed(2)}</td><td className="muted small">{i.note}</td></tr>)}
          {free.length > 0 && <tr><td colSpan={2} className="muted">{free.length} other block{free.length === 1 ? "" : "s"} (VPC, subnets, Internet Gateway, route tables)</td><td className="num">0.00</td><td className="muted small">free</td></tr>}
          <tr><th colSpan={2}>Total</th><th className="num">{c.total.toFixed(2)}</th><th></th></tr>
        </tbody></table>
      <h3>How to pay less</h3>
      {c.advice.length === 0 ? <p className="muted">Nothing to save: the design has no idle cost.</p>
        : <ul className="problems">{c.advice.map((a, i) => <li key={i}>{a}</li>)}</ul>}
      <h3>Assumptions</h3>
      <ul className="muted small">{c.assumptions.map((a, i) => <li key={i}>{a}</li>)}</ul>
    </div>
  );
}
