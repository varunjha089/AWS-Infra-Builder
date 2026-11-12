//! AWS Infra Builder desktop app (Tauri 2). The window (web/) calls these commands; the work is in aib-core. Nothing here talks to
//! AWS: the design computer designs (phase 0: canvas, checks and text only).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod aws;

use aib_core::checks::{self, Analysis};
use aib_core::creds::{self, Source};
use aib_core::dsl;
use aib_core::hcl;
use aib_core::import;
use aib_core::model::{self, Design, INSTANCE_TYPES, KINDS, OS, PORTS, REGIONS, TIERS};
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

const SERVICE: &str = "AWS Infra Builder";

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
        "instance_types": INSTANCE_TYPES.iter().map(|i| json!({"type": i.0, "vcpu": i.1, "memory": i.2, "arch": i.3, "us_east_1": i.4, "ap_south_1": i.5})).collect::<Vec<_>>(),
        "os": OS.iter().map(|o| json!({"key": o.0, "title": o.1})).collect::<Vec<_>>(),
        "ports": PORTS.iter().map(|p| json!({"name": p.0, "port": p.1})).collect::<Vec<_>>(),
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

/// the design as HCL files (name -> text), generated afresh from the design the window holds
#[tauri::command]
fn code(design: Design) -> hcl::Files {
    hcl::generate(&design)
}

/// the project's current design as HCL into a folder the owner picks
#[tauri::command]
async fn export_code(app: tauri::AppHandle, st: State<'_, App>) -> R<String> {
    let d = project(&st)?.design();
    let Some(dir) = pick_folder(app).await? else { return Err("No folder chosen.".into()) };
    let dir = PathBuf::from(dir);
    hcl::write_to(&hcl::generate(&d), &dir).map_err(e)?;
    Ok(dir.to_string_lossy().to_string())
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

// ---------------------------------------------------------------- AWS credentials and read-only calls
/// each keychain item is read once per launch and kept in memory: every read of an unsigned (or rebuilt) app can make macOS ask for
/// the login password again
static SECRETS: Mutex<Option<std::collections::HashMap<String, Option<String>>>> = Mutex::new(None);

fn secret(name: &str) -> Option<String> {
    let mut cache = SECRETS.lock().unwrap_or_else(|p| p.into_inner());
    let map = cache.get_or_insert_with(Default::default);
    if let Some(v) = map.get(name) {
        return v.clone();
    }
    let v = keyring::Entry::new(SERVICE, name).ok().and_then(|en| en.get_password().ok()).filter(|s| !s.is_empty());
    map.insert(name.to_string(), v.clone());
    v
}

fn remember_secret(name: &str, value: Option<String>) {
    SECRETS.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Default::default).insert(name.to_string(), value);
}

fn set_secret(name: &str, value: &str) -> R<()> {
    let en = keyring::Entry::new(SERVICE, name).map_err(e)?;
    if value.is_empty() {
        let _ = en.delete_credential();
        remember_secret(name, None);
    } else {
        en.set_password(value).map_err(e)?;
        remember_secret(name, Some(value.to_string()));
    }
    Ok(())
}

/// what the window may know about the pasted keys: whether they exist and the first characters of the id (kept in settings.json so
/// that starting the app never touches the keychain)
fn keys_meta() -> Value {
    store::settings()["aws_keys"].clone()
}

fn source() -> Source {
    serde_json::from_value(store::settings()["aws_source"].clone()).unwrap_or_default()
}

/// the credentials for one call, from the chosen source
fn creds_for_call() -> R<aws::Creds> {
    match source() {
        Source::None => Err("Choose a profile or paste keys in the Connection tab first.".into()),
        Source::Profile { name } => Ok(aws::Creds::Profile(name)),
        Source::Keys => {
            let id = secret("aws_key_id").ok_or("No pasted keys: paste them in the Connection tab, or choose a profile.")?;
            let secret_key = secret("aws_key_secret").ok_or("The secret access key is missing from the keychain: paste the keys again.")?;
            Ok(aws::Creds::Keys { id, secret: secret_key, token: secret("aws_key_token") })
        }
    }
}

fn region_for_call(region: Option<String>) -> String {
    region.filter(|r| !r.trim().is_empty()).or_else(|| store::settings()["aws_region"].as_str().map(String::from)).unwrap_or_else(|| "ap-south-1".into())
}

#[tauri::command]
fn aws_state() -> Value {
    let s = store::settings();
    json!({
        "profiles": creds::profiles(),
        "aws_dir": creds::aws_dir().to_string_lossy(),
        "source": source(),
        "keys": keys_meta(),
        "region": s["aws_region"],
        "identity": s["aws_identity"],
        "policy": creds::read_only_policy(),
    })
}

