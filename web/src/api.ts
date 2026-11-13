// Typed calls to the Rust side (Tauri commands in app/src-tauri/src/main.rs).
import { invoke } from "@tauri-apps/api/core";

export type Params = Record<string, unknown>;
export interface DNode { id: string; kind: string; label: string; params: Params; parent: string | null; x: number; y: number; w: number; h: number }
export interface DEdge { id: string; source: string; target: string; kind: string; params: Params }
export interface Design { name: string; region: string; azs: number; nodes: DNode[]; edges: DEdge[]; text?: string }

export interface NodeResult { id: string; kind: string; title: string; status: "ok" | "warn" | "error"; messages: string[]; info: string[]; summary: string }
export interface EdgeResult { id: string; status: "ok" | "warn" | "error"; messages: string[]; label: string }
export interface CostItem { id: string; label: string; kind: string; monthly: number; note: string }
export interface CostReport { items: CostItem[]; total: number; advice: string[]; region: string; assumptions: string[] }
export interface Recommendation { id: string; kind: "delete" | "merge" | "combine"; title: string; detail: string; keep: string | null; drop: string[]; label: string | null; widens: string[] }
export interface Fix { id: string; kind: string; target: string; target_label: string; title: string; note: string; safe: boolean; needs_ip: boolean }
export interface Analysis {
  nodes: NodeResult[]; edges: EdgeResult[]; problems: string[]; ok: boolean; errors: number; warnings: number;
  counts: Record<string, number>; az_spread: number; az_names: string[]; recommendations: Recommendation[]; fixes: Fix[]; cost: CostReport;
}
/** block id -> [width, height] as the canvas measured them; the Rust side lays out with these */
export type Sizes = Record<string, [number, number]>;
export interface Profile { name: string; kind: string; region: string | null; key_hint: string | null }
export type Source = { kind: "none" } | { kind: "profile"; name: string } | { kind: "keys" };
export interface Identity { account: string; arn: string; user_id: string; region: string; checked: number }
export interface AwsState { profiles: Profile[]; aws_dir: string; source: Source; keys: { hint: string; temporary: boolean } | null; region: string | null; identity: Identity | null; policy: string }
export interface VpcInfo { id: string; name: string | null; cidr: string; is_default: boolean; subnets: number }
export interface Change { address: string; resource_type: string; name: string; action: string; block_id: string | null; block_label: string | null; replace_because: string[] }
export interface PlanSummary { creates: number; updates: number; replaces: number; destroys: number; changes: Change[]; by_block: Record<string, string>; cost_delta: number; summary: string; empty: boolean }
export interface JobSnap { state: "idle" | "running" | "done" | "failed"; kind: string; log: string[]; next: number; message: string; plan: PlanSummary | null; outputs: [string, string][]; marks: Record<string, string>; aws_ids: Record<string, string>; started: number }
export interface TofuStatus { tofu: string | null; version: string | null; install_version: string; source: string; identity: Identity | null; project_account: string | null; budget: number | null;
  state: { has_state: boolean; resources: number; last_apply: string | null; backups: number; plan_fresh: boolean; plan_destroy: boolean } }
export interface Loaded { design: Design; analysis: Analysis }
export interface Host { id: string; label: string; host: string; user: string; port: number; key: string; block: string | null; aws_id: string | null; note: string }
export interface HostsState { saved: Host[]; suggested: Host[]; ssh: string | null }
export interface KeyFile { path: string; name: string; kind: string }
export interface KeyPair { name: string; kind: string; fingerprint: string; file: string | null }
export interface TextError { line: number; message: string }

export interface ProjectInfo { name: string; path: string; draft: boolean }
export interface AppState {
  version: string; project: ProjectInfo | null; projects: { path: string; exists: boolean; name: string }[]; projects_dir: string;
  templates: { key: string; title: string; about: string }[]; regions: { id: string; name: string; azs: number }[];
  kinds: { kind: string; title: string; parent: string }[]; tiers: string[];
  instance_types: { type: string; vcpu: number; memory: number; arch: string; us_east_1: number; ap_south_1: number }[];
  os: { key: string; title: string }[]; ports: { name: string; port: number }[]; endpoint_services: { service: string; title: string; type: string }[];
}

export const errText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));

