//! OpenTofu: find or install the binary, and run init / plan / apply / destroy for a project in a background job whose log the
//! window follows. The working folder is <project>/tofu (the generated .tf files, .terraform, terraform.tfstate, plan.tfplan);
//! the state is backed up to <project>/tofu/backups before every apply. Credentials reach the process only through its environment.

use aib_core::model::Design;
use aib_core::plan::{self, PlanSummary};
use aib_core::{hcl, store};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// the OpenTofu release the app installs when none is found (MPL-2.0)
pub const TOFU_VERSION: &str = "1.13.9";

// ---------------------------------------------------------------- the one background job
#[derive(Debug, Clone, Serialize, Default)]
pub struct JobSnap {
    /// "idle" | "running" | "done" | "failed"
    pub state: String,
    /// "install" | "plan" | "plan-destroy" | "apply" | "destroy"
    pub kind: String,
    pub log: Vec<String>,
    /// the index to ask for next time (the log is sent in pieces)
    pub next: usize,
    pub message: String,
    pub plan: Option<PlanSummary>,
    pub outputs: Vec<(String, String)>,
    /// block id -> "creating" | "created" | "failed" | "destroyed" | "changing" | "changed"
    pub marks: HashMap<String, String>,
    /// block id -> the AWS id of its main resource, once created
    pub aws_ids: HashMap<String, String>,
    pub started: u64,
}

#[derive(Default)]
pub struct Jobs {
    cur: Arc<Mutex<JobSnap>>,
}

impl Jobs {
    pub fn snapshot(&self, since: usize) -> JobSnap {
        let j = self.cur.lock().unwrap_or_else(|p| p.into_inner());
        let mut s = j.clone();
        s.log = j.log.iter().skip(since).cloned().collect();
        s.next = j.log.len();
        s
    }
    pub fn running(&self) -> bool {
        self.cur.lock().unwrap_or_else(|p| p.into_inner()).state == "running"
    }
    /// start a job; refused while one runs
    fn start(&self, kind: &str) -> Result<Log, String> {
        let mut j = self.cur.lock().unwrap_or_else(|p| p.into_inner());
        if j.state == "running" {
            return Err(format!("A {} is still running; wait for it to finish.", j.kind));
        }
        *j = JobSnap { state: "running".into(), kind: kind.into(), started: now(), ..Default::default() };
        Ok(Log { cur: self.cur.clone() })
    }
}

/// what a job writes while it runs
#[derive(Clone)]
pub struct Log {
    cur: Arc<Mutex<JobSnap>>,
}

