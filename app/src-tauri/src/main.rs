//! AWS Infra Builder desktop app (Tauri 2). The window (web/) calls these commands; the work is in aib-core. Nothing here talks to
//! AWS: the design computer designs (phase 0: canvas, checks and text only).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod aws;
mod term;
mod tofu;

use aib_core::checks::{self, Analysis};
use aib_core::creds::{self, Source};
use aib_core::dsl;
use aib_core::hcl;
use aib_core::hosts::{self, Host};
use aib_core::import;
use aib_core::model::{self, Design, ENDPOINT_SERVICES, INSTANCE_TYPES, KINDS, OS, PORTS, REGIONS, TIERS};
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
    jobs: tofu::Jobs,
    terms: term::Terms,
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
        "os": OS.iter().map(|o| json!({"key": o.0, "title": o.1, "arch": o.4, "min_disk": o.5, "note": o.6})).collect::<Vec<_>>(),
        "ports": PORTS.iter().map(|p| json!({"name": p.0, "port": p.1})).collect::<Vec<_>>(),
        "endpoint_services": ENDPOINT_SERVICES.iter().map(|e| json!({"service": e.0, "title": e.1, "type": e.2})).collect::<Vec<_>>(),
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

/// `sizes`: the canvas's measured block sizes (id -> [w, h]); the estimate stands in for blocks it does not name
#[tauri::command]
fn design_arrange(st: State<'_, App>, mut design: Design, sizes: Option<aib_core::layout::Sizes>) -> R<Loaded> {
    aib_core::layout::arrange_with(&mut design, &sizes.unwrap_or_default());
    save_design(st, design)
}

/// one fix by its id, or every safe one (`all_safe`); the layout runs again when blocks came or went
#[tauri::command]
fn design_fix(st: State<'_, App>, mut design: Design, id: Option<String>, all_safe: bool, sizes: Option<aib_core::layout::Sizes>) -> R<Value> {
    let blocks = design.nodes.len();
    let mut done = 0;
    if all_safe {
        done += aib_core::fix::apply_all_safe(&mut design);
    }
    if let Some(id) = id {
        let f = aib_core::fix::fixes(&design).into_iter().find(|f| f.id == id).ok_or_else(|| "The design changed since that fix was offered; look at the list again.".to_string())?;
        let ip = if f.needs_ip { Some(my_ip()?) } else { None };
        aib_core::fix::apply(&mut design, &f, ip.as_deref())?;
        done += 1;
    }
    if design.nodes.len() != blocks {
        aib_core::layout::arrange_with(&mut design, &sizes.unwrap_or_default());
    }
    let l = save_design(st, design)?;
    Ok(json!({"design": l.design, "analysis": l.analysis, "done": done}))
}

/// the owner's public IPv4 address, from AWS's own check service (free, no account needed)
fn my_ip() -> R<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(8))).build().into();
    let s = agent.get("https://checkip.amazonaws.com").call().map_err(|e| format!("Your public address could not be looked up ({e}). Add an Address range block with it by hand."))?
        .into_body().read_to_string().map_err(|e| e.to_string())?;
    Ok(s.trim().to_string())
}

/// carry out one of the analysis's recommendations (by its id), then lay the design out again
#[tauri::command]
fn design_tidy(st: State<'_, App>, mut design: Design, id: String, sizes: Option<aib_core::layout::Sizes>) -> R<Loaded> {
    let rec = aib_core::tidy::recommend(&design).into_iter().find(|r| r.id == id).ok_or_else(|| "The design changed since that recommendation was made; look at the list again.".to_string())?;
    aib_core::tidy::apply(&mut design, &rec);
    aib_core::layout::arrange_with(&mut design, &sizes.unwrap_or_default());
    save_design(st, design)
}

#[tauri::command]
fn design_add(st: State<'_, App>, mut design: Design, kind: String, parent: Option<String>, params: Option<Value>) -> R<Value> {
    let id = design.add(&kind, parent.as_deref());
    // the New-instance dialog's choices (type, os, hours) onto the new block
    if let (Some(id), Some(Value::Object(m))) = (&id, params) {
        if let Some(n) = design.node_mut(id) {
            for (k, v) in m {
                n.params.insert(k, v);
            }
        }
    }
    let l = loaded(design);
    project(&st)?.save_design(&l.design).map_err(e)?;
    Ok(json!({"design": l.design, "analysis": l.analysis, "id": id}))
}

