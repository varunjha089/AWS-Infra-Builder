// The canvas: VPCs and subnets are boxes that hold other blocks (React Flow nested nodes); route tables, gateways are small blocks;
// arrows are attachments (subnet → route table) and routes (route table → gateway). A palette on the left, the inspector on the right.
import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  ReactFlow, ReactFlowProvider, Background, Controls, MiniMap, Handle, Position, NodeResizer, applyEdgeChanges, applyNodeChanges, useReactFlow,
  MarkerType, type Connection, type Edge, type EdgeChange, type Node, type NodeChange, type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { api, errText, type Sizes, type Analysis, type AppState, type DNode, type Design, type EdgeResult, type JobSnap, type Loaded, type NodeResult } from "./api";
import { NewInstance, HOUR_PRESETS, hoursText, type Pick } from "./NewInstance";
import { Num } from "./Num";
import { cidrText, nextFree, parseCidr, size } from "./cidr";
import { neon, useTheme } from "./theme";

export const COLORS: Record<string, string> = { vpc: "#e07b00", subnet: "#2f6fde", public: "#2e7d32", private: "#2f6fde", isolated: "#6b7280", igw: "#7c3aed", nat: "#0e7490", rt: "#8e5bb5", sg: "#c0392b", ec2: "#b7791f", cidr: "#475569", vpce: "#0f766e", s3: "#15803d", alb: "#9333ea", asg: "#a16207", lambda: "#be185d", other: "#6b7280" };
const CONTAINER = (kind: string) => kind === "vpc" || kind === "subnet";

type Data = { node: DNode; result?: NodeResult; azName: string; plan?: string; mark?: string };

function baseColor(n: DNode): string {
  return n.kind === "subnet" ? COLORS[String(n.params.tier)] ?? COLORS.subnet : COLORS[n.kind] ?? "#999";
}
/** a block's colour: as defined by day, its neon version at night */
function useColor(): (n: DNode) => string {
  const night = useTheme() === "night";
  return (n: DNode) => (night ? neon(baseColor(n)) : baseColor(n));
}
const mark = (st?: string) => (st === "error" ? " ✕" : st === "warn" ? " ⚠" : "");

function BoxNode({ data, selected }: NodeProps<Node<Data>>) {
  const { node: n, result: r } = data;
  const color = useColor();
  const st = r?.status ?? "ok";
  const isVpc = n.kind === "vpc";
  return (
    <div className={`box ${n.kind} st-${st} ${selected ? "sel" : ""}`} style={{ "--c": color(n) } as CSSProperties}>
      <NodeResizer minWidth={isVpc ? 300 : 160} minHeight={isVpc ? 200 : 90} isVisible={!!selected} />
      {(data.plan || data.mark) && <span className={`ribbon act-${data.mark ?? data.plan}`}>{data.mark ?? data.plan}</span>}
      <div className="bh">
        <span>{isVpc ? "VPC" : String(n.params.tier ?? "subnet")}{mark(st)}</span><span className="bl">{n.label}</span>{n.params.aws_id ? <span className="awsid" title="created in AWS">{String(n.params.aws_id)}</span> : null}
        <span className="bs" title={r?.summary}>{isVpc ? String(n.params.cidr ?? "") : `${n.params.cidr || "no CIDR"} · ${data.azName}`}</span>
      </div>
      {!isVpc && r && <div className={`bm st-${st}`}>{r.status !== "ok" ? r.messages[0] : r.info[0]?.replace(/^.*= /, "")}</div>}
      {isVpc && r && r.status !== "ok" && <div className={`bm st-${st}`}>{r.messages[0]}</div>}
      {!isVpc && <Handle type="source" position={Position.Right} style={{ top: 18 }} title="drag to a route table" />}
    </div>
  );
}

function BlockNode({ data, selected }: NodeProps<Node<Data>>) {
  const { node: n, result: r } = data;
  const color = useColor();
  const st = r?.status ?? "ok";
  return (
    <div className={`block ${n.kind} st-${st} ${selected ? "sel" : ""}`} style={{ "--c": color(n) } as CSSProperties}>
      {(n.kind === "rt" || n.kind === "sg" || n.kind === "cidr" || n.kind === "asg" || n.kind === "ec2") && <Handle type="target" position={Position.Left} />}
      {(n.kind === "igw" || n.kind === "nat" || n.kind === "vpce" || n.kind === "other") && <Handle type="target" position={Position.Top} />}
      {(data.plan || data.mark) && <span className={`ribbon act-${data.mark ?? data.plan}`}>{data.mark ?? data.plan}</span>}
      <div className="lh"><span>{r?.title ?? n.kind}{mark(st)}</span><span className="bl">{n.label}</span></div>
      <div className={`ls st-${st}`}>{st !== "ok" ? r?.messages[0] : r?.summary || ""}{n.params.aws_id ? <span className="awsid"> · {String(n.params.aws_id)}</span> : null}</div>
      {n.kind === "rt" && <Handle type="source" position={Position.Right} title="drag to a gateway" />}
      {(n.kind === "sg" || n.kind === "cidr") && <Handle type="source" position={Position.Right} title="drag to a security group or an address range: traffic" />}
      {n.kind === "alb" && <Handle type="source" position={Position.Right} title="drag to an Auto Scaling group or an instance: the target" />}
    </div>
  );
}
const nodeTypes = { box: memo(BoxNode), block: memo(BlockNode) };

/** the design's blocks as React Flow nodes (containers first, positions relative to the container) */
function toNodes(d: Design, a: Analysis, azNames: string[], job: JobSnap | null): Node<Data>[] {
  const res = Object.fromEntries(a.nodes.map((r) => [r.id, r]));
  const planned = job && job.plan && (job.kind === "plan" || job.kind === "plan-destroy") && job.state !== "running" ? job.plan.by_block : {};
  const marks = job && (job.kind === "apply" || job.kind === "destroy") ? job.marks : {};
  return d.nodes.map((n) => {
    const azIx = "abcdef".indexOf(String(n.params.az ?? ""));
    const azName = azNames[azIx] ?? `${d.region}${n.params.az ?? ""}`;
    const container = CONTAINER(n.kind);
    return {
      id: n.id, type: container ? "box" : "block", position: { x: n.x, y: n.y }, data: { node: n, result: res[n.id], azName, plan: planned[n.id], mark: marks[n.id] },
      className: `${planned[n.id] ? `plan-${planned[n.id]}` : ""} ${marks[n.id] ? `mark-${marks[n.id]}` : ""}`,
      parentId: n.parent ?? undefined, style: container ? { width: n.w, height: n.h } : undefined, zIndex: n.kind === "vpc" ? 0 : n.kind === "subnet" ? 1 : 2,
    };
  });
}
function toEdges(d: Design, a: Analysis): Edge[] {
  const res: Record<string, EdgeResult> = Object.fromEntries(a.edges.map((r) => [r.id, r]));
  return d.edges.map((e) => ({
    id: e.id, source: e.source, target: e.target, label: res[e.id]?.label ?? "", className: `st-${res[e.id]?.status ?? "ok"}`,
    markerEnd: { type: MarkerType.ArrowClosed, width: 18, height: 18 }, type: "smoothstep", zIndex: 5,
  }));
}