impl Log {
    pub fn line(&self, s: impl Into<String>) {
        let s = s.into();
        for l in s.lines() {
            self.cur.lock().unwrap_or_else(|p| p.into_inner()).log.push(l.to_string());
        }
    }
    fn with(&self, f: impl FnOnce(&mut JobSnap)) {
        f(&mut self.cur.lock().unwrap_or_else(|p| p.into_inner()));
    }
    fn finish(&self, result: Result<String, String>) {
        self.with(|j| match result {
            Ok(m) => { j.state = "done".into(); j.message = m; }
            Err(m) => { j.state = "failed".into(); j.log.push(format!("FAILED: {m}")); j.message = m; }
        });
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------- the binary
/// the app's own copy: <settings>/bin/tofu
pub fn own_path() -> PathBuf {
    store::home().join("bin").join(if cfg!(windows) { "tofu.exe" } else { "tofu" })
}

/// where OpenTofu is: the owner's choice (settings tofu_path), the app's own copy, then PATH and the usual places
pub fn locate() -> Option<PathBuf> {
    if let Some(p) = store::settings()["tofu_path"].as_str().map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    if own_path().is_file() {
        return Some(own_path());
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].iter().map(PathBuf::from));
    for bin in ["tofu", "terraform"] {
        if let Some(p) = dirs.iter().map(|d| d.join(bin)).find(|p| p.is_file()) {
            return Some(p);
        }
    }
    None
}

pub fn version(path: &Path) -> Option<String> {
    let out = Command::new(path).arg("version").output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next().map(|l| l.trim().to_string())
}

fn release_file() -> Result<(String, String), String> {
    let os = if cfg!(target_os = "macos") { "darwin" } else if cfg!(target_os = "linux") { "linux" } else { return Err("On Windows, install OpenTofu from opentofu.org and name its path in Settings (tofu_path).".into()) };
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    Ok((format!("tofu_{TOFU_VERSION}_{os}_{arch}.zip"), format!("https://github.com/opentofu/opentofu/releases/download/v{TOFU_VERSION}")))
}

/// download the official release into the app's folder, check its SHA-256 against the release's checksum file, unzip
pub fn install(jobs: &Jobs) -> Result<(), String> {
    let log = jobs.start("install")?;
    std::thread::spawn(move || {
        let r = (|| -> Result<String, String> {
            let (file, base) = release_file()?;
            let dir = own_path().parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            log.line(format!("Downloading {base}/{file} …"));
            let mut zip = vec![];
            ureq::get(format!("{base}/{file}")).call().map_err(|e| format!("download failed: {e}"))?.into_body().as_reader().read_to_end(&mut zip).map_err(|e| e.to_string())?;
            log.line(format!("{} MB downloaded; checking the SHA-256 …", zip.len() / 1_000_000));
            let sums = ureq::get(format!("{base}/tofu_{TOFU_VERSION}_SHA256SUMS")).call().map_err(|e| format!("checksum file: {e}"))?.into_body().read_to_string().map_err(|e| e.to_string())?;
            let want = sums.lines().find(|l| l.ends_with(&file)).and_then(|l| l.split_whitespace().next()).ok_or("the checksum file does not list this release")?.to_lowercase();
            use sha2::Digest;
            let have = format!("{:x}", sha2::Sha256::digest(&zip));
            if have != want {
                return Err(format!("the download's SHA-256 {have} is not the published {want}; nothing was installed"));
            }
            let zip_path = dir.join(&file);
            std::fs::write(&zip_path, &zip).map_err(|e| e.to_string())?;
            let status = if cfg!(target_os = "macos") {
                Command::new("ditto").args(["-x", "-k", &zip_path.to_string_lossy(), &dir.to_string_lossy()]).status()
            } else {
                Command::new("unzip").args(["-o", &zip_path.to_string_lossy(), "-d", &dir.to_string_lossy()]).status()
            }.map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&zip_path);
            if !status.success() {
                return Err("unzip failed".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(own_path(), std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            }
            let v = version(&own_path()).unwrap_or_default();
            log.line(format!("Installed {v} at {}", own_path().display()));
            Ok(format!("OpenTofu installed: {v}"))
        })();
        log.finish(r);
    });
    Ok(())
}

// ---------------------------------------------------------------- the project's working folder and state
pub fn work_dir(project: &Path) -> PathBuf {
    project.join("tofu")
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StateInfo {
    pub has_state: bool,
    pub resources: usize,
    pub last_apply: Option<String>,
    pub backups: usize,
    /// a plan file exists and is newer than the generated code
    pub plan_fresh: bool,
    pub plan_destroy: bool,
}

pub fn state_info(project: &Path) -> StateInfo {
    let w = work_dir(project);
    let state = w.join("terraform.tfstate");
    let mut s = StateInfo { has_state: state.is_file(), ..Default::default() };
    if let Some(v) = store::read_json::<serde_json::Value>(&state) {
        s.resources = v["resources"].as_array().map(|a| a.iter().filter(|r| r["mode"].as_str() == Some("managed")).map(|r| r["instances"].as_array().map(|i| i.len()).unwrap_or(0)).sum()).unwrap_or(0);
    }
    s.last_apply = std::fs::metadata(&state).and_then(|m| m.modified()).ok().map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string());
    s.backups = std::fs::read_dir(w.join("backups")).map(|d| d.count()).unwrap_or(0);
    let plan_time = std::fs::metadata(w.join("plan.tfplan")).and_then(|m| m.modified()).ok();
    let code_time = std::fs::metadata(w.join("main.tf")).and_then(|m| m.modified()).ok();
    s.plan_fresh = matches!((plan_time, code_time), (Some(p), Some(c)) if p >= c);
    s.plan_destroy = w.join("plan.destroy").is_file();
    s
}

fn backup_state(w: &Path, log: &Log) -> Result<(), String> {
    let state = w.join("terraform.tfstate");
    if !state.is_file() {
        return Ok(());
    }
    let dir = w.join("backups");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let to = dir.join(format!("terraform.tfstate.{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::copy(&state, &to).map_err(|e| format!("could not back up the state: {e}"))?;
    log.line(format!("State backed up to {}", to.display()));
    Ok(())
}

/// everything a run needs; `env` carries the credentials and is never logged
pub struct Ctx {
    pub tofu: PathBuf,
    pub project: PathBuf,
    pub project_name: String,
    pub design: Design,
    pub env: Vec<(String, String)>,
}

fn command(cx: &Ctx, args: &[&str]) -> Command {
    let mut c = Command::new(&cx.tofu);
    c.args(args).current_dir(work_dir(&cx.project)).env("TF_IN_AUTOMATION", "1").env("TF_INPUT", "0").env("AWS_SDK_LOAD_CONFIG", "1")
        .env("TF_PLUGIN_CACHE_DIR", store::home().join("plugin-cache")).env("AWS_REGION", &cx.design.region).env("AWS_DEFAULT_REGION", &cx.design.region);
    for (k, v) in &cx.env {
        c.env(k, v);
    }
    c
}

/// run a command, streaming each output line into the log (through `each` first, which may mark blocks)
fn run(cx: &Ctx, args: &[&str], log: &Log, mut each: impl FnMut(&str, &Log)) -> Result<(), String> {
    let mut child = command(cx, args).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).spawn().map_err(|e| format!("could not start {}: {e}", cx.tofu.display()))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let log2 = log.clone();
    let err_thread = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
            if !line.trim().is_empty() {
                log2.line(line);
            }
        }
    });
    for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
        each(&line, log);
    }
    let _ = err_thread.join();
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("tofu {} exited with {}", args.first().unwrap_or(&""), status.code().map(|c| c.to_string()).unwrap_or_else(|| "a signal".into()))) }
}

