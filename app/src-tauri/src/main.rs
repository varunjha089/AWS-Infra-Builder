//! AWS Infra Builder desktop app (Tauri 2). The window (web/) calls these commands; the work is in aib-core. Nothing here talks to
//! AWS: the design computer designs (phase 0: canvas, checks and text only).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aib_core::checks::{self, Analysis};
use aib_core::dsl;
use aib_core::model::{self, Design, KINDS, REGIONS, TIERS};
use aib_core::store::{self, Project};
use aib_core::templates;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::{Emitter, State};
use tauri_plugin_dialog::DialogExt;

struct App {
    project: Mutex<Option<Project>>,
}

type R<T> = Result<T, String>;

fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

fn project(st: &State<'_, App>) -> R<Project> {
    st.project.lock().unwrap().clone().ok_or_else(|| "Open or create a project first.".to_string())
}

#[derive(Serialize)]
struct Loaded {
    design: Design,
    analysis: Analysis,
}

fn loaded(mut d: Design) -> Loaded {
    dsl::sync(&mut d);
    let analysis = checks::analyse(&d);
    Loaded { design: d, analysis }
}

fn project_json(p: &Project) -> Value {
    json!({"name": p.name(), "path": p.dir.to_string_lossy(), "draft": p.is_draft()})
}

// ---------------------------------------------------------------- state and projects
#[tauri::command]
fn app_state(st: State<'_, App>) -> Value {
    let p = st.project.lock().unwrap().clone();
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "project": p.map(|p| project_json(&p)),
        "projects": store::listing(),
        "projects_dir": store::default_projects_dir().to_string_lossy(),
        "templates": templates::TEMPLATES.iter().map(|t| json!({"key": t.key, "title": t.title, "about": t.about})).collect::<Vec<_>>(),
        "regions": REGIONS.iter().map(|r| json!({"id": r.0, "name": r.1, "azs": r.2})).collect::<Vec<_>>(),
        "kinds": KINDS.iter().map(|k| json!({"kind": k.0, "title": k.1, "parent": k.2})).collect::<Vec<_>>(),
        "tiers": TIERS,
    })
}

#[tauri::command]
fn project_new(st: State<'_, App>, name: String, location: Option<String>) -> R<Value> {
    let p = store::create(&name, location.as_deref().filter(|s| !s.trim().is_empty()).map(std::path::Path::new)).map_err(e)?;
    *st.project.lock().unwrap() = Some(p.clone());
    Ok(project_json(&p))
}

#[tauri::command]
fn project_open(st: State<'_, App>, path: String) -> R<Value> {
    let p = store::open(&PathBuf::from(path)).map_err(e)?;
    *st.project.lock().unwrap() = Some(p.clone());
    Ok(project_json(&p))
}

#[tauri::command]
fn project_from_template(st: State<'_, App>, key: String) -> R<Value> {
    let d = templates::build(&key).map_err(e)?;
    let name = d.name.clone();
    let p = store::create_draft(&name, d).map_err(e)?;
    *st.project.lock().unwrap() = Some(p.clone());
    Ok(project_json(&p))
}

#[tauri::command]
fn project_save_as(st: State<'_, App>, name: String, location: Option<String>) -> R<Value> {
    let draft = project(&st)?;
    let p = store::save_as(&draft, &name, location.as_deref().filter(|s| !s.trim().is_empty()).map(std::path::Path::new)).map_err(e)?;
    // the design carries the project's name
    let mut d = p.design();
    if d.name.trim().is_empty() || d.name == draft.name() {
        d.name = name.trim().to_string();
        dsl::sync(&mut d);
        p.save_design(&d).map_err(e)?;
    }
    *st.project.lock().unwrap() = Some(p.clone());
    Ok(project_json(&p))
}

#[tauri::command]
fn project_close(st: State<'_, App>) -> R<()> {
    let was_draft = st.project.lock().unwrap().as_ref().map(|p| p.is_draft()).unwrap_or(false);
    *st.project.lock().unwrap() = None;
    if was_draft {
        store::discard_draft().map_err(e)?;
    }
    store::save_setting("last_project", Value::Null).map_err(e)
}

#[tauri::command]
fn project_forget(st: State<'_, App>, path: String) -> R<()> {
    let p = PathBuf::from(&path);
    if st.project.lock().unwrap().as_ref().map(|c| c.dir == p).unwrap_or(false) {
        *st.project.lock().unwrap() = None;
    }
    store::forget(&p).map_err(e)
}

#[tauri::command]
fn set_projects_dir(path: String) -> R<()> {
    let p = store::project_path(&path);
    if !p.is_dir() {
        return Err(format!("{} is not a folder.", p.display()));
    }
    store::save_setting("projects_dir", json!(p.to_string_lossy())).map_err(e)
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> R<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |f| {
        let _ = tx.send(f.map(|p| p.to_string()));
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(e)).await.map_err(e)?
}

// ---------------------------------------------------------------- the design
#[tauri::command]
fn project_load(st: State<'_, App>) -> R<Loaded> {
    Ok(loaded(project(&st)?.design()))
}

/// the window's design, written to the project; the text is printed again and the checks run
#[tauri::command]
fn save_design(st: State<'_, App>, design: Design) -> R<Loaded> {
    let l = loaded(design);
    project(&st)?.save_design(&l.design).map_err(e)?;
    Ok(l)
}

#[tauri::command]
fn design_from_text(st: State<'_, App>, text: String) -> R<Value> {
    let base = project(&st)?.design();
    match dsl::parse(&text, Some(&base)) {
        Ok(d) => {
            let analysis = checks::analyse(&d);
            project(&st)?.save_design(&d).map_err(e)?;
            Ok(json!({"ok": true, "design": d, "analysis": analysis, "errors": []}))
        }
        Err(errors) => Ok(json!({"ok": false, "errors": errors})),
    }
}

