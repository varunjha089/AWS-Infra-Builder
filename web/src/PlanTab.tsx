// Plan & Apply: OpenTofu runs plan (read-only), then apply only after the plan is shown, with a typed confirmation when anything
// is destroyed or replaced; destroy is a separate action behind the project name. The log streams in; the plan is drawn on the canvas.
import { useEffect, useState } from "react";
import { api, errText, type Analysis, type AppState, type Design, type JobSnap, type TofuStatus } from "./api";

const usd = (v: number) => `${v >= 0 ? "+" : "−"}USD ${Math.abs(v).toFixed(2)}`;

export function PlanTab({ st, design, analysis, job, startJob, toast, goDesign }: {
  st: AppState; design: Design; analysis: Analysis; job: JobSnap | null; startJob: () => void; toast: (m: string) => void; goDesign: () => void;
}) {
  const [status, setStatus] = useState<TofuStatus | null>(null);
  const [typed, setTyped] = useState("");
  const [acceptBudget, setAcceptBudget] = useState(false);
  const [budget, setBudget] = useState("");
  const reload = () => api.tofuStatus().then((s) => { setStatus(s); setBudget(s.budget ? String(s.budget) : ""); }).catch((e) => toast(errText(e)));
  useEffect(() => { reload(); }, [job?.state]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!status) return <div className="page muted">Loading…</div>;
  const running = job?.state === "running";
  const plan = job?.plan ?? null;
  const planKind = job?.kind ?? "";
  const havePlan = status.state.plan_fresh && plan && (job?.state === "done") && (planKind === "plan" || planKind === "plan-destroy");
  const isDestroyPlan = planKind === "plan-destroy";
  const dangerous = !!plan && (plan.destroys > 0 || plan.replaces > 0);
  /** blocks the plan would create although they already exist in AWS (they carry an id): the app refuses to apply that */
  const duplicates = plan ? Array.from(new Set(plan.changes.filter((c) => (c.action === "create" || c.action === "replace") && c.block_id && design.nodes.find((n) => n.id === c.block_id && n.params.aws_id)).map((c) => c.block_label ?? c.address))) : [];
  const projectName = st.project?.name ?? "";
  const estimatedAfter = analysis.cost.total;
  const overBudget = !!status.budget && estimatedAfter > status.budget && !isDestroyPlan;
  const canApply = !!havePlan && !plan!.empty && !running && (!dangerous || typed.trim() === projectName) && (!overBudget || acceptBudget);
  const go = (p: Promise<unknown>) => p.then(() => { setTyped(""); startJob(); }).catch((e) => toast(errText(e)));
  const guardOk = !status.project_account || !status.identity || status.project_account === status.identity.account;

  return (
    <div className="page">
      <h2>Plan &amp; Apply</h2>
      <p className="muted">Plan is read-only and shows exactly what would change. Apply needs a click after the plan is shown, and the project's name typed when anything would be destroyed or replaced. The state file lives in the project's <code>tofu/</code> folder and is backed up before every apply.</p>

      <div className="card">
        <h3>OpenTofu</h3>
        {status.tofu ? <div className="row"><span className="pill ok">{status.version}</span><span className="muted small">{status.tofu}</span></div>
          : <div className="row"><span className="pill warn">not found</span>
            <button className="primary" disabled={running} onClick={() => go(api.tofuInstall())}>Install OpenTofu {status.install_version} into the app's folder</button>
            <span className="muted small">downloaded from the official release and checked against its SHA-256 (MPL-2.0); or <code>brew install opentofu</code></span></div>}
      </div>

      <div className="card">
        <h3>Account guard and budget</h3>
        <div className="row">
          <span>Credentials: {status.source === "none" ? <span className="pill warn">none (Connection tab)</span> : <span className="pill">{status.source}</span>}</span>
          <span>Connected account: {status.identity ? <code>{status.identity.account}</code> : <span className="muted">not tested (Connection → Test)</span>}</span>
          <span>Project's account: {status.project_account ? <code>{status.project_account}</code> : <span className="muted">none yet: set by the first plan</span>}</span>
        </div>
        {!guardOk && <div className="msg error">The credentials point at account {status.identity?.account}, but this project belongs to {status.project_account}. Plan and apply refuse to run: switch credentials in the Connection tab, or make a new project for the other account.</div>}
        <div className="row">
          <label className="row" style={{ margin: 0 }}>Monthly budget (USD, optional)
            <input type="number" min={0} style={{ width: 110 }} value={budget} onChange={(e) => setBudget(e.target.value)} onBlur={() => api.projectSetBudget(budget ? Number(budget) : null).then(reload).catch((e) => toast(errText(e)))} /></label>
          <span className="muted small">the design's estimate is USD {estimatedAfter.toFixed(2)} a month; an apply above the budget needs an extra tick</span>
        </div>
      </div>

      <div className="card">
        <h3>1. Plan</h3>
        <div className="row">
          <button className="primary" disabled={running || !status.tofu || status.source === "none" || !guardOk || !analysis.ok} onClick={() => go(api.tofuPlan(false))}>Plan</button>
          <button disabled={running || !status.tofu || status.source === "none" || !guardOk || !status.state.has_state} onClick={() => go(api.tofuPlan(true))} title="what destroying everything this project created would remove">Plan destroy</button>
          {!analysis.ok && <span className="st-error">the design has {analysis.errors} error{analysis.errors === 1 ? "" : "s"}: fix them first</span>}
          <span className="muted small">{status.state.has_state ? `state: ${status.state.resources} resources, last apply ${status.state.last_apply ?? "?"}, ${status.state.backups} backup${status.state.backups === 1 ? "" : "s"}` : "nothing applied yet"}</span>
        </div>
        {plan && (job?.kind === "plan" || job?.kind === "plan-destroy") && job?.state === "done" && (
          <>
            <div className={`msg ${plan.empty ? "ok" : dangerous ? "warn" : "info"}`}><strong>{plan.summary}</strong>{!plan.empty && <> · <button className="link" onClick={goDesign}>see it on the canvas</button></>}</div>
            {!plan.empty && <table className="grid"><thead><tr><th>Action</th><th>Resource</th><th>Block</th><th></th></tr></thead><tbody>
              {plan.changes.map((c) => <tr key={c.address} className={`act-${c.action}`}><td><span className={`pill act-${c.action}`}>{c.action}</span></td><td><code>{c.address}</code></td><td>{c.block_label ?? <span className="muted">—</span>}</td>
                <td className="muted small">{c.replace_because.length ? `forces replacement: ${c.replace_because.join(", ")}` : ""}</td></tr>)}
            </tbody></table>}
          </>
        )}
      </div>

      <div className="card">
        <h3>2. {isDestroyPlan ? "Destroy" : "Apply"}</h3>
        {!havePlan && <p className="muted">Run Plan first; the plan is used as saved, so what you saw is what runs.</p>}
        {havePlan && plan && !plan.empty && (
          <>
            {plan.imports > 0 && <div className="msg info">{plan.imports} resource{plan.imports === 1 ? "" : "s"} already in the account {plan.imports === 1 ? "is" : "are"} adopted into the state (import blocks): nothing is created for them and nothing changes on them by that alone.</div>}
            {duplicates.length > 0 && <div className="msg error">This plan would create a second copy of {duplicates.join(", ")}: they already exist in AWS and the design carries their ids. Apply is refused; run Plan again so the import blocks adopt them, or look at the plan table for a replace and its reason.</div>}
            {dangerous && <div className="msg warn">This {isDestroyPlan ? "destroys" : "replaces or destroys"} resources ({plan.destroys} destroyed, {plan.replaces} replaced). Type the project's name, <strong>{projectName}</strong>, to allow it.
              <div className="row"><input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={projectName} /></div></div>}
            {overBudget && <label className="row chk"><input type="checkbox" checked={acceptBudget} onChange={(e) => setAcceptBudget(e.target.checked)} />The estimate (USD {estimatedAfter.toFixed(2)}) is above the budget (USD {status.budget}); apply anyway</label>}
            <div className="row">
              <button className={isDestroyPlan ? "danger" : "primary"} disabled={!canApply || (!isDestroyPlan && duplicates.length > 0)} onClick={() => go(api.tofuApply(typed, acceptBudget))}>{isDestroyPlan ? "Destroy" : "Apply"}: {plan.summary.split(" · ")[0]}</button>
              <span className="muted small">{usd(plan.cost_delta)} a month (estimate)</span>
            </div>
          </>
        )}
      </div>

      {job && job.kind !== "install" && (
        <div className="card">
          <h3>Log <span className={`pill ${job.state === "failed" ? "error" : job.state === "done" ? "ok" : ""}`}>{job.kind} · {job.state}</span></h3>
          <pre className="log" style={{ maxHeight: 320 }}>{job.log.join("\n")}</pre>
          {job.message && <div className={`msg ${job.state === "failed" ? "error" : "ok"}`}>{job.message}</div>}
          {job.outputs.length > 0 && <><h4>Outputs</h4><table className="grid"><tbody>{job.outputs.map(([k, v]) => <tr key={k}><td>{k}</td><td><code>{v}</code></td></tr>)}</tbody></table></>}
        </div>
      )}
      {job && job.kind === "install" && <div className="card"><h3>Install <span className="pill">{job.state}</span></h3><pre className="log">{job.log.join("\n")}</pre>{job.message && <div className={`msg ${job.state === "failed" ? "error" : "ok"}`}>{job.message}</div>}</div>}
    </div>
  );
}