/// the ids of every resource the state file already holds: those are managed, an import block for them would fail
pub fn managed_ids(work: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(text) = std::fs::read_to_string(work.join("terraform.tfstate")) else { return out };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return out };
    for r in v["resources"].as_array().cloned().unwrap_or_default() {
        for i in r["instances"].as_array().cloned().unwrap_or_default() {
            if let Some(id) = i["attributes"]["id"].as_str() {
                out.insert(id.to_string());
            }
        }
    }
    out
}

fn write_code(cx: &Ctx) -> Result<(), String> {
    let w = work_dir(&cx.project);
    std::fs::create_dir_all(&w).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(w.join("imports.tf")); // written again below only when something is left to adopt
    hcl::write_to(&hcl::generate_with(&cx.design, &managed_ids(&w)), &w).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(store::home().join("plugin-cache")).map_err(|e| e.to_string())?;
    Ok(())
}

fn init_if_needed(cx: &Ctx, log: &Log) -> Result<(), String> {
    let w = work_dir(&cx.project);
    if w.join(".terraform").join("providers").is_dir() {
        return Ok(());
    }
    log.line("tofu init (the AWS provider is downloaded once) …");
    run(cx, &["init", "-input=false", "-no-color"], log, |l, log| log.line(l))
}

/// plan (or a destroy plan): the code is written again, init runs when needed, the plan is saved as plan.tfplan and summarised
pub fn plan(jobs: &Jobs, cx: Ctx, destroy: bool) -> Result<(), String> {
    let log = jobs.start(if destroy { "plan-destroy" } else { "plan" })?;
    std::thread::spawn(move || {
        let r = (|| -> Result<String, String> {
            write_code(&cx)?;
            let w = work_dir(&cx.project);
            let _ = std::fs::remove_file(w.join("plan.tfplan"));
            let _ = std::fs::remove_file(w.join("plan.destroy"));
            init_if_needed(&cx, &log)?;
            log.line(if destroy { "tofu plan -destroy …" } else { "tofu plan …" });
            let mut args = vec!["plan", "-input=false", "-no-color", "-out=plan.tfplan"];
            if destroy {
                args.push("-destroy");
            }
            run(&cx, &args, &log, |l, log| log.line(l))?;
            let out = command(&cx, &["show", "-json", "plan.tfplan"]).output().map_err(|e| e.to_string())?;
            let summary = plan::parse_plan(&cx.design, &String::from_utf8_lossy(&out.stdout))?;
            if destroy {
                std::fs::write(w.join("plan.destroy"), "1").map_err(|e| e.to_string())?;
            }
            let msg = summary.summary.clone();
            log.with(|j| j.plan = Some(summary));
            Ok(msg)
        })();
        log.finish(r);
    });
    Ok(())
}

