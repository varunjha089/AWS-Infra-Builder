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
export interface Analysis {
  nodes: NodeResult[]; edges: EdgeResult[]; problems: string[]; ok: boolean; errors: number; warnings: number;
  counts: Record<string, number>; az_spread: number; az_names: string[]; cost: CostReport;
}
export interface Profile { name: string; kind: string; region: string | null; key_hint: string | null }
export type Source = { kind: "none" } | { kind: "profile"; name: string } | { kind: "keys" };
export interface Identity { account: string; arn: string; user_id: string; region: string; checked: number }
export interface AwsState { profiles: Profile[]; aws_dir: string; source: Source; keys: { hint: string; temporary: boolean } | null; region: string | null; identity: Identity | null; policy: string }
export interface VpcInfo { id: string; name: string | null; cidr: string; is_default: boolean; subnets: number }
export interface Loaded { design: Design; analysis: Analysis }
export interface TextError { line: number; message: string }

export interface ProjectInfo { name: string; path: string; draft: boolean }
export interface AppState {
  version: string; project: ProjectInfo | null; projects: { path: string; exists: boolean; name: string }[]; projects_dir: string;
  templates: { key: string; title: string; about: string }[]; regions: { id: string; name: string; azs: number }[];
  kinds: { kind: string; title: string; parent: string }[]; tiers: string[];
  instance_types: { type: string; vcpu: number; memory: number; arch: string; us_east_1: number; ap_south_1: number }[];
  os: { key: string; title: string }[]; ports: { name: string; port: number }[];
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
  designArrange: (design: Design) => invoke<Loaded>("design_arrange", { design }),
  designAdd: (design: Design, kind: string, parent: string | null) => invoke<Loaded & { id: string | null }>("design_add", { design, kind, parent }),
  designConnect: (design: Design, source: string, target: string) => invoke<Loaded & { id: string | null }>("design_connect", { design, source, target }),
  designRemove: (design: Design, nodes: string[], edges: string[]) => invoke<Loaded>("design_remove", { design, nodes, edges }),
  textHelp: () => invoke<[string, string][]>("text_help"),
  code: (design: Design) => invoke<Record<string, string>>("code", { design }),
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