/// the instance types that fit what was asked, cheapest first, priced at the hours a month
#[tauri::command]
fn instance_recommend(vcpu: u8, ram: f64, gpu: bool, hours: f64, region: Option<String>) -> Vec<aib_core::pricing::Candidate> {
    aib_core::pricing::recommend(vcpu, ram, gpu, &region.unwrap_or_else(|| "ap-south-1".into()), hours)
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

// ---------------------------------------------------------------- plan / apply with OpenTofu
fn project_meta(p: &Project) -> Value {
    store::read_json(&p.dir.join("project.json")).unwrap_or_else(|| json!({}))
}

fn save_project_meta(p: &Project, meta: &Value) -> R<()> {
    store::write_json(&p.dir.join("project.json"), meta).map_err(e)
}

#[tauri::command]
fn tofu_status(st: State<'_, App>) -> R<Value> {
    let path = tofu::locate();
    let p = st.project.lock().unwrap().clone();
    let meta = p.as_ref().map(project_meta).unwrap_or_else(|| json!({}));
    let src = match source() { Source::None => "none".to_string(), Source::Profile { name } => format!("profile {name}"), Source::Keys => "pasted keys".into() };
    Ok(json!({
        "tofu": path.as_ref().map(|p| p.to_string_lossy().to_string()),
        "version": path.as_ref().and_then(|p| tofu::version(p)),
        "install_version": tofu::TOFU_VERSION,
        "source": src,
        "identity": store::settings()["aws_identity"],
        "project_account": meta["aws"]["account"].as_str().filter(|a| !a.is_empty()),
        "budget": meta["budget"],
        "state": p.as_ref().map(|p| tofu::state_info(&p.dir)).unwrap_or_default(),
    }))
}

#[tauri::command]
fn tofu_install(st: State<'_, App>) -> R<()> {
    tofu::install(&st.jobs)
}

#[tauri::command]
fn job_status(st: State<'_, App>, since: usize) -> tofu::JobSnap {
    st.jobs.snapshot(since)
}

#[tauri::command]
fn project_set_budget(st: State<'_, App>, budget: Option<f64>) -> R<()> {
    let p = project(&st)?;
    let mut meta = project_meta(&p);
    match budget {
        Some(b) if b > 0.0 => meta["budget"] = json!(b),
        _ => { meta.as_object_mut().map(|m| m.remove("budget")); }
    }
    save_project_meta(&p, &meta)
}

/// the environment OpenTofu gets: a profile name, or the pasted keys (never logged, never written)
fn tofu_env() -> R<Vec<(String, String)>> {
    Ok(match creds_for_call()? {
        aws::Creds::Profile(name) => vec![("AWS_PROFILE".into(), name)],
        aws::Creds::Keys { id, secret, token } => {
            let mut v = vec![("AWS_ACCESS_KEY_ID".into(), id), ("AWS_SECRET_ACCESS_KEY".into(), secret)];
            if let Some(t) = token.filter(|t| !t.is_empty()) {
                v.push(("AWS_SESSION_TOKEN".into(), t));
            }
            v
        }
    })
}

/// the account guard: the credentials must point at the project's account (set by the first plan), and the identity is checked live
async fn guarded_ctx(st: &State<'_, App>) -> R<tofu::Ctx> {
    let p = project(st)?;
    let tofu_path = tofu::locate().ok_or("OpenTofu is not installed: use Install in the Plan tab, or `brew install opentofu`.")?;
    let design = p.design();
    let id = aws::identity(creds_for_call()?, &design.region).await?;
    store::save_setting("aws_identity", json!({"account": id.account, "arn": id.arn, "user_id": id.user_id, "region": design.region, "checked": chrono_now()})).map_err(e)?;
    let mut meta = project_meta(&p);
    match meta["aws"]["account"].as_str().filter(|a| !a.is_empty()) {
        Some(a) if a != id.account => return Err(format!("Account guard: this project belongs to account {a}, but the credentials are for {}. Nothing was run.", id.account)),
        Some(_) => {}
        None => {
            meta["aws"]["account"] = json!(id.account);
            meta["aws"]["region"] = json!(design.region);
            save_project_meta(&p, &meta)?;
        }
    }
    if meta["aws"]["region"].as_str().map(|r| r != design.region).unwrap_or(false) {
        return Err(format!("This project's resources are in {}, but the design now says {}. Change the region back, or make a new project for the other region.", meta["aws"]["region"].as_str().unwrap_or("?"), design.region));
    }
    Ok(tofu::Ctx { tofu: tofu_path, project: p.dir.clone(), project_name: p.name(), design, env: tofu_env()? })
}

/// before a plan of an imported project: the facts an adopt needs (images, real names, rule ids, implicit attachments) are
/// read again from AWS (Describe* only) and filled into the design's blocks; the owner's drawing is not changed otherwise
async fn refresh_facts(p: &Project, design: &mut Design) -> R<Vec<String>> {
    let meta = project_meta(p);
    if !meta["aws"]["imported"].as_bool().unwrap_or(false) {
        return Ok(vec![]);
    }
    let (Some(vpc_id), Some(region), Some(account)) = (meta["aws"]["vpc_id"].as_str(), meta["aws"]["region"].as_str(), meta["aws"]["account"].as_str()) else { return Ok(vec![]) };
    let inv = aws::inventory(creds_for_call()?, region, vpc_id, account).await?;
    let fresh = import::to_design(&inv).design;
    let notes = import::refresh_facts(design, &fresh);
    if !notes.is_empty() {
        p.save_design(design).map_err(e)?;
    }
    Ok(notes)
}

#[tauri::command]
async fn tofu_plan(st: State<'_, App>, destroy: bool) -> R<Vec<String>> {
    if st.jobs.running() {
        return Err("A job is still running.".into());
    }
    let mut cx = guarded_ctx(&st).await?;
    let notes = refresh_facts(&project(&st)?, &mut cx.design).await?;
    if !destroy && !checks::analyse(&cx.design).ok {
        return Err("The design has errors; fix them before planning.".into());
    }
    tofu::plan(&st.jobs, cx, destroy)?;
    Ok(notes)
}

/// apply the shown plan; `confirm` must be the project's name when anything is destroyed or replaced; `accept_budget` when the
/// estimate is above the project's budget
#[tauri::command]
async fn tofu_apply(st: State<'_, App>, confirm: String, accept_budget: bool) -> R<()> {
    if st.jobs.running() {
        return Err("A job is still running.".into());
    }
    let snap = st.jobs.snapshot(0);
    let Some(plan) = snap.plan.clone().filter(|_| snap.state == "done" && (snap.kind == "plan" || snap.kind == "plan-destroy")) else {
        return Err("Run Plan first: apply only runs the plan you have seen.".into());
    };
    let cx = guarded_ctx(&st).await?;
    let info = tofu::state_info(&cx.project);
    if !info.plan_fresh {
        return Err("The design changed since the plan: run Plan again.".into());
    }
    let destroy = snap.kind == "plan-destroy";
    let dup = aib_core::plan::duplicates(&cx.design, &plan);
    if !destroy && !dup.is_empty() {
        return Err(format!("Not applied: this plan would create a second copy of {} (they already exist in AWS; the design carries their ids), and you would pay twice. Run Plan again: the import blocks adopt them instead. If a block was replaced, see why in the plan table.", dup.join(", ")));
    }
    if (plan.destroys > 0 || plan.replaces > 0) && confirm.trim() != cx.project_name {
        return Err(format!("This would destroy {} and replace {} resources: type the project's name ({}) to allow it.", plan.destroys, plan.replaces, cx.project_name));
    }
    let meta = project_meta(&project(&st)?);
    if let Some(b) = meta["budget"].as_f64() {
        let est = checks::analyse(&cx.design).cost.total;
        if !destroy && est > b && !accept_budget {
            return Err(format!("The estimate (USD {est:.2} a month) is above this project's budget (USD {b:.2}). Tick the budget box to apply anyway, or change the design."));
        }
    }
    tofu::apply(&st.jobs, cx, destroy)
}

/// after an apply: the AWS ids of the created resources go into the design (the blocks show them)
#[tauri::command]
fn design_set_aws_ids(st: State<'_, App>, ids: std::collections::HashMap<String, String>, clear: bool, outputs: Option<Vec<(String, String)>>) -> R<Loaded> {
    let p = project(&st)?;
    let mut d = p.design();
    let outputs = outputs.unwrap_or_default();
    for n in d.nodes.iter_mut() {
        if clear {
            n.params.remove("aws_id");
            n.params.remove("address");
            n.params.remove("private_ip");
        }
        if let Some(id) = ids.get(&n.id) {
            n.params.insert("aws_id".into(), json!(id));
        }
        // output "instance_<label>" = {id, private_ip, public_ip}: the addresses for the Terminal tab
        if n.kind == "ec2" {
            if let Some((_, v)) = outputs.iter().find(|(k, _)| *k == format!("instance_{}", n.label)) {
                if let Ok(o) = serde_json::from_str::<Value>(v) {
                    if let Some(ip) = o["public_ip"].as_str().filter(|s| !s.is_empty()) {
                        n.params.insert("address".into(), json!(ip));
                    }
                    if let Some(ip) = o["private_ip"].as_str().filter(|s| !s.is_empty()) {
                        n.params.insert("private_ip".into(), json!(ip));
                    }
                }
            }
        }
    }
    let l = loaded(d);
    p.save_design(&l.design).map_err(e)?;
    Ok(l)
}

// ---------------------------------------------------------------- terminal (the Termius part)
#[tauri::command]
fn hosts_state(design: Option<Design>) -> Value {
    let suggested = design.map(|d| hosts::suggest(&d)).unwrap_or_default();
    json!({"saved": hosts::saved(), "suggested": suggested, "ssh": which("ssh")})
}

fn which(program: &str) -> Option<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    let found = path.split(':').chain(["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"]).map(|d| PathBuf::from(d).join(program)).find(|p| p.is_file());
    found.map(|p| p.to_string_lossy().into())
}

