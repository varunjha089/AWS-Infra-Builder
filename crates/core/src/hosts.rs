//! Hosts to open a terminal on: the SSH side of an instance (the Terminal tab, like Termius). The app spawns the system `ssh`
//! in a pseudo-terminal (term.rs in the app); this module only decides who to connect to and how: the user name the OS
//! expects, the key file by the key pair's name, the arguments. Saved hosts live in settings.json under "hosts". No key is
//! ever read here: ssh reads it. Nothing here talks to AWS and nothing costs money (SSH traffic to an instance is free).

use crate::model::Design;
use crate::store;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Host {
    pub id: String,
    pub label: String,
    /// address or DNS name; "" when not known yet
    pub host: String,
    pub user: String,
    pub port: u16,
    /// path of the private key file ("~" allowed); "" = ssh's own defaults (agent, ~/.ssh/config)
    pub key: String,
    /// the design block it came from, when it did
    #[serde(default)]
    pub block: Option<String>,
    #[serde(default)]
    pub aws_id: Option<String>,
    /// why it cannot be reached yet, in plain words ("" = fine)
    #[serde(default)]
    pub note: String,
}

/// the login name the AMI's OS creates
pub fn guess_user(os: &str) -> &'static str {
    match os {
        "ubuntu" => "ubuntu",
        "debian" => "admin",
        _ => "ec2-user",
    }
}

/// where the key pair's file is by convention when the console downloaded it
pub fn default_key_path(key_pair: &str) -> String {
    if key_pair.trim().is_empty() { String::new() } else { format!("~/.ssh/{}.pem", key_pair.trim()) }
}

/// one host per instance of the design, with what the design knows about it
pub fn suggest(d: &Design) -> Vec<Host> {
    let mut out = Vec::new();
    for n in d.nodes.iter().filter(|n| n.kind == "ec2") {
        if n.params.get("state").and_then(|v| v.as_str()) == Some("terminated") {
            continue;
        }
        let address = n.params.get("address").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let public = n.params.get("public_ip").and_then(|v| v.as_bool()).unwrap_or(false);
        let key_pair = n.params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let os = n.params.get("os").and_then(|v| v.as_str()).unwrap_or("al2023");
        let mut notes = Vec::new();
        if address.is_empty() {
            notes.push(if public { "The address is known after Apply or an import." } else { "No public address: turn one on (USD 3.65 a month) or connect through a host that has one." });
        }
        if key_pair.is_empty() {
            notes.push("No key pair on the block: ssh will use your agent or ~/.ssh/config.");
        }
        out.push(Host {
            id: format!("block:{}", n.id),
            label: n.label.clone(),
            host: address,
            user: guess_user(os).into(),
            port: 22,
            key: default_key_path(key_pair),
            block: Some(n.id.clone()),
            aws_id: n.params.get("aws_id").and_then(|v| v.as_str()).map(String::from),
            note: notes.join(" "),
        });
    }
    out
}

/// the arguments for `ssh`; `key_path` is the key file with "~" already expanded, when there is one
pub fn ssh_args(h: &Host, key_path: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = vec!["-p".into(), h.port.to_string(), "-o".into(), "ServerAliveInterval=30".into(), "-o".into(), "ConnectTimeout=15".into()];
    if let Some(k) = key_path.filter(|k| !k.is_empty()) {
        a.push("-i".into());
        a.push(k.into());
    }
    let user = h.user.trim();
    a.push(if user.is_empty() { h.host.trim().to_string() } else { format!("{}@{}", user, h.host.trim()) });
    a
}

/// what is wrong with a host before ssh is even started ("" = nothing)
pub fn check(h: &Host) -> String {
    let host = h.host.trim();
    if host.is_empty() {
        return "Give the host an address or a DNS name.".into();
    }
    if host.chars().any(|c| c.is_whitespace()) || host.starts_with('-') {
        return "The address must be one word and not start with a dash.".into();
    }
    if h.user.starts_with('-') {
        return "The user name must not start with a dash.".into();
    }
    if h.port == 0 {
        return "The port must be between 1 and 65535.".into();
    }
    String::new()
}

// ---------------------------------------------------------------- saved hosts (settings.json)
pub fn saved() -> Vec<Host> {
    serde_json::from_value(store::settings()["hosts"].clone()).unwrap_or_default()
}

pub fn save(hosts: &[Host]) -> anyhow::Result<()> {
    store::save_setting("hosts", json!(hosts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl;

    #[test]
    fn suggests_one_host_per_instance_with_the_right_user() {
        let mut d = dsl::parse("net \"t\"\nregion ap-south-1\nazs 1\n\nvpc main 10.0.0.0/16\nigw igw in main\nsubnet public_a public 10.0.0.0/24 az a in main rt public\nec2 web t4g.micro in public_a sg g key lab public os ubuntu\nec2 box t4g.micro in public_a sg g\nrt public in main: 0.0.0.0/0 -> igw\nsg g in main\n", None).unwrap();
        let web = d.by_label("web").unwrap().id.clone();
        d.nodes.iter_mut().find(|n| n.id == web).unwrap().params.insert("address".into(), json!("3.7.1.2"));
        let hs = suggest(&d);
        assert_eq!(hs.len(), 2);
        let w = hs.iter().find(|h| h.label == "web").unwrap();
        assert_eq!((w.user.as_str(), w.host.as_str(), w.key.as_str(), w.port), ("ubuntu", "3.7.1.2", "~/.ssh/lab.pem", 22));
        assert_eq!(w.note, "");
        assert_eq!(check(w), "");
        let b = hs.iter().find(|h| h.label == "box").unwrap();
        assert_eq!(b.user, "ec2-user");
        assert!(b.note.contains("No public address") && b.note.contains("No key pair"), "{}", b.note);
        assert!(!check(b).is_empty());
        let args = ssh_args(w, Some("/home/me/.ssh/lab.pem"));
        assert_eq!(args, vec!["-p", "22", "-o", "ServerAliveInterval=30", "-o", "ConnectTimeout=15", "-i", "/home/me/.ssh/lab.pem", "ubuntu@3.7.1.2"]);
        assert_eq!(ssh_args(b, None).last().unwrap(), "ec2-user@");
    }

    #[test]
    fn a_saved_host_reads_back_and_old_entries_without_the_new_fields_still_load() {
        let h = Host { id: "h1".into(), label: "lab".into(), host: "example.org".into(), user: "ubuntu".into(), port: 2222, key: "".into(), ..Default::default() };
        let back: Vec<Host> = serde_json::from_value(json!([h])).unwrap();
        assert_eq!(back, vec![h.clone()]);
        assert!(check(&back[0]).is_empty());
        let old: Host = serde_json::from_value(json!({"id": "x", "label": "x", "host": "h", "user": "u", "port": 22, "key": ""})).unwrap();
        assert_eq!(old.note, "");
        assert_eq!(guess_user("debian"), "admin");
        assert_eq!(default_key_path("  "), "");
    }
}
