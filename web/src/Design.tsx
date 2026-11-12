// The canvas: VPCs and subnets are boxes that hold other blocks (React Flow nested nodes); route tables, gateways are small blocks;
// arrows are attachments (subnet → route table) and routes (route table → gateway). A palette on the left, the inspector on the right.
import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  ReactFlow, ReactFlowProvider, Background, Controls, MiniMap, Handle, Position, NodeResizer, applyEdgeChanges, applyNodeChanges, useReactFlow,
  MarkerType, type Connection, type Edge, type EdgeChange, type Node, type NodeChange, type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { api, errText, type Analysis, type AppState, type DNode, type Design, type EdgeResult, type Loaded, type NodeResult } from "./api";
import { cidrText, nextFree, parseCidr, size } from "./cidr";
import { neon, useTheme } from "./theme";

export const COLORS: Record<string, string> = { vpc: "#e07b00", subnet: "#2f6fde", public: "#2e7d32", private: "#2f6fde", isolated: "#6b7280", igw: "#7c3aed", nat: "#0e7490", rt: "#8e5bb5", sg: "#c0392b", ec2: "#b7791f", cidr: "#475569" };
const CONTAINER = (kind: string) => kind === "vpc" || kind === "subnet";

type Data = { node: DNode; result?: NodeResult; azName: string };

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
      <div className="bh">
        <span>{isVpc ? "VPC" : String(n.params.tier ?? "subnet")}{mark(st)}</span><span className="bl">{n.label}</span>
        <span className="bs" title={r?.summary}>{isVpc ? String(n.params.cidr ?? "") : `${n.params.cidr || "no CIDR"} · ${data.azName}`}</span>
      </div>
      {!isVpc && r && <div className={`bm st-${st}`}>{r.status !== "ok" ? r.messages[0] : r.info[0]?.replace(/^.*= /, "") + (r.info[0] ? " usable" : "")}</div>}
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
    <div className={`block st-${st} ${selected ? "sel" : ""}`} style={{ "--c": color(n) } as CSSProperties}>
      {(n.kind === "rt" || n.kind === "sg" || n.kind === "cidr") && <Handle type="target" position={Position.Left} />}
      {(n.kind === "igw" || n.kind === "nat") && <Handle type="target" position={Position.Top} />}
      <div className="lh"><span>{r?.title ?? n.kind}{mark(st)}</span><span className="bl">{n.label}</span></div>
      <div className={`ls st-${st}`}>{st !== "ok" ? r?.messages[0] : r?.summary || ""}</div>
      {n.kind === "rt" && <Handle type="source" position={Position.Right} title="drag to a gateway" />}
      {(n.kind === "sg" || n.kind === "cidr") && <Handle type="source" position={Position.Right} title="drag to a security group or an address range: traffic" />}
    </div>
  );
}
const nodeTypes = { box: memo(BoxNode), block: memo(BlockNode) };

