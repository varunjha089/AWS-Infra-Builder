// Code: the design as OpenTofu / Terraform HCL (main.tf, variables.tf, outputs.tf), generated again on every change; Copy and Export.
import { useEffect, useState } from "react";
import { api, errText, type Analysis, type Design } from "./api";

export function CodeTab({ design, analysis, toast }: { design: Design; analysis: Analysis; toast: (m: string) => void }) {
  const [files, setFiles] = useState<Record<string, string>>({});
  const [name, setName] = useState("main.tf");
  useEffect(() => { api.code(design).then(setFiles).catch((e) => toast(errText(e))); }, [design, toast]);
  const names = Object.keys(files);
  const text = files[name] ?? "";
  return (
    <div className="page">
      <div className="row">
        <h2 style={{ margin: 0 }}>Code</h2>
        <span className="muted">OpenTofu / Terraform, AWS provider 5 · generated from the design; take it and run it, or Apply from the app in a later version</span>
        <span className="grow" />
        <button onClick={() => navigator.clipboard.writeText(text).then(() => toast(`${name} copied.`))}>Copy {name}</button>
        <button className="primary" onClick={() => api.exportCode().then((p) => toast(`Written to ${p}`)).catch((e) => toast(errText(e)))}>Export…</button>
      </div>
      {!analysis.ok && <div className="msg error">The design has {analysis.errors} error{analysis.errors === 1 ? "" : "s"}; the code carries them as a header. Fix them in the Design tab before applying.</div>}
      <div className="row" style={{ gap: 2 }}>{names.map((n) => <button key={n} className={`tab ${n === name ? "on" : ""}`} onClick={() => setName(n)}>{n}</button>)}</div>
      <pre className="code" style={{ maxHeight: "calc(100vh - 230px)" }}>{text}</pre>
      <p className="muted small">Export writes the three files into a folder of your choice; then <code>tofu init && tofu plan</code> there shows what would be created. Every resource is tagged <code>managed-by = aws-infra-builder</code>, <code>project</code> and <code>design-block</code>, so the account shows what came from where.</p>
    </div>
  );
}
