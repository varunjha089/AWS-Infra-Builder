// A number field that takes what is typed. It never clamps while typing (clamping turned "select all, type 200" into 800:
// the "2" was pushed up to the minimum 8, then "00" followed). The value is stored whenever the text is a number; what is
// out of range is the checks' business, said as a message on the block, not forced in the field.
import { useEffect, useRef, useState, type InputHTMLAttributes } from "react";

export function Num({ value, onCommit, ...rest }: { value: number; onCommit: (v: number) => void } & Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "type">) {
  const [text, setText] = useState(String(value));
  const last = useRef(value);
  useEffect(() => {
    // a change from elsewhere (undo, a Fix, another block selected) shows up; our own commits do not reset the text
    if (value !== last.current) { last.current = value; setText(String(value)); }
  }, [value]);
  return <input type="number" {...rest} value={text} onChange={(e) => {
    const t = e.target.value;
    setText(t);
    const v = Number(t);
    if (t.trim() !== "" && Number.isFinite(v)) { last.current = v; onCommit(v); }
  }} />;
}
