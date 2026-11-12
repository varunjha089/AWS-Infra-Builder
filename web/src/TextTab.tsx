// The Text tab: the design as .aib text. Typing changes the canvas once the text is valid (Apply, ⌘↩); problems name their line.
import { useEffect, useRef, useState } from "react";
import { api, type Design, type TextError } from "./api";

export function TextTab({ design, onApply, onFormat }: { design: Design; onApply: (text: string) => Promise<TextError[]>; onFormat: () => void }) {
  const [text, setText] = useState(design.text ?? "");
  const [errors, setErrors] = useState<TextError[]>([]);
  const [dirty, setDirty] = useState(false);
  const [help, setHelp] = useState<[string, string][]>([]);
  const [showHelp, setShowHelp] = useState(false);
  const gutter = useRef<HTMLDivElement>(null);
  // the design changed elsewhere (canvas, undo): show its text, unless the owner is in the middle of typing
  useEffect(() => { if (!dirty) { setText(design.text ?? ""); setErrors([]); } }, [design.text, dirty]);
  useEffect(() => { api.textHelp().then(setHelp).catch(() => {}); }, []);

  const apply = async () => {
    const errs = await onApply(text);
    setErrors(errs);
    if (errs.length === 0) setDirty(false);
  };
  const lines = text.split("\n").length;
  const errLines = new Set(errors.map((e) => e.line));
  return (
    <div className="page text-tab">
      <div className="row">
        <h2 style={{ margin: 0 }}>Text</h2>
        <span className="muted">one line per block; the canvas and this text are two views of one design</span>
        <span className="grow" />
        <button className="primary" onClick={apply} disabled={!dirty} title="⌘↩">Apply to canvas</button>
        <button onClick={() => { setDirty(false); setText(design.text ?? ""); setErrors([]); }} disabled={!dirty}>Discard typing</button>
        <button onClick={onFormat} disabled={dirty} title="print the text again from the blocks">Format</button>
        <button onClick={() => setShowHelp((s) => !s)}>{showHelp ? "Hide help" : "Help"}</button>
      </div>
      {showHelp && <table className="text-help"><tbody>{help.map(([k, v]) => <tr key={k}><td><code>{k}</code></td><td className="muted">{v}</td></tr>)}</tbody></table>}
      <div className="editor">
        <div className="gutter" ref={gutter}>{Array.from({ length: lines }, (_, i) => <div key={i} className={errLines.has(i + 1) ? "err" : ""}>{i + 1}</div>)}</div>
        <textarea value={text} spellCheck={false}
          onChange={(e) => { setText(e.target.value); setDirty(true); }}
          onScroll={(e) => { if (gutter.current) gutter.current.scrollTop = e.currentTarget.scrollTop; }}
          onKeyDown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === "Enter") { e.preventDefault(); apply(); } }} />
      </div>
      {errors.length > 0 && <div className="msg error">{errors.map((e, i) => <div key={i}>line {e.line}: {e.message}</div>)}</div>}
      {errors.length === 0 && dirty && <div className="msg info">Press Apply (⌘↩) to put the text on the canvas.</div>}
    </div>
  );
}