#[tauri::command]
fn hosts_save(hosts: Vec<Host>) -> R<Vec<Host>> {
    hosts::save(&hosts).map_err(e)?;
    Ok(hosts::saved())
}

/// what is wrong with the key file, if anything
#[tauri::command]
fn key_check(path: String) -> Option<String> {
    term::key_problem(&path)
}

#[tauri::command]
fn key_fix(path: String) -> R<Option<String>> {
    term::fix_key_permissions(&path)?;
    Ok(term::key_problem(&path))
}

#[tauri::command]
async fn pick_file(app: tauri::AppHandle) -> R<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_file(move |f| {
        let _ = tx.send(f.map(|p| p.to_string()));
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(e)).await.map_err(e)?
}

/// start ssh to the host in a new terminal; the window then listens for term-data / term-exit events with this id
#[tauri::command]
fn term_open(app: tauri::AppHandle, st: State<'_, App>, id: String, host: Host, cols: u16, rows: u16) -> R<Vec<String>> {
    let problem = hosts::check(&host);
    if !problem.is_empty() {
        return Err(problem);
    }
    if let Some(p) = term::key_problem(&host.key) {
        return Err(p);
    }
    let ssh = which("ssh").ok_or("ssh was not found. On macOS it comes with the system (/usr/bin/ssh); on Linux install openssh-client.")?;
    let key = if host.key.trim().is_empty() { None } else { Some(term::expand_home(&host.key)) };
    let args = hosts::ssh_args(&host, key.as_deref());
    st.terms.open(app, &id, &ssh, &args, cols, rows)?;
    Ok(args)
}

