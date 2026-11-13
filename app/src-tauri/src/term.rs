//! Terminal sessions (the Terminal tab): the system `ssh` running in a pseudo-terminal, its output streamed to the window as
//! `term-data` events (base64 bytes) and `term-exit` when it ends. The key file is only named on ssh's command line; this
//! program never reads it. Host keys are checked by ssh as usual (the first-connection question is answered in the terminal).

use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

#[derive(Default)]
pub struct Terms {
    sessions: Mutex<HashMap<String, Session>>,
}

#[derive(Serialize, Clone)]
struct Data {
    id: String,
    data: String,
}

#[derive(Serialize, Clone)]
struct Exit {
    id: String,
    code: Option<u32>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows: rows.max(2), cols: cols.max(2), pixel_width: 0, pixel_height: 0 }
}

impl Terms {
    /// start `program args` in a new pseudo-terminal; its output goes to the window as events for `id`
    pub fn open(&self, app: tauri::AppHandle, id: &str, program: &str, args: &[String], cols: u16, rows: u16) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if sessions.contains_key(id) {
            return Err("That terminal is already open.".into());
        }
        let pair = native_pty_system().openpty(size(cols, rows)).map_err(|e| format!("Could not open a terminal: {e}"))?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.env("TERM", "xterm-256color");
        cmd.env("LANG", std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into()));
        if let Some(home) = std::env::var_os("HOME") {
            cmd.cwd(home);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("Could not start {program}: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let child = Arc::new(Mutex::new(child));
        let child2 = child.clone();
        let sid = id.to_string();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                        let _ = app.emit("term-data", Data { id: sid.clone(), data });
                    }
                }
            }
            let code = child2.lock().unwrap_or_else(|e| e.into_inner()).wait().ok().map(|s| s.exit_code());
            let _ = app.emit("term-exit", Exit { id: sid, code });
        });
        sessions.insert(id.to_string(), Session { master: pair.master, writer, child });
        Ok(())
    }

    pub fn write(&self, id: &str, data: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let s = sessions.get_mut(id).ok_or("That terminal is closed.")?;
        s.writer.write_all(data.as_bytes()).and_then(|_| s.writer.flush()).map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let s = sessions.get(id).ok_or("That terminal is closed.")?;
        s.master.resize(size(cols, rows)).map_err(|e| e.to_string())
    }

    /// end the session (the reader thread then reports `term-exit`)
    pub fn close(&self, id: &str) {
        let removed = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
        if let Some(s) = removed {
            let _ = s.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
    }

    pub fn open_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
        v.sort();
        v
    }
}

/// "~" at the start of a path becomes the home folder
pub fn expand_home(p: &str) -> String {
    let p = p.trim();
    if let Some(rest) = p.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with('/') {
            if let Some(h) = std::env::var_os("HOME") {
                return format!("{}{}", h.to_string_lossy(), rest);
            }
        }
    }
    p.to_string()
}

/// what ssh will say about the key file before it is even tried: missing, or readable by others (ssh refuses those)
pub fn key_problem(path: &str) -> Option<String> {
    if path.trim().is_empty() {
        return None;
    }
    let p = expand_home(path);
    let meta = match std::fs::metadata(&p) {
        Ok(m) => m,
        Err(_) => return Some(format!("The key file {p} does not exist. Download the key pair's .pem file from the console (it is given once, when the pair is made) and choose it here.")),
    };
    if meta.is_dir() {
        return Some(format!("{p} is a folder, not a key file."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Some(format!("ssh refuses a key others can read: {p} needs mode 600 (Fix permissions sets it)."));
        }
    }
    None
}

/// `chmod 600` on the key file, so ssh accepts it
pub fn fix_key_permissions(path: &str) -> Result<(), String> {
    let p = expand_home(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).map_err(|e| format!("Could not change {p}: {e}"))
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        Ok(())
    }
}

