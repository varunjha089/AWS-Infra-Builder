//! Files: the settings folder (AWS_INFRA_BUILDER_HOME, else ~/Library/Application Support/AWS Infra Builder), settings.json, and
//! project folders. A project = project.json + design.json (+ later the generated HCL and state). Projects live wherever the owner
//! chooses. A template opens as the one draft (settings folder/draft): nothing is written to the projects folder until it is
//! saved (save_as). An unsaved draft is discarded when another project is opened or created, and when the app quits or starts.

use crate::model::Design;
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const APP: &str = "AWS Infra Builder";

pub fn home() -> PathBuf {
    let h = if let Ok(v) = std::env::var("AWS_INFRA_BUILDER_HOME") {
        PathBuf::from(v)
    } else {
        let base = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()));
        if cfg!(target_os = "macos") {
            base.join("Library/Application Support").join(APP)
        } else if cfg!(windows) {
            PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join(APP)
        } else {
            base.join(".local/share/aws-infra-builder")
        }
    };
    let _ = std::fs::create_dir_all(&h);
    h
}

pub fn read_json<T: DeserializeOwned>(p: &Path) -> Option<T> {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// atomic write (a crash never leaves half a file)
pub fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v)?)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

pub fn settings() -> Value {
    read_json(&home().join("settings.json")).unwrap_or_else(|| json!({}))
}

static SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn save_setting(key: &str, value: Value) -> Result<()> {
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = settings();
    s.as_object_mut().context("settings.json is not an object")?.insert(key.into(), value);
    write_json(&home().join("settings.json"), &s)
}

pub fn default_projects_dir() -> PathBuf {
    settings()["projects_dir"].as_str().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("AWS_Infra_Builder_Projects"))
}

#[derive(Debug, Clone)]
pub struct Project {
    pub dir: PathBuf,
}

impl Project {
    pub fn name(&self) -> String {
        read_json::<Value>(&self.dir.join("project.json")).and_then(|v| v["name"].as_str().map(String::from)).unwrap_or_else(|| self.dir.file_name().unwrap_or_default().to_string_lossy().into())
    }
    pub fn design(&self) -> Design {
        read_json(&self.dir.join("design.json")).unwrap_or_default()
    }
    pub fn save_design(&self, d: &Design) -> Result<()> {
        write_json(&self.dir.join("design.json"), d)
    }
    pub fn is_draft(&self) -> bool {
        self.dir == draft_dir()
    }
}

/// the one unsaved project: a template opened from the Projects page
pub fn draft_dir() -> PathBuf {
    home().join("draft")
}

pub fn is_project(p: &Path) -> bool {
    p.join("project.json").is_file()
}

/// paths are kept in their canonical form, so that one folder is listed once however it was typed
fn remember(p: &Path) -> Result<()> {
    let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_string();
    let mut list: Vec<String> = settings()["projects"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    list.retain(|x| *x != p);
    list.insert(0, p.clone());
    list.truncate(50);
    save_setting("projects", json!(list))?;
    save_setting("last_project", json!(p))
}

/// the folder a new project with this name gets (refused when it exists)
fn new_dir(name: &str, location: Option<&Path>) -> Result<PathBuf> {
    if name.is_empty() {
        bail!("Give the project a name.");
    }
    let folder: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect::<String>().trim_matches(|c| c == ' ' || c == '.').to_string();
    let dir = location.map(PathBuf::from).unwrap_or_else(default_projects_dir).join(if folder.is_empty() { "project".into() } else { folder });
    if dir.exists() {
        bail!("A folder {} already exists. Choose another name.", dir.display());
    }
    Ok(dir)
}

fn init(dir: &Path, name: &str, draft: bool, design: Design) -> Result<Project> {
    std::fs::create_dir_all(dir)?;
    let mut meta = json!({"name": name, "created": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(), "app": APP});
    if draft {
        meta["draft"] = json!(true);
    }
    write_json(&dir.join("project.json"), &meta)?;
    let pr = Project { dir: dir.into() };
    pr.save_design(&design)?;
    Ok(pr)
}

/// a new, empty design named like the project
pub fn create(name: &str, location: Option<&Path>) -> Result<Project> {
    let name = name.trim();
    let dir = new_dir(name, location)?;
    discard_draft()?;
    let mut d = Design { name: name.into(), ..Default::default() };
    crate::dsl::sync(&mut d);
    init(&dir, name, false, d)?;
    remember(&dir)?;
    Ok(Project { dir: dir.canonicalize().unwrap_or(dir) })
}

/// start the draft afresh (an unsaved draft is replaced) with this design
pub fn create_draft(name: &str, design: Design) -> Result<Project> {
    discard_draft()?;
    let pr = init(&draft_dir(), name.trim(), true, design)?;
    save_setting("last_project", json!(draft_dir().to_string_lossy()))?;
    Ok(pr)
}

/// throw the unsaved draft away (it is app scratch, not a project); last_project no longer points at it
pub fn discard_draft() -> Result<()> {
    let dir = draft_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir).context("could not remove the unsaved draft")?;
    }
    if settings()["last_project"].as_str() == Some(&*dir.to_string_lossy()) {
        save_setting("last_project", Value::Null)?;
    }
    Ok(())
}