#[tauri::command]
fn aws_set_keys(id: String, secret_key: String, token: String) -> R<Value> {
    creds::check_keys(&id, &secret_key, &token)?;
    set_secret("aws_key_id", id.trim())?;
    set_secret("aws_key_secret", secret_key.trim())?;
    set_secret("aws_key_token", token.trim())?;
    let meta = json!({"hint": format!("{}…", id.trim().chars().take(4).collect::<String>()), "temporary": id.trim().starts_with("ASIA")});
    store::save_setting("aws_keys", meta.clone()).map_err(e)?;
    store::save_setting("aws_source", serde_json::to_value(Source::Keys).map_err(e)?).map_err(e)?;
    store::save_setting("aws_identity", Value::Null).map_err(e)?;
    Ok(meta)
}

#[tauri::command]
fn aws_clear_keys() -> R<()> {
    for n in ["aws_key_id", "aws_key_secret", "aws_key_token"] {
        set_secret(n, "")?;
    }
    store::save_setting("aws_keys", Value::Null).map_err(e)?;
    if matches!(source(), Source::Keys) {
        store::save_setting("aws_source", serde_json::to_value(Source::None).map_err(e)?).map_err(e)?;
        store::save_setting("aws_identity", Value::Null).map_err(e)?;
    }
    Ok(())
}

#[tauri::command]
fn aws_select(source: Source, region: Option<String>) -> R<()> {
    store::save_setting("aws_source", serde_json::to_value(source).map_err(e)?).map_err(e)?;
    if let Some(r) = region {
        store::save_setting("aws_region", json!(r)).map_err(e)?;
    }
    store::save_setting("aws_identity", Value::Null).map_err(e)
}

/// sts:GetCallerIdentity with the chosen credentials: the account and who the key belongs to (remembered for the Account tab)
#[tauri::command]
async fn aws_test(region: Option<String>) -> R<Value> {
    let creds = creds_for_call()?;
    let region = region_for_call(region);
    let id = aws::identity(creds, &region).await?;
    let v = json!({"account": id.account, "arn": id.arn, "user_id": id.user_id, "region": region, "checked": chrono_now()});
    store::save_setting("aws_identity", v.clone()).map_err(e)?;
    Ok(v)
}

/// seconds since 1970, for "checked a moment ago" in the window
fn chrono_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[tauri::command]
async fn aws_list_vpcs(region: Option<String>) -> R<Value> {
    let creds = creds_for_call()?;
    let region = region_for_call(region);
    let vpcs = aws::list_vpcs(creds, &region).await?;
    Ok(json!({"region": region, "vpcs": vpcs}))
}

/// one VPC of the account as a new unsaved project (read-only: Describe* calls only)
#[tauri::command]
async fn aws_import(st: State<'_, App>, region: Option<String>, vpc_id: String) -> R<Value> {
    let creds = creds_for_call()?;
    let region = region_for_call(region);
    let account = store::settings()["aws_identity"]["account"].as_str().unwrap_or("").to_string();
    let inv = aws::inventory(creds, &region, &vpc_id, &account).await?;
    let im = import::to_design(&inv);
    let name = im.design.name.clone();
    let p = store::create_draft(&name, im.design).map_err(e)?;
    // the project remembers where it came from (the account guard of later phases)
    let mut meta: Value = store::read_json(&p.dir.join("project.json")).unwrap_or_else(|| json!({}));
    meta["aws"] = json!({"account": account, "region": region, "vpc_id": vpc_id, "imported": true});
    store::write_json(&p.dir.join("project.json"), &meta).map_err(e)?;
    *st.project.lock().unwrap() = Some(p.clone());
    Ok(json!({"project": project_json(&p), "notes": im.notes}))
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
        &tauri::menu::PredefinedMenuItem::separator(app)?,
        &item("export-code", "Export Code (HCL)…", Some("Shift+CmdOrCtrl+E"))?,
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
        &item("tab-code", "Code", Some("CmdOrCtrl+3"))?,
        &item("tab-costs", "Costs", Some("CmdOrCtrl+4"))?,
        &item("tab-account", "Account", Some("CmdOrCtrl+5"))?,
        &item("tab-connection", "Connection", Some("CmdOrCtrl+6"))?,
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
        &item("add-sg", "Add Security group", None)?,
        &item("add-ec2", "Add EC2 instance", None)?,
        &item("add-cidr", "Add Address range", None)?,
    ])?;
    menu.append_items(&[&file, &edit, &view, &design])?;
    Ok(menu)
}

fn on_menu(app: &tauri::AppHandle, id: &str) {
    match id {
        "tab-design" | "tab-text" | "tab-code" | "tab-costs" | "tab-account" | "tab-connection" | "tab-project" => { let _ = app.emit("open-tab", id.trim_start_matches("tab-")); }
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
            project_load, save_design, design_from_text, design_arrange, design_add, design_connect, design_remove, text_help, code, export_code, open_url,
            aws_state, aws_set_keys, aws_clear_keys, aws_select, aws_test, aws_list_vpcs, aws_import
        ])
        .build(tauri::generate_context!())
        .expect("error while running AWS Infra Builder")
        .run(|_, ev| {
            if let tauri::RunEvent::Exit = ev {
                let _ = store::discard_draft();
            }
        });
}