/// the key folder, made with mode 700 when missing
pub fn ensure_dir(dir: &std::path::Path) -> Result<(), String> {
    if !dir.is_dir() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not make {}: {e}", dir.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// move a key file into the key folder (copy + remove when a plain rename fails, e.g. from another volume), mode 600;
/// never overwrites. Returns the new path
pub fn move_key_into(path: &str, dir: &std::path::Path) -> Result<String, String> {
    let from = std::path::PathBuf::from(expand_home(path));
    if !from.is_file() {
        return Err(format!("{} is not a file.", from.display()));
    }
    ensure_dir(dir)?;
    let name = from.file_name().ok_or("The key file has no name.")?.to_os_string();
    let to = dir.join(&name);
    if to == from {
        fix_key_permissions(&to.to_string_lossy())?;
        return Ok(to.to_string_lossy().into_owned());
    }
    if to.exists() {
        return Err(format!("{} already exists; rename one of them first.", to.display()));
    }
    if std::fs::rename(&from, &to).is_err() {
        std::fs::copy(&from, &to).map_err(|e| format!("Could not copy to {}: {e}", to.display()))?;
        std::fs::remove_file(&from).map_err(|e| format!("Copied to {}, but the original could not be removed: {e}", to.display()))?;
    }
    fix_key_permissions(&to.to_string_lossy())?;
    Ok(to.to_string_lossy().into_owned())
}

/// write a freshly created private key as <dir>/<name>.pem with mode 600 (created new, never overwritten)
pub fn write_new_key(dir: &std::path::Path, name: &str, material: &str) -> Result<String, String> {
    ensure_dir(dir)?;
    let to = dir.join(format!("{name}.pem"));
    if to.exists() {
        return Err(format!("{} already exists: the new key was NOT written. Move the old file away and create the pair again under another name.", to.display()));
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&to).map_err(|e| format!("Could not create {}: {e}", to.display()))?;
    f.write_all(material.as_bytes()).and_then(|_| if material.ends_with('\n') { Ok(()) } else { f.write_all(b"\n") }).map_err(|e| format!("Could not write {}: {e}", to.display()))?;
    Ok(to.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_move_into_the_folder_with_mode_600_and_new_keys_are_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let ssh = root.path().join("home/.ssh");
        let dl = root.path().join("Downloads/lab.pem");
        std::fs::create_dir_all(dl.parent().unwrap()).unwrap();
        std::fs::write(&dl, "key").unwrap();
        let moved = move_key_into(&dl.to_string_lossy(), &ssh).unwrap();
        assert!(moved.ends_with("/.ssh/lab.pem") && !dl.exists());
        assert_eq!(key_problem(&moved), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&ssh).unwrap().permissions().mode() & 0o777, 0o700);
        }
        std::fs::write(&dl, "key2").unwrap();
        assert!(move_key_into(&dl.to_string_lossy(), &ssh).unwrap_err().contains("already exists"));
        let p = write_new_key(&ssh, "nn", "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----").unwrap();
        assert!(p.ends_with("nn.pem"));
        assert_eq!(key_problem(&p), None);
        assert!(std::fs::read_to_string(&p).unwrap().ends_with("-----\n"));
        assert!(write_new_key(&ssh, "nn", "other").unwrap_err().contains("NOT written"));
        assert_eq!(std::fs::read_to_string(&p).unwrap().contains("abc"), true);
    }

    #[test]
    fn key_problems_are_named_and_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let k = dir.path().join("lab.pem");
        assert!(key_problem(&k.to_string_lossy()).unwrap().contains("does not exist"));
        std::fs::write(&k, "not really a key").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&k, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(key_problem(&k.to_string_lossy()).unwrap().contains("mode 600"));
            fix_key_permissions(&k.to_string_lossy()).unwrap();
        }
        assert_eq!(key_problem(&k.to_string_lossy()), None);
        assert_eq!(key_problem(""), None);
        assert_eq!(key_problem(&dir.path().to_string_lossy()).unwrap().contains("folder"), true);
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_home("~/.ssh/x.pem"), format!("{home}/.ssh/x.pem"));
        assert_eq!(expand_home("/abs/x"), "/abs/x");
        assert_eq!(expand_home("~me/x"), "~me/x");
    }

    #[test]
    fn a_local_shell_runs_in_a_terminal_and_echoes() {
        // no Tauri app handle in a test: only the pty plumbing through the same crate, with `sh -c`
        let pair = native_pty_system().openpty(size(80, 24)).unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.args(["-c", "echo hello-from-pty"]);
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut out = String::new();
        let mut buf = [0u8; 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            out.push_str(&String::from_utf8_lossy(&buf[..n]));
            if out.contains("hello-from-pty") { break; }
        }
        assert!(out.contains("hello-from-pty"), "{out}");
        let _ = child.wait();
    }
}