#[tauri::command]
fn design_arrange(st: State<'_, App>, mut design: Design) -> R<Loaded> {
    model::arrange(&mut design);
    save_design(st, design)
}

#[tauri::command]
fn design_add(st: State<'_, App>, mut design: Design, kind: String, parent: Option<String>) -> R<Value> {
    let id = design.add(&kind, parent.as_deref());
    let l = loaded(design);
    project(&st)?.save_design(&l.design).map_err(e)?;
    Ok(json!({"design": l.design, "analysis": l.analysis, "id": id}))
}

#[tauri::command]
fn design_connect(st: State<'_, App>, mut design: Design, source: String, target: String) -> R<Value> {
    let (sk, tk) = (design.node(&source).map(|n| n.kind.clone()).unwrap_or_default(), design.node(&target).map(|n| n.kind.clone()).unwrap_or_default());
    let id = design.connect(&source, &target);
    if id.is_none() && model::edge_kind(&sk, &tk).is_none() {
        return Err(format!("No arrow from a {} to a {}: subnets attach to route tables, route tables route to gateways.", model::title(&sk), model::title(&tk)));
    }
    let l = loaded(design);
    project(&st)?.save_design(&l.design).map_err(e)?;
    Ok(json!({"design": l.design, "analysis": l.analysis, "id": id}))
}

#[tauri::command]
fn design_remove(st: State<'_, App>, mut design: Design, nodes: Vec<String>, edges: Vec<String>) -> R<Loaded> {
    design.remove(&nodes);
    design.edges.retain(|e| !edges.contains(&e.id));
    save_design(st, design)
}

#[tauri::command]
fn text_help() -> Vec<(String, String)> {
    dsl::help()
}

#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> R<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("not a web address".into());
    }
    let _ = app;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(&url).spawn().map_err(e)?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(&url).spawn().map_err(e)?;
    #[cfg(windows)]
    std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn().map_err(e)?;
    Ok(())
}

// ---------------------------------------------------------------- menu
fn build_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, text: &str, accel: Option<&str>| MenuItem::with_id(app, id, text, true, accel);
    let menu = Menu::new(app)?;
    #[cfg(target_os = "macos")]
    {
        use tauri::menu::PredefinedMenuItem;
        let about = Submenu::with_items(app, "AWS Infra Builder", true, &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ])?;
        menu.append(&about)?;
    }
    let file = Submenu::with_items(app, "File", true, &[
        &item("new-project", "New Project…", Some("CmdOrCtrl+N"))?,
        &item("open-project", "Open Project…", Some("CmdOrCtrl+O"))?,
        &item("save-project", "Save Project…", Some("CmdOrCtrl+S"))?,
        &item("close-project", "Close Project", Some("CmdOrCtrl+W"))?,
    ])?;
    let edit = Submenu::with_items(app, "Edit", true, &[
        &item("undo", "Undo", Some("CmdOrCtrl+Z"))?,
        &item("redo", "Redo", Some("Shift+CmdOrCtrl+Z"))?,
        &tauri::menu::PredefinedMenuItem::separator(app)?,
        &tauri::menu::PredefinedMenuItem::cut(app, None)?,
        &tauri::menu::PredefinedMenuItem::copy(app, None)?,
        &tauri::menu::PredefinedMenuItem::paste(app, None)?,
        &tauri::menu::PredefinedMenuItem::select_all(app, None)?,
    ])?;
    let view = Submenu::with_items(app, "View", true, &[
        &item("tab-design", "Design", Some("CmdOrCtrl+1"))?,
        &item("tab-text", "Text", Some("CmdOrCtrl+2"))?,
        &item("tab-project", "Projects", Some("CmdOrCtrl+0"))?,
        &tauri::menu::PredefinedMenuItem::separator(app)?,
        &item("night", "Night Mode", Some("Shift+CmdOrCtrl+N"))?,
    ])?;
    let design = Submenu::with_items(app, "Design", true, &[
        &item("arrange", "Arrange in AZ × tier grid", Some("Shift+CmdOrCtrl+A"))?,
        &item("fit", "Fit to window", Some("Shift+CmdOrCtrl+F"))?,
        &tauri::menu::PredefinedMenuItem::separator(app)?,
        &item("add-vpc", "Add VPC", None)?,
        &item("add-subnet", "Add Subnet", None)?,
        &item("add-igw", "Add Internet Gateway", None)?,
        &item("add-nat", "Add NAT Gateway", None)?,
        &item("add-rt", "Add Route table", None)?,
    ])?;
    menu.append_items(&[&file, &edit, &view, &design])?;
    Ok(menu)
}

fn on_menu(app: &tauri::AppHandle, id: &str) {
    match id {
        "tab-design" | "tab-text" | "tab-project" => { let _ = app.emit("open-tab", id.trim_start_matches("tab-")); }
        other => { let _ = app.emit("menu", other); }
    }
}

fn main() {
    // an unsaved draft does not outlive the app (also after a crash)
    let _ = store::discard_draft();
    let app = App { project: Mutex::new(store::last_project()) };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app)
        .setup(|app| {
            let menu = build_menu(app.handle())?;
            app.set_menu(menu)?;
            app.on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_state, project_new, project_open, project_from_template, project_save_as, project_close, project_forget, set_projects_dir, pick_folder,
            project_load, save_design, design_from_text, design_arrange, design_add, design_connect, design_remove, text_help, open_url
        ])
        .build(tauri::generate_context!())
        .expect("error while running AWS Infra Builder")
        .run(|_, ev| {
            if let tauri::RunEvent::Exit = ev {
                let _ = store::discard_draft();
            }
        });
}
