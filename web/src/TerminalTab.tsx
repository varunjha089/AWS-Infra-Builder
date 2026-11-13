// Terminal: SSH into the design's instances (or any host) from inside the app, like Termius. Each session is the system `ssh`
// running in a pseudo-terminal on the Rust side (term.rs); this file draws it with xterm.js and sends the keys back. The tab
// stays mounted while another tab is shown, so sessions survive a tab switch. Hosts come from the design (one per instance,
// with the user name the OS expects and ~/.ssh/<key pair>.pem) and from the saved list in settings.json.
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { api, errText, type Design, type Host, type HostsState } from "./api";
import { useTheme } from "./theme";

interface Session { id: string; host: Host; term: Terminal; fit: FitAddon; el: HTMLDivElement; open: boolean }

const DAY = { background: "#ffffff", foreground: "#1d2330", cursor: "#2f6fde", cursorAccent: "#ffffff", selectionBackground: "#cfe0ff" };
const NIGHT = {
  background: "#070914", foreground: "#d9f7ff", cursor: "#00e5ff", cursorAccent: "#070914", selectionBackground: "#1b2a4a",
  black: "#0b0f1f", red: "#ff4d6d", green: "#39ff88", yellow: "#ffd166", blue: "#4da3ff", magenta: "#ff4df0", cyan: "#00e5ff", white: "#d9f7ff",
  brightBlack: "#5b6478", brightRed: "#ff8099", brightGreen: "#8bffb8", brightYellow: "#ffe49a", brightBlue: "#8cc4ff", brightMagenta: "#ff8cf5", brightCyan: "#7df9ff", brightWhite: "#ffffff",
};
const blank = (): Host => ({ id: "", label: "", host: "", user: "ec2-user", port: 22, key: "", block: null, aws_id: null, note: "" });
const bytes = (b64: string) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
const grey = (s: string) => `\x1b[90m${s}\x1b[0m`;