/** the absolute position of a block (its own plus every enclosing block's) */
function absPos(d: Design, id: string): { x: number; y: number } {
  const at = Object.fromEntries(d.nodes.map((n) => [n.id, n]));
  let x = 0, y = 0, cur: string | null | undefined = id, guard = 0;
  while (cur && at[cur] && guard++ < 50) { x += at[cur].x; y += at[cur].y; cur = at[cur].parent; }
  return { x, y };
}

/** the container a dropped block belongs to: the innermost box of the right kind whose area holds the block's top-left corner */
function dropTarget(d: Design, n: DNode, want: string, abs: { x: number; y: number }): string | null {
  if (!want) return null;
  const inside = (c: DNode) => { const p = absPos(d, c.id); return abs.x >= p.x && abs.y >= p.y && abs.x <= p.x + c.w && abs.y <= p.y + c.h; };
  const below = new Set(descendants(d, n.id));
  const hits = d.nodes.filter((c) => c.kind === want && !below.has(c.id) && inside(c));
  return hits.length ? hits[hits.length - 1].id : null;
}
function descendants(d: Design, id: string): string[] {
  const out = [id];
  for (let i = 0; i < out.length; i++) d.nodes.filter((n) => n.parent === out[i]).forEach((n) => out.push(n.id));
  return out;
}

interface Props {
  design: Design; analysis: Analysis; st: AppState; change: (d: Design, later?: boolean) => void; apply: (l: Loaded) => void; toast: (m: string) => void;
  selectId: string | null; setSelectId: (id: string | null) => void; signal: { fit: number }; job: JobSnap | null;
}

export function DesignTab(p: Props) {
  return <ReactFlowProvider><Canvas {...p} /></ReactFlowProvider>;
}