#[tauri::command]
fn term_write(st: State<'_, App>, id: String, data: String) -> R<()> {
    st.terms.write(&id, &data)
}

#[tauri::command]
fn term_resize(st: State<'_, App>, id: String, cols: u16, rows: u16) -> R<()> {
    st.terms.resize(&id, cols, rows)
}

#[tauri::command]
fn term_close(st: State<'_, App>, id: String) {
    st.terms.close(&id)
}

#[tauri::command]
fn term_list(st: State<'_, App>) -> Vec<String> {
    st.terms.open_ids()
}

/// the state of the design's instances in AWS (read-only)
#[tauri::command]
async fn aws_instance_state(region: Option<String>, ids: Vec<String>) -> R<Vec<aws::InstanceState>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let creds = creds_for_call()?;
    let region = region_for_call(region);
    aws::instance_states(creds, &region, ids).await
}

/// start or stop one instance (the inspector's buttons; starting costs money and the window says so first)
#[tauri::command]
async fn aws_instance_power(region: Option<String>, id: String, on: bool) -> R<Vec<aws::InstanceState>> {
    let creds = creds_for_call()?;
    let region = region_for_call(region);
    aws::set_instance_power(creds.clone(), &region, &id, on).await?;
    aws::instance_states(creds, &region, vec![id]).await
}