export function TerminalTab({ visible, design, toast }: { visible: boolean; design: Design | null; toast: (m: string) => void }) {
  const theme = useTheme();
  const [hs, setHs] = useState<HostsState | null>(null);
  const [form, setForm] = useState<Host>(blank());
  const [keyProblem, setKeyProblem] = useState<string | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const holder = useRef<HTMLDivElement>(null);
  const sessRef = useRef<Session[]>([]);
  sessRef.current = sessions;

  const reload = useCallback(() => api.hostsState(design).then(setHs).catch((e) => toast(errText(e))), [design, toast]);
  useEffect(() => { reload(); }, [reload]);

  // the key file: say what ssh will say before trying
  useEffect(() => {
    if (!form.key.trim()) { setKeyProblem(null); return; }
    const t = window.setTimeout(() => api.keyCheck(form.key).then(setKeyProblem).catch(() => setKeyProblem(null)), 250);
    return () => window.clearTimeout(t);
  }, [form.key]);

  // output and the end of a session
  useEffect(() => {
    const a = listen<{ id: string; data: string }>("term-data", (e) => { sessRef.current.find((s) => s.id === e.payload.id)?.term.write(bytes(e.payload.data)); });
    const b = listen<{ id: string; code: number | null }>("term-exit", (e) => {
      const s = sessRef.current.find((x) => x.id === e.payload.id);
      if (!s) return;
      s.term.write(`\r\n${grey(`[connection closed${e.payload.code != null && e.payload.code !== 0 ? `, exit code ${e.payload.code}` : ""}]`)}\r\n`);
      setSessions((l) => l.map((x) => (x.id === s.id ? { ...x, open: false } : x)));
    });
    return () => { [a, b].forEach((u) => u.then((f) => f())); };
  }, []);

  const fitActive = useCallback(() => {
    const s = sessRef.current.find((x) => x.id === active);
    if (!s || !visible) return;
    try { s.fit.fit(); } catch { /* not measurable yet */ }
  }, [active, visible]);

  // the size follows the window and the tab becoming visible
  useEffect(() => {
    const r = window.requestAnimationFrame(fitActive);
    const ro = new ResizeObserver(() => fitActive());
    if (holder.current) ro.observe(holder.current);
    return () => { window.cancelAnimationFrame(r); ro.disconnect(); };
  }, [fitActive]);

  useEffect(() => { sessions.forEach((s) => { s.term.options.theme = theme === "night" ? NIGHT : DAY; }); }, [theme, sessions]);

  const connect = useCallback((host: Host) => {
    if (!holder.current) return;
    const id = `t${Date.now().toString(36)}`;
    const term = new Terminal({ theme: theme === "night" ? NIGHT : DAY, fontSize: 13, fontFamily: "Menlo, Monaco, 'Courier New', monospace", cursorBlink: true, scrollback: 5000 });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const el = document.createElement("div");
    el.className = "term";
    holder.current.appendChild(el);
    term.open(el);
    try { fit.fit(); } catch { /* measured after the first paint */ }
    term.onData((d) => api.termWrite(id, d).catch(() => {}));
    term.onResize(({ cols, rows }) => api.termResize(id, cols, rows).catch(() => {}));
    const s: Session = { id, host, term, fit, el, open: true };
    setSessions((l) => [...l, s]);
    setActive(id);
    api.termOpen(id, host, term.cols, term.rows).then((args) => { term.writeln(grey(`$ ssh ${args.join(" ")}`)); term.focus(); })
      .catch((e) => { term.writeln(`\x1b[31m${errText(e)}\x1b[0m`); setSessions((l) => l.map((x) => (x.id === id ? { ...x, open: false } : x))); });
  }, [theme]);

  const close = (id: string) => {
    const s = sessRef.current.find((x) => x.id === id);
    if (!s) return;
    api.termClose(id).catch(() => {});
    s.term.dispose();
    s.el.remove();
    setSessions((l) => l.filter((x) => x.id !== id));
    if (active === id) setActive(sessRef.current.find((x) => x.id !== id)?.id ?? null);
  };

  // "Connect (SSH)" on an instance block: the host for that block, saved version first (it carries the owner's key path)
  useEffect(() => {
    const on = (e: Event) => setPending(String((e as CustomEvent).detail ?? ""));
    window.addEventListener("aib-connect", on);
    return () => window.removeEventListener("aib-connect", on);
  }, []);
  useEffect(() => {
    if (!pending || !visible) return;
    const block = pending;
    setPending(null);
    api.hostsState(design).then((h) => {
      setHs(h);
      const sug = h.suggested.find((x) => x.block === block);
      if (!sug) { toast("That block is not an instance of this design."); return; }
      const saved = h.saved.find((x) => x.block === block || (sug.aws_id && x.aws_id === sug.aws_id));
      const host = saved ? { ...saved, host: saved.host || sug.host } : sug;
      setForm(host);
      if (!host.host) { toast(host.note || "The instance has no address yet."); return; }
      connect(host);
    }).catch((e) => toast(errText(e)));
  }, [pending, visible, design, connect, toast]);

  const save = () => {
    if (!hs) return;
    const h = { ...form, id: form.id || `h${Date.now().toString(36)}`, label: form.label.trim() || form.host.trim() || "host" };
    const list = hs.saved.some((x) => x.id === h.id) ? hs.saved.map((x) => (x.id === h.id ? h : x)) : [...hs.saved, h];
    api.hostsSave(list).then((saved) => { setHs({ ...hs, saved }); setForm(h); toast("Saved."); }).catch((e) => toast(errText(e)));
  };
  const forget = (id: string) => { if (!hs) return; api.hostsSave(hs.saved.filter((x) => x.id !== id)).then((saved) => setHs({ ...hs, saved })).catch((e) => toast(errText(e))); };
  const edit = (h: Host) => setForm(h.block && !hs?.saved.some((x) => x.id === h.id) ? { ...h, id: "" } : h);
  const field = (k: keyof Host, v: string | number) => setForm((f) => ({ ...f, [k]: v }));

  const hostRow = (h: Host, saved: boolean) => (
    <div key={h.id} className="host">
      <div className="row" style={{ margin: 0, justifyContent: "space-between" }}>
        <strong>{h.label}</strong>
        <span>
          <button className="small primary" disabled={!h.host} onClick={() => connect(h)}>Connect</button>{" "}
          <button className="small" onClick={() => edit(h)}>Edit</button>{" "}
          {saved && <button className="small danger" onClick={() => forget(h.id)}>Forget</button>}
        </span>
      </div>
      <div className="addr">{h.user}@{h.host || "?"}{h.port !== 22 ? `:${h.port}` : ""}{h.key ? ` · ${h.key}` : ""}</div>
      {h.note && <div className="small muted">{h.note}</div>}
    </div>);

  return (
    <div className="term-page" style={{ display: visible ? "flex" : "none" }}>
      <aside className="hosts">
        <h3 style={{ marginTop: 0 }}>Terminal</h3>
        <p className="small muted">SSH from here, with your own <code>ssh</code>{hs?.ssh ? ` (${hs.ssh})` : ""}: it reads the key, checks the host key and keeps nothing in the app. Traffic to an instance costs nothing.</p>
        {hs && hs.ssh === null && <div className="msg error">ssh was not found on this computer.</div>}
        <h4>From the design</h4>
        {hs && hs.suggested.length === 0 && <p className="small muted">No instance in this design yet. Add one on the canvas; after Apply or an import its address appears here.</p>}
        {hs?.suggested.map((h) => hostRow(h, false))}
        <h4>Saved hosts</h4>
        {hs && hs.saved.length === 0 && <p className="small muted">None yet. Fill the form and Save: any machine, not only AWS.</p>}
        {hs?.saved.map((h) => hostRow(h, true))}
        <h4>{form.id ? "Edit host" : "New host"}</h4>
        <label>Label<input value={form.label} onChange={(e) => field("label", e.target.value)} placeholder="lab box" /></label>
        <label>Address or DNS name<input value={form.host} onChange={(e) => field("host", e.target.value)} placeholder="3.7.1.2 or ec2-….compute.amazonaws.com" /></label>
        <div className="row">
          <label style={{ flex: 1 }}>User<input value={form.user} onChange={(e) => field("user", e.target.value)} /></label>
          <label style={{ width: 90 }}>Port<input type="number" min={1} max={65535} value={form.port} onChange={(e) => field("port", Math.max(1, Math.min(65535, Number(e.target.value) || 22)))} /></label>
        </div>
        <label>Key file (.pem; empty = ssh's agent and ~/.ssh/config)
          <div className="row" style={{ margin: 0 }}>
            <input style={{ flex: 1 }} value={form.key} onChange={(e) => field("key", e.target.value)} placeholder="~/.ssh/my_key.pem" />
            <button className="small" onClick={() => api.pickFile().then((p) => p && field("key", p)).catch((e) => toast(errText(e)))}>Choose…</button>
          </div>
        </label>
        {keyProblem && <div className="msg warn small">{keyProblem}{keyProblem.includes("mode 600") && <> <button className="small" onClick={() => api.keyFix(form.key).then(setKeyProblem).catch((e) => toast(errText(e)))}>Fix permissions</button></>}</div>}
        <div className="row">
          <button className="primary" disabled={!form.host.trim()} onClick={() => connect(form)}>Connect</button>
          <button onClick={save}>Save</button>
          <button className="small" onClick={() => setForm(blank())}>Clear</button>
        </div>
        <p className="small muted">Amazon Linux logs in as <code>ec2-user</code>, Ubuntu as <code>ubuntu</code>, Debian as <code>admin</code>. The instance's group must let port 22 in from your address (the Fix on the canvas does that).</p>
      </aside>
      <section className="term-main">
        <div className="term-tabs">
          {sessions.map((s) => (
            <div key={s.id} className={`ttab ${s.id === active ? "on" : ""}`} onClick={() => { setActive(s.id); window.requestAnimationFrame(() => s.term.focus()); }}>
              <span className={`dot ${s.open ? "" : "closed"}`} />{s.host.label || s.host.host}
              <button className="x" title="Close" onClick={(e) => { e.stopPropagation(); close(s.id); }}>×</button>
            </div>))}
          {sessions.length === 0 && <span className="small muted" style={{ padding: "4px 2px" }}>No session open. Connect to a host on the left, or press "Connect (SSH)" on an instance block.</span>}
        </div>
        <div className="term-holder" ref={holder}>
          {sessions.map((s) => { s.el.style.display = s.id === active ? "block" : "none"; return null; })}
        </div>
      </section>
    </div>
  );
}