function Canvas({ design, analysis, st, change, apply, toast, selectId, setSelectId, signal, job }: Props) {
  const rf = useReactFlow();
  const color = useColor();
  const night = useTheme() === "night";
  const pal = (kind: string) => ({ borderColor: night ? neon(COLORS[kind]) : COLORS[kind], "--c": night ? neon(COLORS[kind]) : COLORS[kind] } as CSSProperties);
  const [nodes, setNodes] = useState<Node<Data>[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  const [selEdge, setSelEdge] = useState<string | null>(null);
  const dragging = useRef(false);
  const parentKind = useMemo(() => Object.fromEntries(st.kinds.map((k) => [k.kind, k.parent])), [st.kinds]);

  // the design is the truth; the canvas is redrawn from it (except while a block is being dragged)
  useEffect(() => {
    if (dragging.current) return;
    setNodes((old) => {
      const sel = new Set(old.filter((n) => n.selected).map((n) => n.id));
      if (selectId) sel.add(selectId);
      return toNodes(design, analysis, analysis.az_names, job).map((n) => ({ ...n, selected: sel.has(n.id) }));
    });
    setEdges((old) => { const sel = new Set(old.filter((e) => e.selected).map((e) => e.id)); return toEdges(design, analysis).map((e) => ({ ...e, selected: sel.has(e.id) })); });
  }, [design, analysis, selectId, job]);
  useEffect(() => { if (signal.fit) window.setTimeout(() => rf.fitView({ padding: 0.15, duration: 300 }), 60); }, [signal, rf]);

  const onNodesChange = useCallback((changes: NodeChange<Node<Data>>[]) => {
    setNodes((ns) => applyNodeChanges(changes, ns));
    // a resized box: its new size into the design (written after a pause)
    const dim = changes.filter((c) => c.type === "dimensions" && c.resizing === false && c.dimensions);
    if (dim.length) {
      const d: Design = { ...design, nodes: design.nodes.map((n) => { const c = dim.find((c) => c.type === "dimensions" && c.id === n.id); return c && c.type === "dimensions" && c.dimensions && CONTAINER(n.kind) ? { ...n, w: c.dimensions.width, h: c.dimensions.height } : n; }) };
      change(d, true);
    }
    const sel = changes.find((c) => c.type === "select");
    if (sel) { const s = applyNodeChanges(changes, nodes).find((n) => n.selected); setSelectId(s?.id ?? null); if (s) setSelEdge(null); }
  }, [design, change, nodes, setSelectId]);
  const onEdgesChange = useCallback((changes: EdgeChange[]) => {
    setEdges((es) => applyEdgeChanges(changes, es));
    const sel = changes.find((c) => c.type === "select");
    if (sel) { const s = applyEdgeChanges(changes, edges).find((e) => e.selected); setSelEdge(s?.id ?? null); if (s) setSelectId(null); }
  }, [edges, setSelectId]);

  /** dropped: the block's place (and, when it was dragged into another box, its new container) go into the design */
  const onNodeDragStop = useCallback((_: unknown, _node: Node, dragged: Node[]) => {
    dragging.current = false;
    let d: Design = { ...design, nodes: design.nodes.map((n) => ({ ...n })) };
    for (const moved of dragged) {
      const n = d.nodes.find((x) => x.id === moved.id);
      if (!n) continue;
      n.x = moved.position.x; n.y = moved.position.y;
      const abs = absPos(d, n.id);
      const want = parentKind[n.kind] ?? "";
      const target = dropTarget(d, n, want, { x: abs.x + 20, y: abs.y + 16 });
      if (target !== (n.parent ?? null) && (target || want === "")) {
        const tp = target ? absPos(d, target) : { x: 0, y: 0 };
        n.parent = target; n.x = abs.x - tp.x; n.y = abs.y - tp.y;
      } else if (!target && want !== "" && n.parent) {
        // dragged out of every box of the right kind: it stays in the region, and the check says so
        n.parent = null; n.x = abs.x; n.y = abs.y;
      }
    }
    change(d);
  }, [design, change, parentKind]);

  const onConnect = useCallback((c: Connection) => {
    if (!c.source || !c.target) return;
    api.designConnect(design, c.source, c.target).then((l) => { apply(l); }).catch((e) => toast(errText(e)));
  }, [design, apply, toast]);
  const onDelete = useCallback(({ nodes: ns, edges: es }: { nodes: Node[]; edges: Edge[] }) => {
    if (!ns.length && !es.length) return;
    api.designRemove(design, ns.map((n) => n.id), es.map((e) => e.id)).then((l) => { apply(l); setSelectId(null); setSelEdge(null); }).catch((e) => toast(errText(e)));
  }, [design, apply, toast, setSelectId]);

  /** a block from the palette (or the Design menu) into the right container: the selected one, or the first that fits */
  const add = useCallback((kind: string) => {
    const want = parentKind[kind] ?? "";
    let parent: string | null = null;
    if (want) {
      const sel = selectId ? design.nodes.find((n) => n.id === selectId) : undefined;
      let cur = sel;
      while (cur && cur.kind !== want) cur = design.nodes.find((n) => n.id === cur!.parent);
      parent = cur?.id ?? (kind === "nat" ? design.nodes.find((n) => n.kind === "subnet" && n.params.tier === "public")?.id : undefined) ?? design.nodes.find((n) => n.kind === want)?.id ?? null;
      if (!parent) { toast(want === "vpc" ? "Add a VPC first: every other block lives in one." : kind === "ec2" ? "An instance lives in a subnet: add a subnet first." : "A NAT Gateway lives in a (public) subnet: add a subnet first."); return; }
    }
    if (kind === "ec2" && parent) { setNewInst({ parent }); return; }
    api.designAdd(design, kind, parent).then((l) => { apply(l); if (l.id) setSelectId(l.id); }).catch((e) => toast(errText(e)));
  }, [design, parentKind, selectId, apply, toast, setSelectId]);
  /** the New-instance dialog: for a new block in a subnet, or "change" for the selected instance */
  const [newInst, setNewInst] = useState<{ parent: string } | "change" | null>(null);
  /** key pair names to suggest: the .pem files in ~/.ssh and the names already on blocks */
  const [keyFiles, setKeyFiles] = useState<string[]>([]);
  useEffect(() => { if (newInst) api.keysList().then((k) => setKeyFiles(k.keys.map((f) => f.name.replace(/\.(pem|key)$/, "")))).catch(() => {}); }, [newInst]);
  const keyNames = Array.from(new Set([...keyFiles, ...design.nodes.filter((n) => n.kind === "ec2" || n.kind === "asg").map((n) => String(n.params.key ?? "")).filter(Boolean)]));
  const [armed, setArmed] = useState<string | null>(null);
  const [powerBusy, setPowerBusy] = useState(false);
  const hourlyOf = (t: string) => { const it = st.instance_types.find((x) => x.type === t); return it ? (design.region === "us-east-1" ? it.us_east_1 : design.region === "ap-south-1" ? it.ap_south_1 : it.us_east_1 * 1.1) : 0; };
  /** Refresh (null), Start (true) or Stop (false) the real instance behind a block; the state and addresses come back onto the block */
  const power = (n: DNode, on: boolean | null) => {
    const id = String(n.params.aws_id ?? "");
    if (!id) return;
    setPowerBusy(true); setArmed(null);
    (on === null ? api.awsInstanceState([id], design.region) : api.awsInstancePower(id, on, design.region)).then((states) => {
      const s = states.find((x) => x.id === id);
      if (!s) { toast(`${id} was not found in ${design.region}.`); return; }
      change({ ...design, nodes: design.nodes.map((x) => (x.id === n.id ? { ...x, params: { ...x.params, state: s.state, address: s.address ?? "", private_ip: s.private_ip ?? "" } } : x)) }, false);
      toast(on === null ? `${n.label}: ${s.state}` : on ? `${n.label} is starting (${s.state}). It costs USD ${hourlyOf(String(n.params.type ?? "")).toFixed(4)} an hour until stopped; the address may be new.` : `${n.label} is stopping (${s.state}). Only its volume${n.params.public_ip ? " and address" : ""} are charged now.`);
    }).catch((e) => toast(errText(e))).finally(() => setPowerBusy(false));
  };
  /** two Lambda functions that stop the instance in the evening and start it in the morning, weekdays */
  const schedule = (n: DNode) => {
    api.designAdd(design, "lambda", null, { action: "stop", instances: [n.id], time: "19:00" }).then((l1) =>
      api.designAdd(l1.design, "lambda", null, { action: "start", instances: [n.id], time: "09:00" }).then((l2) => {
        const stopId = l1.id, startId = l2.id;
        const d = { ...l2.design, nodes: l2.design.nodes.map((x) => (x.id === stopId ? { ...x, label: `stop_${n.label}` } : x.id === startId ? { ...x, label: `start_${n.label}` } : x)) };
        change(d, false);
        if (startId) setSelectId(startId);
        toast(`stop_${n.label} (19:00) and start_${n.label} (09:00), weekdays, Asia/Kolkata. Change the times, days or zone on each.`);
      })).catch((e) => toast(errText(e)));
  };
  const pickInstance = (c: Pick) => {
    const minDisk = st.os.find((o) => o.key === c.os)?.min_disk ?? 8;
    if (newInst === "change" && selectId) {
      change({ ...design, nodes: design.nodes.map((n) => (n.id === selectId ? { ...n, params: { ...n.params, ...c, disk: Math.max(Number(n.params.disk ?? 8), minDisk) } } : n)) }, false);
    } else if (newInst && newInst !== "change") {
      api.designAdd(design, "ec2", newInst.parent, { ...c, disk: minDisk }).then((l) => { apply(l); if (l.id) setSelectId(l.id); }).catch((e) => toast(errText(e)));
    }
    setNewInst(null);
  };
  useEffect(() => {
    const h = (e: Event) => add((e as CustomEvent<string>).detail);
    window.addEventListener("aib-add", h);
    return () => window.removeEventListener("aib-add", h);
  }, [add]);

  /** the blocks' sizes as drawn, so Arrange and Tidy lay out with the real text heights (a block it has not drawn gets the estimate) */
  const measured = useCallback((): Sizes => Object.fromEntries(rf.getNodes().filter((n) => n.type === "block" && n.measured?.width && n.measured?.height).map((n) => [n.id, [n.measured!.width!, n.measured!.height!] as [number, number]])), [rf]);
  const arrange = useCallback(() => {
    api.designArrange(design, measured()).then(apply).then(() => window.setTimeout(() => rf.fitView({ padding: 0.15, duration: 300 }), 60)).catch((e) => toast(errText(e)));
  }, [design, measured, apply, rf, toast]);
  useEffect(() => {
    window.addEventListener("aib-arrange", arrange);
    return () => window.removeEventListener("aib-arrange", arrange);
  }, [arrange]);
  const fixIt = useCallback((id: string | null, allSafe: boolean) => {
    api.designFix(design, id, allSafe, measured()).then((l) => { apply(l); toast(allSafe ? `${l.done} fixed. ⌘Z undoes it.` : "Fixed. ⌘Z undoes it."); }).catch((e) => toast(errText(e)));
  }, [design, measured, apply, toast]);
  /** the fixes for one block or arrow, as buttons */
  const fixesFor = (target: string) => {
    const fs = analysis.fixes.filter((f) => f.target === target);
    return fs.length > 0 ? <div className="fixes">{fs.map((f) => <div key={f.id} className="fix"><button className="small primary" onClick={() => fixIt(f.id, false)}>{f.title}</button>{f.note && <span className="small muted"> {f.note}</span>}</div>)}</div> : null;
  };
  const tidy = useCallback((id: string) => {
    api.designTidy(design, id, measured()).then((l) => { apply(l); setSelectId(null); toast("Done. The design was laid out again; ⌘Z undoes it."); }).catch((e) => toast(errText(e)));
  }, [design, measured, apply, toast, setSelectId]);

  const selected = selectId ? design.nodes.find((n) => n.id === selectId) : undefined;
  const result = selected ? analysis.nodes.find((r) => r.id === selected.id) : undefined;
  const edge = selEdge ? design.edges.find((e) => e.id === selEdge) : undefined;
  const edgeResult = edge ? analysis.edges.find((r) => r.id === edge.id) : undefined;

  const setParam = (id: string, key: string, value: unknown, later = true) => change({ ...design, nodes: design.nodes.map((n) => (n.id === id ? { ...n, params: { ...n.params, [key]: value } } : n)) }, later);
  const setLabel = (id: string, label: string) => change({ ...design, nodes: design.nodes.map((n) => (n.id === id ? { ...n, label } : n)) }, true);
  const setEdgeDest = (id: string, dest: string) => change({ ...design, edges: design.edges.map((e) => (e.id === id ? { ...e, params: { ...e.params, dest } } : e)) }, true);
  const propose = (s: DNode) => {
    const vpc = design.nodes.find((n) => n.id === s.parent);
    const vc = vpc && parseCidr(String(vpc.params.cidr ?? ""));
    if (!vc) { toast("Put the subnet in a VPC with a valid CIDR first."); return; }
    const taken = design.nodes.filter((n) => n.parent === vpc!.id && n.kind === "subnet" && n.id !== s.id).map((n) => parseCidr(String(n.params.cidr ?? ""))).filter((c): c is NonNullable<typeof c> => !!c);
    const own = parseCidr(String(s.params.cidr ?? ""));
    const prefix = own ? own.prefix : Math.min(28, vc.prefix + 4);
    const c = nextFree(vc, taken, prefix);
    if (!c) { toast(`No free /${prefix} left in ${cidrText(vc)}. Use a smaller block (a larger prefix) or a bigger VPC.`); return; }
    setParam(s.id, "cidr", cidrText(c), false);
  };
  const letters = "abcdef".slice(0, design.azs).split("");
  const problemsFor = (r?: NodeResult | EdgeResult) => r && r.messages.length > 0 && <div>{r.messages.map((m, i) => <div key={i} className={`msg ${r.status === "error" ? "error" : "warn"}`}>{m}</div>)}</div>;

  return (
    <div className="design">
      {newInst && <NewInstance st={st} region={design.region} toast={toast} keyNames={keyNames} title={newInst === "change" ? "Recommend an instance type" : "New instance: what must it have?"}
        initial={newInst === "change" && selected ? { os: String(selected.params.os ?? "al2023"), hours: Number(selected.params.hours ?? 730), key: String(selected.params.key ?? ""), gpu: /^[gp]/.test(String(selected.params.type ?? "")), vcpu: st.instance_types.find((t) => t.type === selected.params.type)?.vcpu ?? 2, ram: st.instance_types.find((t) => t.type === selected.params.type)?.memory ?? 1 } : undefined}
        onPick={pickInstance} onClose={() => setNewInst(null)} />}
      <aside className="palette">
        <h4>Scope</h4>
        <button className="pal" style={pal("vpc")} onClick={() => add("vpc")}>VPC<small>a private network in the region</small></button>
        <h4>Network</h4>
        <button className="pal" style={pal("subnet")} onClick={() => add("subnet")}>Subnet<small>public, private or isolated, in one AZ</small></button>
        <button className="pal" style={pal("igw")} onClick={() => add("igw")}>Internet Gateway<small>one per VPC</small></button>
        <button className="pal" style={pal("nat")} onClick={() => add("nat")}>NAT Gateway<small>in a public subnet, ≈ USD 32 / month</small></button>
        <button className="pal" style={pal("rt")} onClick={() => add("rt")}>Route table<small>subnet → table → gateway</small></button>
        <button className="pal" style={pal("vpce")} onClick={() => add("vpce")}>VPC endpoint<small>S3/DynamoDB free; reach AWS without a NAT</small></button>
        <h4>Security</h4>
        <button className="pal" style={pal("sg")} onClick={() => add("sg")}>Security group<small>arrows between groups = allowed traffic</small></button>
        <button className="pal" style={pal("cidr")} onClick={() => add("cidr")}>Address range<small>the internet, your office IP</small></button>
        <h4>Data</h4>
        <button className="pal" style={pal("s3")} onClick={() => add("s3")}>S3 bucket<small>public access blocked, encrypted</small></button>
        <button className="pal" style={pal("lambda")} onClick={() => add("lambda")}>Lambda function<small>stops or starts instances on a schedule; or your own code. Free tier</small></button>
        <h4>Compute</h4>
        <button className="pal" style={pal("ec2")} onClick={() => add("ec2")}>EC2 instance<small>cheapest type by default, no public IP</small></button>
        <button className="pal" style={pal("asg")} onClick={() => add("asg")}>Auto Scaling group<small>instances in a tier; Spot is safe here</small></button>
        <button className="pal" style={pal("alb")} onClick={() => add("alb")}>Load balancer<small>two AZs; ≈ USD 17/month idle</small></button>
        <h4>Arrows</h4>
        <p className="small muted" style={{ margin: "4px 0" }}>Subnet → route table: the subnet uses it. Route table → gateway: a route. Group or range → group: the target lets that traffic in (ports in the inspector).</p>
        <h4>Layout</h4>
        <button className="pal" style={pal("rt")} onClick={arrange}>Arrange<small>innermost first: every block its own room (⇧⌘A)</small></button>
        <button className="pal" style={pal("rt")} onClick={() => rf.fitView({ padding: 0.15, duration: 300 })}>Fit to window<small>⇧⌘F</small></button>
      </aside>
      <div className="canvas">
        {design.nodes.length === 0 && <div className="msg info big-hint">Empty design: add a VPC from the palette, then subnets, an Internet Gateway and a route table. Or open a template from the Projects page.</div>}
        {job && job.plan && (job.kind === "plan" || job.kind === "plan-destroy") && job.state === "done" && !job.plan.empty && <div className="legend">
          <strong>Plan:</strong> {job.plan.summary} · <span className="pill act-create">create</span> <span className="pill act-update">change</span> <span className="pill act-replace">replace</span> <span className="pill act-destroy">destroy</span></div>}
        {job && (job.kind === "apply" || job.kind === "destroy") && <div className="legend"><strong>{job.kind === "apply" ? "Applying" : "Destroying"}{job.state === "running" ? "…" : `: ${job.state}`}</strong> · blocks turn solid as AWS finishes them</div>}
        <ReactFlow nodes={nodes} edges={edges} nodeTypes={nodeTypes} onNodesChange={onNodesChange} onEdgesChange={onEdgesChange}
          onNodeDragStart={() => { dragging.current = true; }} onNodeDragStop={onNodeDragStop} onConnect={onConnect} onDelete={onDelete}
          deleteKeyCode={["Backspace", "Delete"]} fitView minZoom={0.15} maxZoom={2} selectNodesOnDrag={false} elevateNodesOnSelect={false}
          onPaneClick={() => { setSelectId(null); setSelEdge(null); }}>
          <Background gap={18} />
          <Controls />
          <MiniMap pannable zoomable nodeColor={(n) => color((n.data as Data).node)} maskColor={night ? "rgba(5,6,12,.7)" : undefined} />
        </ReactFlow>
      </div>
      <aside className="inspector">
        {selected && result ? (
          <>
            <h3 style={{ color: color(selected), "--c": color(selected) } as CSSProperties}>{result.title}{mark(result.status)}</h3>
            <div className="fields">
              <label>Name (in the text)<input value={selected.label} onChange={(e) => setLabel(selected.id, e.target.value)} /></label>
              {selected.kind === "vpc" && <label>CIDR<input value={String(selected.params.cidr ?? "")} onChange={(e) => setParam(selected.id, "cidr", e.target.value)} placeholder="10.0.0.0/16" /></label>}
              {selected.kind === "subnet" && <>
                <label>Tier<select value={String(selected.params.tier ?? "public")} onChange={(e) => setParam(selected.id, "tier", e.target.value, false)}>
                  {st.tiers.map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
                <label>CIDR<span className="row" style={{ margin: 0 }}><input value={String(selected.params.cidr ?? "")} onChange={(e) => setParam(selected.id, "cidr", e.target.value)} placeholder="10.0.1.0/24" style={{ width: 150 }} />
                  <button className="small" onClick={() => propose(selected)} title="the next free block of this size in the VPC">Propose free</button></span></label>
                <label>Availability Zone<select value={String(selected.params.az ?? "a")} onChange={(e) => setParam(selected.id, "az", e.target.value, false)}>
                  {letters.map((l, i) => <option key={l} value={l}>{analysis.az_names[i] ?? `${design.region}${l}`}</option>)}</select></label>
                <div className="small muted">public: a route to the Internet Gateway · private: out through a NAT Gateway · isolated: no way out</div>
              </>}
              {selected.kind === "rt" && <div className="small muted">Routes are the arrows leaving this table. Select an arrow to change its destination; the VPC's local route is always there.</div>}
              {selected.kind === "cidr" && <label>Range<input value={String(selected.params.cidr ?? "")} onChange={(e) => setParam(selected.id, "cidr", e.target.value)} placeholder="0.0.0.0/0 = anywhere" /></label>}
              {selected.kind === "vpce" && <>
                <label>Service<select value={String(selected.params.service ?? "s3")} onChange={(e) => setParam(selected.id, "service", e.target.value, false)}>
                  {st.endpoint_services.map((s) => <option key={s.service} value={s.service}>{s.title}</option>)}</select></label>
                {st.endpoint_services.find((s) => s.service === selected.params.service)?.type === "gateway"
                  ? <div className="small muted">Free. Draw an arrow from each route table whose subnets should reach the service; the route goes to the service's prefix list.</div>
                  : <>
                    <label>Tier whose subnets get an interface<select value={String(selected.params.tier ?? "private")} onChange={(e) => setParam(selected.id, "tier", e.target.value, false)}>{st.tiers.map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
                    <label>Security group (must let 443 in from the VPC)<select value={String(selected.params.sg ?? "")} onChange={(e) => setParam(selected.id, "sg", e.target.value, false)}>
                      <option value="">none (the VPC's default group)</option>{design.nodes.filter((n) => n.kind === "sg" && n.parent === selected.parent).map((g) => <option key={g.id} value={g.id}>{g.label}</option>)}</select></label>
                    <div className="small muted">USD 0.01 an hour per subnet of the tier, plus USD 0.01 per GB; still far cheaper than a NAT Gateway for a few services.</div>
                  </>}
              </>}
              {selected.kind === "other" && <>
                <div className="msg info small">Read-only. The import found a <strong>{String(selected.params.what ?? "resource").replace(/_/g, " ")}</strong> here ({String(selected.params.aws_id ?? "")}{selected.params.subnet ? `, in ${selected.params.subnet}` : ""}). The palette has no block for it, so the app draws it grey, never changes it and writes nothing for it. {String(selected.params.note ?? "")}</div>
                <div className="small muted">Delete the block to hide it from the picture; the real thing stays.</div>
              </>}
              {selected.kind === "alb" && <>
                <label>Scheme<select value={String(selected.params.scheme ?? "internet")} onChange={(e) => setParam(selected.id, "scheme", e.target.value, false)}><option value="internet">internet-facing</option><option value="internal">internal (VPC only)</option></select></label>
                <label>Tier (its subnets, two AZs needed)<select value={String(selected.params.tier ?? "public")} onChange={(e) => setParam(selected.id, "tier", e.target.value, false)}>{st.tiers.map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
                <label>Security group (must let the port in)<select value={String(selected.params.sg ?? "")} onChange={(e) => setParam(selected.id, "sg", e.target.value, false)}>
                  <option value="">none</option>{design.nodes.filter((n) => n.kind === "sg" && n.parent === selected.parent).map((g) => <option key={g.id} value={g.id}>{g.label}</option>)}</select></label>
                <label>Port<select value={Number(selected.params.port ?? 80)} onChange={(e) => setParam(selected.id, "port", Number(e.target.value), false)}><option value={80}>80 (HTTP)</option><option value={443}>443 (HTTPS, needs a certificate)</option></select></label>
                <label>ACM certificate ARN (for HTTPS)<input value={String(selected.params.cert ?? "")} onChange={(e) => setParam(selected.id, "cert", e.target.value)} placeholder="arn:aws:acm:…" /></label>
                <label>Health check path<input value={String(selected.params.health ?? "/")} onChange={(e) => setParam(selected.id, "health", e.target.value)} /></label>
                <div className="small muted">Draw an arrow from the balancer to an Auto Scaling group or an instance; the arrow's port is where the targets listen.</div>
              </>}
              {selected.kind === "asg" && <>
                <label>Instance type<select value={String(selected.params.type ?? "t4g.micro")} onChange={(e) => setParam(selected.id, "type", e.target.value, false)}>
                  {st.instance_types.map((t) => { const h = design.region === "us-east-1" ? t.us_east_1 : design.region === "ap-south-1" ? t.ap_south_1 : t.us_east_1 * 1.1; return <option key={t.type} value={t.type}>{t.type} · {t.vcpu} vCPU · {t.memory} GiB · USD {(h * 730).toFixed(0)}/month each</option>; })}</select></label>
                <label>Tier (its subnets)<select value={String(selected.params.tier ?? "private")} onChange={(e) => setParam(selected.id, "tier", e.target.value, false)}>{st.tiers.map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
                <div className="row" style={{ margin: 0 }}>
                  <label>Min<Num min={0} style={{ width: 70 }} value={Number(selected.params.min ?? 1)} onCommit={(v) => setParam(selected.id, "min", v)} /></label>
                  <label>Desired<Num min={0} style={{ width: 70 }} value={Number(selected.params.desired ?? 1)} onCommit={(v) => setParam(selected.id, "desired", v)} /></label>
                  <label>Max<Num min={1} style={{ width: 70 }} value={Number(selected.params.max ?? 2)} onCommit={(v) => setParam(selected.id, "max", v)} /></label>
                </div>
                <label>Operating system<select value={String(selected.params.os ?? "al2023")} onChange={(e) => setParam(selected.id, "os", e.target.value, false)}>{st.os.map((o) => <option key={o.key} value={o.key}>{o.title}</option>)}</select></label>
                <label>Key pair (empty = none)<input value={String(selected.params.key ?? "")} onChange={(e) => setParam(selected.id, "key", e.target.value)} /></label>
                <label className="inline"><input type="checkbox" checked={!!selected.params.spot} onChange={(e) => setParam(selected.id, "spot", e.target.checked, false)} />Spot (about 70 % cheaper; the group replaces interrupted instances)</label>
                <label>Root volume (GiB)<Num min={8} value={Number(selected.params.disk ?? 8)} onCommit={(v) => setParam(selected.id, "disk", v)} /></label>
                <div>Security groups{design.nodes.filter((n) => n.kind === "sg" && n.parent === selected.parent).map((g) => {
                  const cur = Array.isArray(selected.params.sgs) ? (selected.params.sgs as string[]) : [];
                  return <label key={g.id} className="inline"><input type="checkbox" checked={cur.includes(g.id)} onChange={(e) => setParam(selected.id, "sgs", e.target.checked ? [...cur, g.id] : cur.filter((x) => x !== g.id), false)} />{g.label}</label>; })}</div>
              </>}
              {selected.kind === "lambda" && <>
                <label>What it does<select value={String(selected.params.action ?? "stop")} onChange={(e) => setParam(selected.id, "action", e.target.value, false)}>
                  <option value="stop">stop instances</option><option value="start">start instances</option><option value="custom">custom Python code</option></select></label>
                {String(selected.params.action ?? "stop") !== "custom" && <div>Instances<div className="small muted">{design.nodes.filter((n) => n.kind === "ec2").length === 0 && "none in the design yet"}</div>
                  {design.nodes.filter((n) => n.kind === "ec2").map((n) => {
                    const cur = Array.isArray(selected.params.instances) ? (selected.params.instances as string[]) : [];
                    return <label key={n.id} className="inline"><input type="checkbox" checked={cur.includes(n.id)} onChange={(e) => setParam(selected.id, "instances", e.target.checked ? [...cur, n.id] : cur.filter((x) => x !== n.id), false)} />{n.label}</label>; })}</div>}
                <div className="row">
                  <label style={{ flex: 1 }}>Time of day (HH:MM; empty = no schedule)<input value={String(selected.params.time ?? "")} onChange={(e) => setParam(selected.id, "time", e.target.value.trim())} placeholder="19:00" /></label>
                  <label style={{ flex: 1 }}>Days<select value={String(selected.params.days ?? "weekdays")} onChange={(e) => setParam(selected.id, "days", e.target.value, false)}>
                    <option value="weekdays">weekdays</option><option value="daily">every day</option><option value="weekends">weekends</option></select></label>
                </div>
                <label>Time zone<input list="aib-tz" value={String(selected.params.tz ?? "Asia/Kolkata")} onChange={(e) => setParam(selected.id, "tz", e.target.value.trim())} />
                  <datalist id="aib-tz">{["Asia/Kolkata", "UTC", "Europe/London", "Europe/Berlin", "America/New_York", "America/Los_Angeles", "Asia/Singapore", "Asia/Tokyo", "Australia/Sydney"].map((z) => <option key={z} value={z} />)}</datalist></label>
                <div className="row">
                  <label style={{ flex: 1 }}>Memory (MB, 128 to 10240)<Num min={128} max={10240} value={Number(selected.params.memory ?? 128)} onCommit={(v) => setParam(selected.id, "memory", v)} /></label>
                  <label style={{ flex: 1 }}>Timeout (s, 1 to 900)<Num min={1} max={900} value={Number(selected.params.timeout ?? 30)} onCommit={(v) => setParam(selected.id, "timeout", v)} /></label>
                </div>
                {String(selected.params.action ?? "stop") === "custom" && <label>Python code (handler(event, context) in index.py; boto3 is available)
                  <textarea rows={10} style={{ width: "100%", boxSizing: "border-box", fontFamily: "ui-monospace, Menlo, monospace", fontSize: 12 }} value={String(selected.params.code ?? "")} onChange={(e) => setParam(selected.id, "code", e.target.value)} placeholder={"def handler(event, context):\n    return {\"ok\": True}"} /></label>}
                <div className="small muted">Runtime python3.12. The role may only write logs{String(selected.params.action ?? "stop") !== "custom" ? ` and ${String(selected.params.action)} the ticked instances` : ""}. Free: 1,000,000 requests a month; the schedule costs nothing.</div>
              </>}
              {selected.kind === "s3" && <>
                <label>Expected storage (GB, for the estimate)<Num min={0} value={Number(selected.params.gb ?? 10)} onCommit={(v) => setParam(selected.id, "gb", v)} /></label>
                <label>Lifecycle (older objects to cheaper storage)<select value={String(selected.params.lifecycle ?? "none")} onChange={(e) => setParam(selected.id, "lifecycle", e.target.value, false)}>
                  <option value="none">none: Standard for ever (USD 0.023/GB)</option><option value="ia-30">after 30 days → Infrequent Access (USD 0.0125/GB)</option>
                  <option value="glacier-90">after 90 days → Glacier Instant (USD 0.004/GB)</option><option value="ia-30-glacier-90">30 days → IA, 90 days → Glacier</option></select></label>
                <label>Delete objects after (days; 0 = never)<Num min={0} value={Number(selected.params.expire_days ?? 0)} onCommit={(v) => setParam(selected.id, "expire_days", v)} /></label>
                <label className="inline"><input type="checkbox" checked={!!selected.params.versioning} onChange={(e) => setParam(selected.id, "versioning", e.target.checked, false)} />Versioning (keeps overwritten objects; costs storage)</label>
                <div className="small muted">Public access is always blocked and encryption always on; both are free.</div>
              </>}
              {selected.kind === "sg" && <>
                <label>Egress (traffic out)<select value={String(selected.params.egress ?? "all")} onChange={(e) => setParam(selected.id, "egress", e.target.value, false)}>
                  <option value="all">all: everything out (AWS's default)</option><option value="rules">rules: only the arrows leaving this group</option></select></label>
                <label>Description<input value={String(selected.params.description ?? "")} onChange={(e) => setParam(selected.id, "description", e.target.value)} /></label>
                <div className="small muted">Rules are the arrows: an arrow into this group lets that traffic in from its source; select an arrow to set the protocol and ports.</div>
              </>}
              {selected.kind === "ec2" && <>
                <label>Instance type (any; the table's ones are suggested)
                  <input list="aib-types" value={String(selected.params.type ?? "")} onChange={(e) => setParam(selected.id, "type", e.target.value.trim())} />
                  <datalist id="aib-types">{st.instance_types.map((t) => <option key={t.type} value={t.type}>{t.vcpu} vCPU · {t.memory} GiB · {t.arch}</option>)}</datalist>
                </label>
                {(() => { const t = st.instance_types.find((x) => x.type === selected.params.type); const hrs = Number(selected.params.hours ?? 730); if (!t) return <div className="small muted">Not in the app's table: price unknown; it is used as written.</div>; const h = design.region === "us-east-1" ? t.us_east_1 : design.region === "ap-south-1" ? t.ap_south_1 : t.us_east_1 * 1.1; return <div className="small muted">{t.vcpu} vCPU · {t.memory} GiB · {t.arch} · USD {(h * hrs).toFixed(2)}/month at {hrs} h</div>; })()}
                <div className="row" style={{ alignItems: "flex-end" }}>
                  <label style={{ flex: 1 }}>Hours a month it runs (the price follows this)<select value={HOUR_PRESETS.some(([v]) => v === Number(selected.params.hours ?? 730)) ? Number(selected.params.hours ?? 730) : "custom"} onChange={(e) => setParam(selected.id, "hours", e.target.value === "custom" ? 100 : Number(e.target.value), false)}>
                    {HOUR_PRESETS.map(([v, t]) => <option key={v} value={v}>{t}</option>)}<option value="custom">custom…</option></select></label>
                  <button className="small" onClick={() => setNewInst("change")}>Recommend a type…</button>
                </div>
                {!HOUR_PRESETS.some(([v]) => v === Number(selected.params.hours ?? 730)) && <label>Hours (1 to 744)<Num min={1} max={744} value={Number(selected.params.hours ?? 730)} onCommit={(v) => setParam(selected.id, "hours", v)} /></label>}
                <div className="small muted">{hoursText(Number(selected.params.hours ?? 730))}: stop the instance when it is not in use (only the volume is charged then), or the estimate will not hold.</div>
                <label>Operating system<select value={String(selected.params.os ?? "al2023")} onChange={(e) => setParam(selected.id, "os", e.target.value, false)}>{st.os.map((o) => <option key={o.key} value={o.key}>{o.title}</option>)}</select></label>
                {(() => { const o = st.os.find((x) => x.key === selected.params.os); return o?.note ? <div className="small muted">{o.title}: {o.note}.</div> : null; })()}
                <label>Key pair (an existing EC2 key pair name; empty = none, use SSM)
                  <input list="aib-keynames-insp" value={String(selected.params.key ?? "")} onChange={(e) => setParam(selected.id, "key", e.target.value)} />
                  <datalist id="aib-keynames-insp">{keyNames.map((n) => <option key={n} value={n} />)}</datalist>
                </label>
                <label className="inline"><input type="checkbox" checked={!!selected.params.public_ip} onChange={(e) => setParam(selected.id, "public_ip", e.target.checked, false)} />Public IPv4 address (USD 3.65 a month)</label>
                <label className="inline"><input type="checkbox" checked={!!selected.params.spot} onChange={(e) => setParam(selected.id, "spot", e.target.checked, false)} />Spot (about 70 % cheaper; may be interrupted: for training and batch work)</label>
                <div className="row" style={{ alignItems: "baseline" }}>
                  <button className="small primary" onClick={() => window.dispatchEvent(new CustomEvent("aib-connect", { detail: selected.id }))}>Connect (SSH) →</button>
                  <span className="small muted">{selected.params.address ? `${selected.params.address}` : "in the Terminal tab; the address is known after Apply or an import"}</span>
                </div>
                {selected.params.aws_id && <div className="row" style={{ alignItems: "baseline", flexWrap: "wrap" }}>
                  <span className="small">In AWS: <strong>{String(selected.params.state ?? "unknown")}</strong></span>
                  <button className="small" disabled={powerBusy} onClick={() => power(selected, null)}>Refresh</button>
                  {String(selected.params.state) === "running"
                    ? <button className="small danger" disabled={powerBusy} onClick={() => power(selected, false)}>Stop (only the volume and address are charged then)</button>
                    : armed === selected.id
                      ? <button className="small primary" disabled={powerBusy} onClick={() => power(selected, true)}>Really start: USD {hourlyOf(String(selected.params.type ?? "")).toFixed(4)} an hour from now on</button>
                      : <button className="small" disabled={powerBusy} onClick={() => setArmed(selected.id)}>Start…</button>}
                </div>}
                <div className="row" style={{ alignItems: "baseline" }}>
                  <button className="small" onClick={() => schedule(selected)}>Schedule stop and start</button>
                  <span className="small muted">two Lambda functions: stop at 19:00, start at 09:00, weekdays (free); edit them afterwards</span>
                </div>
                <label>Root volume (GiB, USD 0.08 per GiB a month)<Num min={8} max={16384} value={Number(selected.params.disk ?? 8)} onCommit={(v) => setParam(selected.id, "disk", v)} /></label>
                <div>Security groups<div className="small muted">{design.nodes.filter((n) => n.kind === "sg" && n.parent === design.nodes.find((x) => x.id === selected.parent)?.parent).length === 0 && "none in this VPC yet: add one from the palette"}</div>
                  {design.nodes.filter((n) => n.kind === "sg" && n.parent === design.nodes.find((x) => x.id === selected.parent)?.parent).map((g) => {
                    const cur = Array.isArray(selected.params.sgs) ? (selected.params.sgs as string[]) : [];
                    return <label key={g.id} className="inline"><input type="checkbox" checked={cur.includes(g.id)} onChange={(e) => setParam(selected.id, "sgs", e.target.checked ? [...cur, g.id] : cur.filter((x) => x !== g.id), false)} />{g.label}</label>; })}</div>
              </>}
            </div>
            {problemsFor(result)}
            {fixesFor(selected.id)}
            {result.info.length > 0 && <><h4>Facts</h4><ul className="info">{result.info.map((s, i) => <li key={i}>{s}</li>)}</ul></>}
            {selected.kind === "subnet" && (() => { const c = parseCidr(String(selected.params.cidr ?? "")); return c ? <p className="small muted">{cidrText(c)} → {size(c).toLocaleString()} − 5 = {(size(c) - 5).toLocaleString()} usable</p> : null; })()}
            <h4>Lives in</h4>
            <p className="small">{selected.parent ? design.nodes.find((n) => n.id === selected.parent)?.label ?? "?" : `the region ${design.region}`}</p>
            <div className="row"><button className="danger small" onClick={() => onDelete({ nodes: nodes.filter((n) => descendants(design, selected.id).includes(n.id)), edges: [] })}>Delete{CONTAINER(selected.kind) ? " with contents" : ""}</button></div>
          </>
        ) : edge && edgeResult ? (
          <>
            <h3>{edge.kind === "route" ? "Route" : edge.kind === "traffic" ? "Traffic rule" : edge.kind === "target" ? "Target" : "Attachment"}</h3>
            <p className="small">{design.nodes.find((n) => n.id === edge.source)?.label} → {design.nodes.find((n) => n.id === edge.target)?.label}</p>
            {edge.kind === "route" && <div className="fields"><label>Destination<input value={String(edge.params.dest ?? "")} onChange={(e) => setEdgeDest(edge.id, e.target.value)} placeholder="0.0.0.0/0" /></label>
              <div className="small muted">0.0.0.0/0 = everything not inside the VPC (the internet)</div></div>}
            {edge.kind === "attach" && <p className="small muted">The subnet uses this route table. A subnet uses one table; a table can serve many subnets.</p>}
            {edge.kind === "target" && <div className="fields"><label>Port the targets listen on<Num min={1} max={65535} value={Number(edge.params.port ?? 8080)} onCommit={(v) => change({ ...design, edges: design.edges.map((x) => (x.id === edge.id ? { ...x, params: { ...x.params, port: v } } : x)) }, true)} /></label>
              <div className="small muted">The target's security group must let this port in from the balancer's group (a traffic arrow between the two groups).</div></div>}
            {edge.kind === "traffic" && <div className="fields">
              <label>Protocol<select value={String(edge.params.proto ?? "tcp")} onChange={(e) => change({ ...design, edges: design.edges.map((x) => (x.id === edge.id ? { ...x, params: { ...x.params, proto: e.target.value } } : x)) })}>
                <option value="tcp">tcp</option><option value="udp">udp</option><option value="icmp">icmp (ping)</option><option value="all">all traffic</option></select></label>
              {(edge.params.proto === "tcp" || edge.params.proto === "udp" || !edge.params.proto) && <label>Ports<input value={String(edge.params.ports ?? "")} onChange={(e) => change({ ...design, edges: design.edges.map((x) => (x.id === edge.id ? { ...x, params: { ...x.params, ports: e.target.value } } : x)) }, true)} placeholder="443, 8080-8090, ssh, postgres, all" list="aib-ports" />
                <datalist id="aib-ports">{st.ports.map((p) => <option key={p.name} value={p.name}>{p.port}</option>)}</datalist></label>}
              <div className="small muted">{design.nodes.find((n) => n.id === edge.target)?.label} lets this traffic in from {design.nodes.find((n) => n.id === edge.source)?.label}; {design.nodes.find((n) => n.id === edge.source)?.label} lets it out (when its egress is "rules").</div>
            </div>}
            {problemsFor(edgeResult)}
            {fixesFor(edge.id)}
            <div className="row"><button className="danger small" onClick={() => onDelete({ nodes: [], edges: edges.filter((e) => e.id === edge.id) })}>Delete arrow</button></div>
          </>
        ) : (
          <>
            <h3>Design</h3>
            <div className="fields">
              <label>Name<input value={design.name} onChange={(e) => change({ ...design, name: e.target.value }, true)} /></label>
              <label>Region<select value={design.region} onChange={(e) => change({ ...design, region: e.target.value })}>
                {st.regions.map((r) => <option key={r.id} value={r.id}>{r.id} · {r.name} ({r.azs} AZs)</option>)}</select></label>
              <label>Availability Zones<input type="number" min={1} max={6} value={design.azs} onChange={(e) => change({ ...design, azs: Math.max(1, Math.min(6, Number(e.target.value) || 1)) })} /></label>
            </div>
            <h4>Summary</h4>
            <div className="counts">{st.kinds.map((k) => <span key={k.kind} className="pill">{analysis.counts[k.kind] ?? 0} {k.title}{(analysis.counts[k.kind] ?? 0) === 1 ? "" : "s"}</span>)}
              <span className="pill">{analysis.az_spread} AZ{analysis.az_spread === 1 ? "" : "s"} used</span></div>
            <h4>Cost</h4>
            <p className="small"><strong>≈ USD {analysis.cost.total.toFixed(2)} a month</strong> while idle (estimate, {analysis.cost.region}). {analysis.cost.advice[0] ?? ""}</p>
            {analysis.recommendations.length > 0 && <>
              <h4>Tidy up <span className="pill warn">{analysis.recommendations.length}</span></h4>
              <ul className="tidy">{analysis.recommendations.map((r) => <li key={r.id} className={`tidy-${r.kind}`}>
                <div className="row" style={{ margin: 0, justifyContent: "space-between" }}><strong>{r.title}</strong><button className="small primary" onClick={() => tidy(r.id)}>{r.kind === "delete" ? "Delete" : r.kind === "combine" ? "Combine" : "Merge"}</button></div>
                <div className="small muted">{r.detail}</div>
                {r.widens.length > 0 && <div className="small st-warn">Opens more: {r.widens.join(" · ")}</div>}
              </li>)}</ul>
              <p className="small muted">Each one is one undo step. Merged groups keep the kept group's name; the instances switch over; the arrows move once each.</p>
            </>}
            <h4>Problems</h4>
            {analysis.problems.length === 0 ? <p className="st-ok">None: every CIDR fits, every subnet routes as its tier says.</p>
              : <ul className="problems">{analysis.problems.map((m, i) => <li key={i} className={analysis.errors && i < analysis.errors ? "" : ""}>{m}</li>)}</ul>}
            {analysis.fixes.length > 0 && <>
              <h4>Fixes <span className="pill">{analysis.fixes.length}</span>{analysis.fixes.some((f) => f.safe) && <button className="small primary" style={{ marginLeft: 8 }} onClick={() => fixIt(null, true)}>Fix all safe ones ({analysis.fixes.filter((f) => f.safe).length})</button>}</h4>
              <ul className="fixes">{analysis.fixes.map((f) => <li key={f.id} className={f.safe ? "safe" : ""}>
                <div className="row" style={{ margin: 0, justifyContent: "space-between" }}><span><strong>{f.target_label}</strong>: {f.title}</span><button className="small" onClick={() => fixIt(f.id, false)}>Fix</button></div>
                {f.note && <div className="small muted">{f.note}</div>}
              </li>)}</ul>
              <p className="small muted">Safe = costs nothing, opens nothing, needs nothing looked up. The others (a public IP, a narrower egress, Graviton, your own address) are one click each.</p>
            </>}
            <p className="small muted">Click a block or an arrow to change it. Red = AWS would refuse it or the design contradicts itself; yellow = works but probably not what you want.</p>
          </>
        )}
      </aside>
    </div>
  );
}
