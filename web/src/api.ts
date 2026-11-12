// Typed calls to the Rust side (Tauri commands in app/src-tauri/src/main.rs).
import { invoke } from "@tauri-apps/api/core";

export type Params = Record<string, unknown>;
export interface DNode { id: string; kind: string; label: string; params: Params; parent: string | null; x: number; y: number; w: number; h: number }
export interface DEdge { id: string; source: string; target: string; kind: string; params: Params }
export interface Design { name: string; region: string; azs: number; nodes: DNode[]; edges: DEdge[]; text?: string }

export interface NodeResult { id: string; kind: string; title: string; status: "ok" | "warn" | "error"; messages: string[]; info: string[]; summary: string }
export interface EdgeResult { id: string; status: "ok" | "warn" | "error"; messages: string[]; label: string }
export interface Analysis {
  nodes: NodeResult[]; edges: EdgeResult[]; problems: string[]; ok: boolean; errors: number; warnings: number;
  counts: Record<string, number>; az_spread: number; az_names: string[];
}
export interface Loaded { design: Design; analysis: Analysis }
export interface TextError { line: number; message: string }

export interface ProjectInfo { name: string; path: string; draft: boolean }
export interface AppState {
  version: string; project: ProjectInfo | null; projects: { path: string; exists: boolean; name: string }[]; projects_dir: string;
  templates: { key: string; title: string; about: string }[]; regions: { id: string; name: string; azs: number }[];
  kinds: { kind: string; title: string; parent: string }[]; tiers: string[];
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
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};