export const api = {
  state: () => invoke<AppState>("app_state"),
  projectNew: (name: string, location?: string) => invoke<ProjectInfo>("project_new", { name, location: location ?? null }),
  projectOpen: (path: string) => invoke<ProjectInfo>("project_open", { path }),
  projectFromTemplate: (key: string) => invoke<ProjectInfo>("project_from_template", { key }),
  projectSaveAs: (name: string, location?: string) => invoke<ProjectInfo>("project_save_as", { name, location: location ?? null }),
  projectClose: () => invoke<void>("project_close"),
  projectForget: (path: string) => invoke<void>("project_forget", { path }),
  setProjectsDir: (path: string) => invoke<void>("set_projects_dir", { path }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  load: () => invoke<Loaded>("project_load"),
  saveDesign: (design: Design) => invoke<Loaded>("save_design", { design }),
  designFromText: (text: string) => invoke<{ ok: boolean; design?: Design; analysis?: Analysis; errors: TextError[] }>("design_from_text", { text }),
  designArrange: (design: Design, sizes?: Sizes) => invoke<Loaded>("design_arrange", { design, sizes: sizes ?? null }),
  designFix: (design: Design, id: string | null, allSafe: boolean, sizes?: Sizes) => invoke<Loaded & { done: number }>("design_fix", { design, id, allSafe, sizes: sizes ?? null }),
  designTidy: (design: Design, id: string, sizes?: Sizes) => invoke<Loaded>("design_tidy", { design, id, sizes: sizes ?? null }),
  designAdd: (design: Design, kind: string, parent: string | null) => invoke<Loaded & { id: string | null }>("design_add", { design, kind, parent }),
  designConnect: (design: Design, source: string, target: string) => invoke<Loaded & { id: string | null }>("design_connect", { design, source, target }),
  designRemove: (design: Design, nodes: string[], edges: string[]) => invoke<Loaded>("design_remove", { design, nodes, edges }),
  textHelp: () => invoke<[string, string][]>("text_help"),
  code: (design: Design) => invoke<Record<string, string>>("code", { design }),
  tofuStatus: () => invoke<TofuStatus>("tofu_status"),
  tofuInstall: () => invoke<void>("tofu_install"),
  tofuPlan: (destroy: boolean) => invoke<void>("tofu_plan", { destroy }),
  tofuApply: (confirm: string, acceptBudget: boolean) => invoke<void>("tofu_apply", { confirm, acceptBudget }),
  job: (since: number) => invoke<JobSnap>("job_status", { since }),
  projectSetBudget: (budget: number | null) => invoke<void>("project_set_budget", { budget }),
  designSetAwsIds: (ids: Record<string, string>, clear: boolean, outputs?: [string, string][]) => invoke<Loaded>("design_set_aws_ids", { ids, clear, outputs: outputs ?? null }),
  hostsState: (design: Design | null) => invoke<HostsState>("hosts_state", { design }),
  hostsSave: (hosts: Host[]) => invoke<Host[]>("hosts_save", { hosts }),
  keyCheck: (path: string) => invoke<string | null>("key_check", { path }),
  keyFix: (path: string) => invoke<string | null>("key_fix", { path }),
  pickFile: () => invoke<string | null>("pick_file"),
  termOpen: (id: string, host: Host, cols: number, rows: number) => invoke<string[]>("term_open", { id, host, cols, rows }),
  termWrite: (id: string, data: string) => invoke<void>("term_write", { id, data }),
  termResize: (id: string, cols: number, rows: number) => invoke<void>("term_resize", { id, cols, rows }),
  termClose: (id: string) => invoke<void>("term_close", { id }),
  termList: () => invoke<string[]>("term_list"),
  keysList: () => invoke<{ dir: string; keys: KeyFile[] }>("keys_list"),
  keyMove: (path: string) => invoke<string>("key_move", { path }),
  awsKeyPairs: (region?: string) => invoke<{ region: string; pairs: KeyPair[] }>("aws_key_pairs", { region: region ?? null }),
  awsCreateKeyPair: (name: string, region?: string) => invoke<{ name: string; path: string; region: string }>("aws_create_key_pair", { name, region: region ?? null }),
  exportCode: () => invoke<string>("export_code"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  awsState: () => invoke<AwsState>("aws_state"),
  awsSetKeys: (id: string, secretKey: string, token: string) => invoke<{ hint: string; temporary: boolean }>("aws_set_keys", { id, secretKey, token }),
  awsClearKeys: () => invoke<void>("aws_clear_keys"),
  awsSelect: (source: Source, region?: string) => invoke<void>("aws_select", { source, region: region ?? null }),
  awsTest: (region?: string) => invoke<Identity>("aws_test", { region: region ?? null }),
  awsListVpcs: (region?: string) => invoke<{ region: string; vpcs: VpcInfo[] }>("aws_list_vpcs", { region: region ?? null }),
  awsImport: (vpcId: string, region?: string) => invoke<{ project: ProjectInfo; notes: string[] }>("aws_import", { region: region ?? null, vpcId }),
};