/// the key files in ~/.ssh, by name only
#[tauri::command]
fn keys_list() -> Value {
    let dir = hosts::ssh_dir();
    json!({"dir": dir.to_string_lossy(), "keys": hosts::list_keys(&dir)})
}

/// move a key file into ~/.ssh with mode 600; returns its new path
#[tauri::command]
fn key_move(path: String) -> R<String> {
    term::move_key_into(&path, &hosts::ssh_dir())
}

/// the account's key pairs in the region, each with whether ~/.ssh/<name>.pem is here (read-only)
#[tauri::command]
async fn aws_key_pairs(region: Option<String>) -> R<Value> {
    let creds = creds_for_call()?;
    let region = region_for_call(region);
    let pairs = aws::key_pairs(creds, &region).await?;
    let dir = hosts::ssh_dir();
    Ok(json!({"region": region, "pairs": pairs.iter().map(|k| {
        let file = dir.join(format!("{}.pem", k.name));
        json!({"name": k.name, "kind": k.kind, "fingerprint": k.fingerprint, "file": file.is_file().then(|| file.to_string_lossy().into_owned())})
    }).collect::<Vec<_>>()}))
}

/// a new key pair in AWS (free), its private key written once to ~/.ssh/<name>.pem with mode 600
#[tauri::command]
async fn aws_create_key_pair(name: String, region: Option<String>) -> R<Value> {
    let name = name.trim().to_string();
    let problem = hosts::key_pair_name_problem(&name);
    if !problem.is_empty() {
        return Err(problem);
    }
    let dir = hosts::ssh_dir();
    if dir.join(format!("{name}.pem")).exists() {
        return Err(format!("{}/{name}.pem already exists. Use it, or choose another name.", dir.display()));
    }
    let creds = creds_for_call()?;
    let region = region_for_call(region);
    let material = aws::create_key_pair(creds, &region, &name).await?;
    let path = term::write_new_key(&dir, &name, &material)?;
    Ok(json!({"name": name, "path": path, "region": region}))
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
        &item("tab-plan", "Plan & Apply", Some("CmdOrCtrl+4"))?,
        &item("tab-costs", "Costs", Some("CmdOrCtrl+7"))?,
        &item("tab-terminal", "Terminal", Some("CmdOrCtrl+8"))?,
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
        &item("add-vpce", "Add VPC endpoint", None)?,
        &item("add-s3", "Add S3 bucket", None)?,
        &item("add-lambda", "Add Lambda function", None)?,
        &item("add-alb", "Add Load balancer", None)?,
        &item("add-asg", "Add Auto Scaling group", None)?,
    ])?;
    menu.append_items(&[&file, &edit, &view, &design])?;
    Ok(menu)
}

fn on_menu(app: &tauri::AppHandle, id: &str) {
    match id {
        "tab-design" | "tab-text" | "tab-code" | "tab-plan" | "tab-costs" | "tab-terminal" | "tab-account" | "tab-connection" | "tab-project" => { let _ = app.emit("open-tab", id.trim_start_matches("tab-")); }
        other => { let _ = app.emit("menu", other); }
    }
}

fn main() {
    // an unsaved draft does not outlive the app (also after a crash)
    let _ = store::discard_draft();
    let app = App { project: Mutex::new(store::last_project()), jobs: tofu::Jobs::default(), terms: term::Terms::default() };
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
            project_load, save_design, design_from_text, design_arrange, design_tidy, design_fix, design_add, instance_recommend, design_connect, design_remove, text_help, code, export_code, open_url,
            aws_state, aws_set_keys, aws_clear_keys, aws_select, aws_test, aws_list_vpcs, aws_import,
            tofu_status, tofu_install, tofu_plan, tofu_apply, job_status, project_set_budget, design_set_aws_ids,
            hosts_state, hosts_save, key_check, key_fix, pick_file, term_open, term_write, term_resize, term_close, term_list,
            keys_list, key_move, aws_key_pairs, aws_create_key_pair, aws_instance_state, aws_instance_power
        ])
        .build(tauri::generate_context!())
        .expect("error while running AWS Infra Builder")
        .run(|_, ev| {
            if let tauri::RunEvent::Exit = ev {
                let _ = store::discard_draft();
            }
        });
}