/** the design's blocks as React Flow nodes (containers first, positions relative to the container) */
function toNodes(d: Design, a: Analysis, azNames: string[]): Node<Data>[] {
  const res = Object.fromEntries(a.nodes.map((r) => [r.id, r]));
  return d.nodes.map((n) => {
    const azIx = "abcdef".indexOf(String(n.params.az ?? ""));
    const azName = azNames[azIx] ?? `${d.region}${n.params.az ?? ""}`;
    const container = CONTAINER(n.kind);
    return {
      id: n.id, type: container ? "box" : "block", position: { x: n.x, y: n.y }, data: { node: n, result: res[n.id], azName },
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
  selectId: string | null; setSelectId: (id: string | null) => void; signal: { fit: number };
}

export function DesignTab(p: Props) {
  return <ReactFlowProvider><Canvas {...p} /></ReactFlowProvider>;
}

function Canvas({ design, analysis, st, change, apply, toast, selectId, setSelectId, signal }: Props) {
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
      return toNodes(design, analysis, analysis.az_names).map((n) => ({ ...n, selected: sel.has(n.id) }));
    });
    setEdges((old) => { const sel = new Set(old.filter((e) => e.selected).map((e) => e.id)); return toEdges(design, analysis).map((e) => ({ ...e, selected: sel.has(e.id) })); });
  }, [design, analysis, selectId]);
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
    api.designAdd(design, kind, parent).then((l) => { apply(l); if (l.id) setSelectId(l.id); }).catch((e) => toast(errText(e)));
  }, [design, parentKind, selectId, apply, toast, setSelectId]);
  useEffect(() => {
    const h = (e: Event) => add((e as CustomEvent<string>).detail);
    window.addEventListener("aib-add", h);
    return () => window.removeEventListener("aib-add", h);
  }, [add]);

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
      <aside className="palette">
        <h4>Scope</h4>
        <button className="pal" style={pal("vpc")} onClick={() => add("vpc")}>VPC<small>a private network in the region</small></button>
        <h4>Network</h4>
        <button className="pal" style={pal("subnet")} onClick={() => add("subnet")}>Subnet<small>public, private or isolated, in one AZ</small></button>
        <button className="pal" style={pal("igw")} onClick={() => add("igw")}>Internet Gateway<small>one per VPC</small></button>
        <button className="pal" style={pal("nat")} onClick={() => add("nat")}>NAT Gateway<small>in a public subnet, ≈ USD 32 / month</small></button>
        <button className="pal" style={pal("rt")} onClick={() => add("rt")}>Route table<small>subnet → table → gateway</small></button>
        <h4>Security</h4>
        <button className="pal" style={pal("sg")} onClick={() => add("sg")}>Security group<small>arrows between groups = allowed traffic</small></button>
        <button className="pal" style={pal("cidr")} onClick={() => add("cidr")}>Address range<small>the internet, your office IP</small></button>
        <h4>Compute</h4>
        <button className="pal" style={pal("ec2")} onClick={() => add("ec2")}>EC2 instance<small>cheapest type by default, no public IP</small></button>
        <h4>Arrows</h4>
        <p className="small muted" style={{ margin: "4px 0" }}>Subnet → route table: the subnet uses it. Route table → gateway: a route. Group or range → group: the target lets that traffic in (ports in the inspector).</p>
        <h4>Layout</h4>
        <button className="pal" style={pal("rt")} onClick={() => api.designArrange(design).then(apply).then(() => window.setTimeout(() => rf.fitView({ padding: 0.15, duration: 300 }), 60)).catch((e) => toast(errText(e)))}>Arrange<small>AZ columns × tier rows (⇧⌘A)</small></button>
        <button className="pal" style={pal("rt")} onClick={() => rf.fitView({ padding: 0.15, duration: 300 })}>Fit to window<small>⇧⌘F</small></button>
      </aside>
      <div className="canvas">
        {design.nodes.length === 0 && <div className="msg info big-hint">Empty design: add a VPC from the palette, then subnets, an Internet Gateway and a route table. Or open a template from the Projects page.</div>}
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
              {selected.kind === "sg" && <>
                <label>Egress (traffic out)<select value={String(selected.params.egress ?? "all")} onChange={(e) => setParam(selected.id, "egress", e.target.value, false)}>
                  <option value="all">all: everything out (AWS's default)</option><option value="rules">rules: only the arrows leaving this group</option></select></label>
                <label>Description<input value={String(selected.params.description ?? "")} onChange={(e) => setParam(selected.id, "description", e.target.value)} /></label>
                <div className="small muted">Rules are the arrows: an arrow into this group lets that traffic in from its source; select an arrow to set the protocol and ports.</div>
              </>}
              {selected.kind === "ec2" && <>
                <label>Instance type<select value={String(selected.params.type ?? "t4g.micro")} onChange={(e) => setParam(selected.id, "type", e.target.value, false)}>
                  {st.instance_types.map((t) => { const h = design.region === "us-east-1" ? t.us_east_1 : design.region === "ap-south-1" ? t.ap_south_1 : t.us_east_1 * 1.1; return <option key={t.type} value={t.type}>{t.type} · {t.vcpu} vCPU · {t.memory} GiB · {t.arch} · USD {(h * 730).toFixed(0)}/month</option>; })}</select></label>
                <label>Operating system<select value={String(selected.params.os ?? "al2023")} onChange={(e) => setParam(selected.id, "os", e.target.value, false)}>{st.os.map((o) => <option key={o.key} value={o.key}>{o.title}</option>)}</select></label>
                <label>Key pair (an existing EC2 key pair name; empty = none, use SSM)<input value={String(selected.params.key ?? "")} onChange={(e) => setParam(selected.id, "key", e.target.value)} /></label>
                <label className="inline"><input type="checkbox" checked={!!selected.params.public_ip} onChange={(e) => setParam(selected.id, "public_ip", e.target.checked, false)} />Public IPv4 address (USD 3.65 a month)</label>
                <div>Security groups<div className="small muted">{design.nodes.filter((n) => n.kind === "sg" && n.parent === design.nodes.find((x) => x.id === selected.parent)?.parent).length === 0 && "none in this VPC yet: add one from the palette"}</div>
                  {design.nodes.filter((n) => n.kind === "sg" && n.parent === design.nodes.find((x) => x.id === selected.parent)?.parent).map((g) => {
                    const cur = Array.isArray(selected.params.sgs) ? (selected.params.sgs as string[]) : [];
                    return <label key={g.id} className="inline"><input type="checkbox" checked={cur.includes(g.id)} onChange={(e) => setParam(selected.id, "sgs", e.target.checked ? [...cur, g.id] : cur.filter((x) => x !== g.id), false)} />{g.label}</label>; })}</div>
              </>}
            </div>
            {problemsFor(result)}
            {result.info.length > 0 && <><h4>Facts</h4><ul className="info">{result.info.map((s, i) => <li key={i}>{s}</li>)}</ul></>}
            {selected.kind === "subnet" && (() => { const c = parseCidr(String(selected.params.cidr ?? "")); return c ? <p className="small muted">{cidrText(c)} → {size(c).toLocaleString()} − 5 = {(size(c) - 5).toLocaleString()} usable</p> : null; })()}
            <h4>Lives in</h4>
            <p className="small">{selected.parent ? design.nodes.find((n) => n.id === selected.parent)?.label ?? "?" : `the region ${design.region}`}</p>
            <div className="row"><button className="danger small" onClick={() => onDelete({ nodes: nodes.filter((n) => descendants(design, selected.id).includes(n.id)), edges: [] })}>Delete{CONTAINER(selected.kind) ? " with contents" : ""}</button></div>
          </>
        ) : edge && edgeResult ? (
          <>
            <h3>{edge.kind === "route" ? "Route" : edge.kind === "traffic" ? "Traffic rule" : "Attachment"}</h3>
            <p className="small">{design.nodes.find((n) => n.id === edge.source)?.label} → {design.nodes.find((n) => n.id === edge.target)?.label}</p>
            {edge.kind === "route" && <div className="fields"><label>Destination<input value={String(edge.params.dest ?? "")} onChange={(e) => setEdgeDest(edge.id, e.target.value)} placeholder="0.0.0.0/0" /></label>
              <div className="small muted">0.0.0.0/0 = everything not inside the VPC (the internet)</div></div>}
            {edge.kind === "attach" && <p className="small muted">The subnet uses this route table. A subnet uses one table; a table can serve many subnets.</p>}
            {edge.kind === "traffic" && <div className="fields">
              <label>Protocol<select value={String(edge.params.proto ?? "tcp")} onChange={(e) => change({ ...design, edges: design.edges.map((x) => (x.id === edge.id ? { ...x, params: { ...x.params, proto: e.target.value } } : x)) })}>
                <option value="tcp">tcp</option><option value="udp">udp</option><option value="icmp">icmp (ping)</option><option value="all">all traffic</option></select></label>
              {(edge.params.proto === "tcp" || edge.params.proto === "udp" || !edge.params.proto) && <label>Ports<input value={String(edge.params.ports ?? "")} onChange={(e) => change({ ...design, edges: design.edges.map((x) => (x.id === edge.id ? { ...x, params: { ...x.params, ports: e.target.value } } : x)) }, true)} placeholder="443, 8080-8090, ssh, postgres, all" list="aib-ports" />
                <datalist id="aib-ports">{st.ports.map((p) => <option key={p.name} value={p.name}>{p.port}</option>)}</datalist></label>}
              <div className="small muted">{design.nodes.find((n) => n.id === edge.target)?.label} lets this traffic in from {design.nodes.find((n) => n.id === edge.source)?.label}; {design.nodes.find((n) => n.id === edge.source)?.label} lets it out (when its egress is "rules").</div>
            </div>}
            {problemsFor(edgeResult)}
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
            <h4>Problems</h4>
            {analysis.problems.length === 0 ? <p className="st-ok">None: every CIDR fits, every subnet routes as its tier says.</p>
              : <ul className="problems">{analysis.problems.map((m, i) => <li key={i} className={analysis.errors && i < analysis.errors ? "" : ""}>{m}</li>)}</ul>}
            <p className="small muted">Click a block or an arrow to change it. Red = AWS would refuse it or the design contradicts itself; yellow = works but probably not what you want.</p>
          </>
        )}
      </aside>
    </div>
  );
}
