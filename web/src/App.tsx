// The window: top bar with the project and the tabs, the Projects page, undo/redo, menu events and messages.
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toggleTheme, useTheme } from "./theme";
import { api, errText, type Analysis, type AppState, type Design, type Loaded } from "./api";
import { DesignTab } from "./Design";
import { TextTab } from "./TextTab";
import { CostsTab } from "./CostsTab";
import { CodeTab } from "./CodeTab";
import { ConnectionTab } from "./ConnectionTab";
import { AccountTab } from "./AccountTab";

type Tab = "project" | "design" | "text" | "code" | "costs" | "account" | "connection";

export default function App() {
  const [st, setSt] = useState<AppState | null>(null);
  const [tab, setTab] = useState<Tab>("design");
  const theme = useTheme();
  const [design, setDesign] = useState<Design | null>(null);
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [selectId, setSelectId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState("");
  const [signal, setSignal] = useState<{ fit: number }>({ fit: 0 });
  const timers = useRef<Record<string, number>>({});
  // Undo / Redo: snapshots before each change; changes in quick succession (typing a CIDR) count as one step
  const past = useRef<Design[]>([]), future = useRef<Design[]>([]), lastPush = useRef(0);
  const designRef = useRef<Design | null>(null);
  designRef.current = design;

  const toast = useCallback((m: string) => {
    setMsg(m);
    window.clearTimeout(timers.current.toast);
    timers.current.toast = window.setTimeout(() => setMsg(""), Math.min(14000, 3500 + m.length * 40));
  }, []);
  const remember = useCallback((prev: Design) => {
    const now = Date.now();
    if (now - lastPush.current > 600 || past.current.length === 0) {
      past.current.push(prev);
      while (past.current.length > 100) past.current.shift();
    }
    lastPush.current = now;
    future.current = [];
  }, []);
  /** a result from the Rust side (checked, text in step) replaces the design, unless something changed meanwhile */
  const take = useCallback((l: Loaded, sent?: Design) => {
    setAnalysis(l.analysis);
    if (!sent || designRef.current === sent) setDesign(l.design);
  }, []);
  // save the design; the Rust side keeps its text in step and runs the checks
  const persist = useCallback((d: Design) => api.saveDesign(d).then((l) => take(l, d)).catch((e) => toast(errText(e))), [take, toast]);
  /** a change made on the canvas (one undo step); `later` = write after a pause (typing) */
  const change = useCallback((d: Design, later = false) => {
    if (designRef.current) remember(designRef.current);
    setDesign(d);
    window.clearTimeout(timers.current.persist);
    if (later) timers.current.persist = window.setTimeout(() => persist(d), 300);
    else persist(d);
  }, [persist, remember]);
  /** a whole new state from the Rust side (add, connect, remove, arrange): one undo step */
  const apply = useCallback((l: Loaded) => {
    if (designRef.current) remember(designRef.current);
    lastPush.current = 0;
    setDesign(l.design);
    setAnalysis(l.analysis);
  }, [remember]);
  const applyText = useCallback(async (text: string) => {
    const r = await api.designFromText(text);
    if (r.ok && r.design && r.analysis) apply({ design: r.design, analysis: r.analysis });
    return r.errors ?? [];
  }, [apply]);

  const reload = useCallback(async () => { const s = await api.state(); setSt(s); return s; }, []);
  const loadProject = useCallback(async () => {
    past.current = []; future.current = [];
    const l = await api.load();
    setDesign(l.design); setAnalysis(l.analysis);
    setSignal((s) => ({ fit: s.fit + 1 }));
  }, []);

  useEffect(() => {
    reload().then((s) => (s.project ? loadProject().then(() => setTab("design")) : setTab("project"))).catch((e) => toast(errText(e)));
    const un = listen<string>("open-tab", (e) => setTab(e.payload as Tab));
    const un2 = listen<string>("menu", (e) => menuRef.current(e.payload));
    return () => { [un, un2].forEach((u) => u.then((f) => f())); };
  }, [reload, loadProject, toast]);

  const typing = () => { const a = document.activeElement; return !!a && (a.tagName === "INPUT" || a.tagName === "TEXTAREA" || a.tagName === "SELECT"); };
  const menuRef = useRef<(id: string) => void>(() => {});
  menuRef.current = (id: string) => {
    const d = designRef.current;
    if (id === "undo" || id === "redo") {
      if (typing()) { document.execCommand(id); return; }
      const [from, to] = id === "undo" ? [past.current, future.current] : [future.current, past.current];
      const target = from.pop();
      if (!target || !d) { toast(id === "undo" ? "Nothing to undo." : "Nothing to redo."); return; }
      to.push(d);
      lastPush.current = 0;
      setDesign(target);
      persist(target);
      if (tab !== "design") setTab("design");
    } else if (id === "arrange" && d) {
      api.designArrange(d).then(apply).then(() => setSignal((s) => ({ fit: s.fit + 1 }))).catch((e) => toast(errText(e)));
      setTab("design");
    } else if (id === "fit") {
      setTab("design"); setSignal((s) => ({ fit: s.fit + 1 }));
    } else if (id.startsWith("add-") && d) {
      window.dispatchEvent(new CustomEvent("aib-add", { detail: id.slice(4) }));
      setTab("design");
    } else if (id === "new-project") {
      setTab("project");
      window.setTimeout(() => document.getElementById("newProjectName")?.focus(), 50);
    } else if (id === "open-project") {
      api.pickFolder().then((p) => (p ? api.projectOpen(p).then(() => reload()).then(() => loadProject()).then(() => setTab("design")) : undefined)).catch((e) => toast(errText(e)));
    } else if (id === "save-project") {
      if (st?.project?.draft) setSaving(true);
      else toast(st?.project ? "Already saved: every change is written as you make it." : "Open or create a project first.");
    } else if (id === "close-project") {
      api.projectClose().then(() => reload()).then(() => { setDesign(null); setAnalysis(null); setTab("project"); }).catch((e) => toast(errText(e)));
    } else if (id === "night") {
      toggleTheme();
    } else if (id === "export-code") {
      api.exportCode().then((p) => toast(`Written to ${p}`)).catch((err) => toast(errText(err)));
    }
  };

  if (!st) return <div className="page muted">Loading…</div>;
  const tabs: [Tab, string][] = [["design", "Design"], ["text", "Text"], ["code", "Code"], ["costs", "Costs"]];
  const summary = analysis ? (analysis.ok ? (analysis.warnings ? `${analysis.warnings} warning${analysis.warnings > 1 ? "s" : ""}` : "design is clean") : `${analysis.errors} error${analysis.errors > 1 ? "s" : ""}`) : "";

  return (
    <div className="app">
      <header>
        <strong className="brand">AWS Infra Builder</strong>
        <button className={`tab proj ${tab === "project" ? "on" : ""}`} onClick={() => setTab("project")}>{st.project ? st.project.name : "No project"} ▾</button>
        {st.project?.draft && <><span className="pill warn" title="Discarded when you open another project or quit, unless you save it">not saved</span>
          <button className="small primary" onClick={() => setSaving(true)}>Save…</button></>}
        <nav>{st.project && tabs.map(([t, label]) => <button key={t} className={`tab ${tab === t ? "on" : ""}`} onClick={() => setTab(t)}>{label}</button>)}</nav>
        <span className="grow" />
        {analysis && st.project && <span className={`summary ${analysis.ok ? (analysis.warnings ? "st-warn" : "st-ok") : "st-error"}`}>{design?.region} · {analysis.counts.subnet ?? 0} subnets in {analysis.az_spread} AZ{analysis.az_spread === 1 ? "" : "s"} · ≈ USD {analysis.cost.total.toFixed(0)}/month · {summary}</span>}
        <button className="tab" title={theme === "night" ? "Day mode (⇧⌘N)" : "Night mode (⇧⌘N)"} onClick={toggleTheme}>{theme === "night" ? "☀" : "☾"}</button>
        <button className={`tab ${tab === "account" ? "on" : ""}`} onClick={() => setTab("account")}>Account</button>
        <button className={`tab conn-tab ${tab === "connection" ? "on" : ""}`} onClick={() => setTab("connection")}>Connection</button>
      </header>
      <main>
        {tab === "project" && <ProjectsPage st={st} reload={reload} toast={toast} onOpened={() => loadProject().then(() => setTab("design")).catch((e) => toast(errText(e)))} />}
        {tab === "design" && design && analysis && st.project && <DesignTab design={design} analysis={analysis} st={st} change={change} apply={apply} toast={toast} selectId={selectId} setSelectId={setSelectId} signal={signal} />}
        {tab === "text" && design && <TextTab design={design} onApply={applyText} onFormat={() => design && persist(design)} />}
        {tab === "code" && design && analysis && <CodeTab design={design} analysis={analysis} toast={toast} />}
        {tab === "costs" && design && analysis && <CostsTab design={design} analysis={analysis} />}
        {tab === "connection" && <ConnectionTab st={st} toast={toast} />}
        {tab === "account" && <AccountTab st={st} toast={toast} onImported={(notes) => reload().then(() => loadProject()).then(() => { setTab("design"); toast(notes.join("\n")); }).catch((e) => toast(errText(e)))} />}
        {!["project", "connection", "account"].includes(tab) && !st.project && <div className="page muted">Open or create a project first.</div>}
      </main>
      {msg && <div className="toast" onClick={() => setMsg("")}>{msg}</div>}
      {saving && st.project && <SaveAs st={st} name={st.project.name} onDone={(ok) => { setSaving(false); if (ok) reload().catch(() => {}); }} toast={toast} />}
    </div>
  );
}

function ProjectsPage({ st, reload, toast, onOpened }: { st: AppState; reload: () => Promise<AppState>; toast: (m: string) => void; onOpened: () => void }) {
  const [name, setName] = useState("");
  const open = (p: Promise<unknown>) => p.then(() => reload()).then(onOpened).catch((e) => toast(errText(e)));
  return (
    <div className="page">
      <h2>Projects</h2>
      <p className="muted">A project is a folder with <code>project.json</code> and <code>design.json</code>. Nothing here talks to AWS: the design computer designs.</p>
      <h3>Start from a template</h3>
      <div className="demos">{st.templates.map((t) => (
        <button key={t.key} className="demo" onClick={() => open(api.projectFromTemplate(t.key))}>
          <strong>{t.title}</strong><p>{t.about}</p>
          <div className="dfoot"><span className="muted">opens unsaved; Save… keeps it</span><span className="go">Open →</span></div>
        </button>))}
      </div>
      <h3>New empty project</h3>
      <div className="row">
        <input id="newProjectName" placeholder="name" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) open(api.projectNew(name)); }} />
        <button className="primary" disabled={!name.trim()} onClick={() => open(api.projectNew(name))}>Create</button>
        <span className="muted small">in {st.projects_dir}</span>
        <button className="small" onClick={() => api.pickFolder().then((p) => (p ? api.setProjectsDir(p).then(() => reload()) : undefined)).catch((e) => toast(errText(e)))}>Change folder…</button>
      </div>
      <h3>Your projects</h3>
      <div className="row"><button onClick={() => api.pickFolder().then((p) => (p ? open(api.projectOpen(p)) : undefined)).catch((e) => toast(errText(e)))}>Open a project folder…</button></div>
      {st.projects.length === 0 && <p className="muted">None yet.</p>}
      <div className="demos">{st.projects.map((p) => (
        <div key={p.path} className={`demo ptile ${p.exists ? "" : "gone"} ${st.project?.path === p.path ? "on" : ""}`}>
          <strong>{p.name}{st.project?.path === p.path ? " (open)" : ""}</strong>
          <div className="ppath muted small" title={p.path}>{p.path}</div>
          <div className="dfoot">
            {!p.exists && <span className="pill error">folder missing</span>}
            <button className="small" onClick={() => api.projectForget(p.path).then(() => reload()).catch((e) => toast(errText(e)))} title="take it off this list; the folder stays">Forget</button>
            {p.exists && <button className="go" onClick={() => open(api.projectOpen(p.path))}>Open →</button>}
          </div>
        </div>))}
      </div>
    </div>
  );
}

function SaveAs({ st, name: initial, onDone, toast }: { st: AppState; name: string; onDone: (ok: boolean) => void; toast: (m: string) => void }) {
  const [name, setName] = useState(initial);
  const [where, setWhere] = useState("");
  const save = () => api.projectSaveAs(name, where || undefined).then((p) => { toast(`Saved in ${p.path}`); onDone(true); }).catch((e) => toast(errText(e)));
  return (
    <div className="modal" onClick={() => onDone(false)}>
      <div className="card" onClick={(e) => e.stopPropagation()}>
        <h3 style={{ marginTop: 0 }}>Save the project</h3>
        <div className="form">
          <label className="wide">Name<input autoFocus value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") save(); }} /></label>
          <label className="wide">Folder<span className="row" style={{ margin: 0 }}><span className="muted small">{where || st.projects_dir}</span>
            <button className="small" onClick={() => api.pickFolder().then((p) => p && setWhere(p)).catch(() => {})}>Choose…</button></span></label>
        </div>
        <div className="row"><button className="primary" disabled={!name.trim()} onClick={save}>Save</button><button onClick={() => onDone(false)}>Cancel</button></div>
      </div>
    </div>
  );
}