/// apply the saved plan: the state is backed up first; every finished resource marks its block; outputs are read at the end
pub fn apply(jobs: &Jobs, cx: Ctx, destroy: bool) -> Result<(), String> {
    let log = jobs.start(if destroy { "destroy" } else { "apply" })?;
    std::thread::spawn(move || {
        let r = (|| -> Result<String, String> {
            let w = work_dir(&cx.project);
            if !w.join("plan.tfplan").is_file() {
                return Err("No plan: run Plan first.".into());
            }
            backup_state(&w, &log)?;
            log.line(if destroy { "tofu apply (destroy) …" } else { "tofu apply …" });
            let design = cx.design.clone();
            let mut failed = 0;
            run(&cx, &["apply", "-input=false", "-no-color", "-json", "plan.tfplan"], &log, |l, log| {
                let Some(ev) = plan::parse_apply_line(&design, l) else { return };
                if !ev.message.is_empty() && ev.kind != "version" {
                    log.line(ev.message.clone());
                }
                if ev.error {
                    failed += 1;
                }
                if let Some(b) = &ev.block_id {
                    let mark = match ev.kind.as_str() {
                        "apply_start" => Some(if destroy { "destroying" } else { "creating" }),
                        "apply_complete" => Some(if destroy { "destroyed" } else { "created" }),
                        "apply_errored" => Some("failed"),
                        _ => None,
                    };
                    if let Some(m) = mark {
                        let aws = ev.aws_id.clone();
                        let b = b.clone();
                        log.with(move |j| {
                            // a block with several resources (NAT + EIP, table + routes): "created" only when nothing of it is still running
                            if (m == "created" || m == "destroyed") && j.marks.get(&b).map(|x| x == "failed").unwrap_or(false) {
                                return;
                            }
                            j.marks.insert(b.clone(), m.into());
                            if let Some(id) = aws {
                                // the block's main resource wins over helpers (the NAT over its EIP, the table over its routes)
                                let main = ev.address.as_deref().map(|a| !a.starts_with("aws_eip.") && !a.starts_with("aws_route.") && !a.starts_with("aws_route_table_association.") && !a.contains("_rule.")).unwrap_or(true);
                                if main || !j.aws_ids.contains_key(&b) {
                                    j.aws_ids.insert(b, id);
                                }
                            }
                        });
                    }
                }
            })?;
            let _ = std::fs::remove_file(w.join("plan.tfplan"));
            let _ = std::fs::remove_file(w.join("plan.destroy"));
            if !destroy {
                if let Ok(out) = command(&cx, &["output", "-json"]).output() {
                    let outs = plan::parse_outputs(&String::from_utf8_lossy(&out.stdout));
                    log.with(|j| j.outputs = outs);
                }
            }
            if failed > 0 {
                return Err(format!("{failed} error{} during apply: see the log. The state file has what was created; Plan again to see what is left.", if failed == 1 { "" } else { "s" }));
            }
            Ok(if destroy { "Destroyed. The state is empty; the backup before the destroy is in tofu/backups.".into() } else { "Applied. The outputs are below; the blocks show their AWS ids.".into() })
        })();
        log.finish(r);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the adopt, for real: import the VPC named by AWS_INFRA_BUILDER_VPC (read-only), plan its untouched design, and expect
    /// nothing created, replaced or destroyed: everything is adopted. Only with AWS_INFRA_BUILDER_REAL=1; plan only, never apply
    #[test]
    #[ignore]
    fn real_plan_of_an_imported_vpc_adopts_everything() {
        if std::env::var("AWS_INFRA_BUILDER_REAL").is_err() {
            return;
        }
        let vpc_id = std::env::var("AWS_INFRA_BUILDER_VPC").expect("AWS_INFRA_BUILDER_VPC=<vpc id>");
        let region = std::env::var("AWS_INFRA_BUILDER_REGION").unwrap_or_else(|_| "ap-south-1".into());
        let t = tempfile::tempdir().unwrap();
        std::env::set_var("AWS_INFRA_BUILDER_HOME", t.path().join("home"));
        let tofu = locate().expect("OpenTofu installed");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let creds = crate::aws::Creds::Profile("default".into());
        let identity = rt.block_on(crate::aws::identity(creds.clone(), &region)).unwrap();
        let inv = rt.block_on(crate::aws::inventory(creds, &region, &vpc_id, &identity.account)).unwrap();
        let imported = aib_core::import::to_design(&inv);
        let mut design = imported.design;
        aib_core::dsl::sync(&mut design);
        println!("{}", design.text.clone().unwrap_or_default());
        let project = t.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let jobs = Jobs::default();
        plan(&jobs, Ctx { tofu, project: project.clone(), project_name: "adopt".into(), design: design.clone(), env: vec![("AWS_PROFILE".into(), "default".into())] }, false).unwrap();
        let start = std::time::Instant::now();
        while jobs.running() && start.elapsed().as_secs() < 900 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let snap = jobs.snapshot(0);
        println!("{}", snap.log.join("\n"));
        assert_eq!(snap.state, "done", "{}", snap.message);
        let p = snap.plan.expect("a plan summary");
        println!("PLAN: {}", p.summary);
        for c in &p.changes {
            println!("  {} {} ({}){}", c.action, c.address, c.block_label.clone().unwrap_or_default(), if c.replace_because.is_empty() { String::new() } else { format!(" because {}", c.replace_because.join(", ")) });
        }
        assert!(p.imports > 0, "{}", p.summary);
        assert_eq!((p.creates, p.replaces, p.destroys), (0, 0, 0), "{}", p.summary);
        assert!(aib_core::plan::duplicates(&design, &p).is_empty());
    }

    /// a real `tofu plan` of the cheapest template with the owner's default profile: read-only (plan creates nothing), only when
    /// asked for (AWS_INFRA_BUILDER_REAL=1); the project lives in a temp folder, OpenTofu must be installed
    #[test]
    #[ignore]
    fn real_plan_of_the_web_server_template() {
        if std::env::var("AWS_INFRA_BUILDER_REAL").is_err() {
            return;
        }
        let t = tempfile::tempdir().unwrap();
        std::env::set_var("AWS_INFRA_BUILDER_HOME", t.path().join("home"));
        let tofu = locate().expect("OpenTofu installed");
        let design = aib_core::templates::build("web_server").unwrap();
        let project = t.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let jobs = Jobs::default();
        plan(&jobs, Ctx { tofu, project: project.clone(), project_name: "test".into(), design: design.clone(), env: vec![("AWS_PROFILE".into(), "default".into())] }, false).unwrap();
        let start = std::time::Instant::now();
        while jobs.running() && start.elapsed().as_secs() < 600 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let snap = jobs.snapshot(0);
        println!("{}", snap.log.join("\n"));
        assert_eq!(snap.state, "done", "{}", snap.message);
        let p = snap.plan.expect("a plan summary");
        // vpc, subnet, igw, route table, route, association, group, 3 ingress rules, egress rule, instance
        assert_eq!((p.creates, p.updates, p.replaces, p.destroys), (12, 0, 0, 0), "{}", p.summary);
        assert_eq!(p.by_block[&design.by_label("web_1").unwrap().id], "create");
        assert!(p.cost_delta > 5.0, "{}", p.cost_delta);
        let info = state_info(&project);
        assert!(info.plan_fresh && !info.has_state);
        println!("PLAN OK: {}", p.summary);
    }
}