/// the draft becomes a real project in the projects folder (moved, or copied when that is on another disk)
pub fn save_as(draft: &Project, name: &str, location: Option<&Path>) -> Result<Project> {
    if !draft.is_draft() {
        bail!("This project is already saved: every change is written as you make it.");
    }
    let name = name.trim();
    let dir = new_dir(name, location)?;
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(&draft.dir, &dir).is_err() {
        copy_dir(&draft.dir, &dir)?;
        std::fs::remove_dir_all(&draft.dir)?;
    }
    let mut meta: Value = read_json(&dir.join("project.json")).unwrap_or_else(|| json!({"app": APP}));
    meta["name"] = json!(name);
    if let Some(m) = meta.as_object_mut() {
        m.remove("draft");
    }
    write_json(&dir.join("project.json"), &meta)?;
    remember(&dir)?;
    Ok(Project { dir: dir.canonicalize().unwrap_or(dir) })
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// a project folder as the owner typed or pasted it: quotes and spaces removed, ~/ = the home folder, and a file inside a project
/// (project.json, design.json) stands for its folder
pub fn project_path(typed: &str) -> PathBuf {
    let t = typed.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    let p = match t.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
        None if t == "~" => PathBuf::from(std::env::var("HOME").unwrap_or_default()),
        None => PathBuf::from(t),
    };
    if p.is_file() {
        if let Some(parent) = p.parent().filter(|d| is_project(d)) {
            return parent.to_path_buf();
        }
    }
    p
}

pub fn open(path: &Path) -> Result<Project> {
    let path = &project_path(&path.to_string_lossy());
    if !path.exists() {
        bail!("There is no folder {}.", path.display());
    }
    if !is_project(path) {
        bail!("{} is not an {APP} project (it has no project.json). Choose the project's own folder, the one with project.json and design.json in it.", path.display());
    }
    let dir = path.canonicalize()?;
    if dir == draft_dir() || Some(&dir) == draft_dir().canonicalize().ok().as_ref() {
        save_setting("last_project", json!(draft_dir().to_string_lossy()))?;
        return Ok(Project { dir: draft_dir() });
    }
    discard_draft()?;
    remember(&dir)?;
    Ok(Project { dir })
}

/// take a project off the projects list (and off "last project"); its folder is not touched
pub fn forget(p: &Path) -> Result<()> {
    let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_string();
    let mut list: Vec<String> = settings()["projects"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    list.retain(|x| *x != p);
    save_setting("projects", json!(list))?;
    if settings()["last_project"].as_str() == Some(&*p) {
        save_setting("last_project", Value::Null)?;
    }
    Ok(())
}

pub fn listing() -> Vec<Value> {
    settings()["projects"].as_array().cloned().unwrap_or_default().iter().filter_map(|v| v.as_str()).map(|p| {
        let pp = Path::new(p);
        json!({"path": p, "exists": is_project(pp), "name": if is_project(pp) { Project { dir: pp.into() }.name() } else { pp.file_name().unwrap_or_default().to_string_lossy().into() }})
    }).collect()
}

pub fn last_project() -> Option<Project> {
    let p = PathBuf::from(settings()["last_project"].as_str()?);
    is_project(&p).then_some(Project { dir: p })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// one scratch home for the whole test (the env var is process-wide)
    fn scratch() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::env::set_var("AWS_INFRA_BUILDER_HOME", t.path().join("home"));
        t
    }

    #[test]
    fn projects_drafts_and_listing() {
        let t = scratch();
        let projects = t.path().join("projects");
        let p = create("My net", Some(&projects)).unwrap();
        assert!(p.dir.join("design.json").is_file());
        assert_eq!(p.name(), "My net");
        assert_eq!(p.design().name, "My net");
        assert!(p.design().text.unwrap().contains("region ap-south-1"));
        assert!(create("My net", Some(&projects)).is_err(), "same folder twice");
        assert_eq!(listing().len(), 1);
        assert_eq!(last_project().unwrap().dir, p.dir);

        let d = crate::templates::build("two_tier").unwrap();
        let dr = create_draft("Two-tier VPC", d).unwrap();
        assert!(dr.is_draft());
        assert_eq!(dr.design().nodes.len(), 16);
        assert_eq!(last_project().unwrap().dir, draft_dir());
        let saved = save_as(&dr, "Saved net", Some(&projects)).unwrap();
        assert!(!saved.is_draft() && !draft_dir().exists());
        assert_eq!(saved.design().nodes.len(), 16);
        assert_eq!(listing().len(), 2);
        assert!(save_as(&saved, "again", Some(&projects)).is_err());

        let opened = open(&saved.dir.join("design.json")).unwrap();
        assert_eq!(opened.dir, saved.dir);
        assert_eq!(listing().len(), 2, "opened by another spelling of the path, listed once");
        assert!(open(&t.path().join("nothing")).is_err());
        forget(&saved.dir).unwrap();
        assert_eq!(listing().len(), 1);
        assert!(last_project().is_none());
    }
}
