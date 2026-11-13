//! Where the AWS credentials come from. Two sources: the profiles in ~/.aws/config and ~/.aws/credentials (read here only to list
//! their names and kinds; the secrets themselves are read by the AWS SDK when a call is made), or keys pasted into the app (kept in
//! the system keychain by the app, never in a project or a log). This module never returns a secret.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub name: String,
    /// "keys" (access key in the credentials file), "sso", "role" (assumes a role from another profile), "process", "web_identity" or "unknown"
    pub kind: String,
    /// the region the profile names, if any
    pub region: Option<String>,
    /// the first characters of the access key id, nothing secret
    pub key_hint: Option<String>,
}

/// which credentials the app uses: a profile, the pasted keys, or nothing yet
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    #[default]
    None,
    Profile { name: String },
    Keys,
}

pub fn aws_dir() -> PathBuf {
    if let Ok(v) = std::env::var("AWS_INFRA_BUILDER_AWS_DIR") {
        return PathBuf::from(v);
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".aws")
}

/// a minimal ini reader: section name -> key -> value (comments with # or ;, keys lower-cased)
fn ini(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut cur = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            cur = inner.trim().to_string();
            out.entry(cur.clone()).or_default();
            continue;
        }
        // nested keys (sso-session blocks indent their lines) are kept flat: they only matter to the SDK
        if let Some((k, v)) = line.split_once('=') {
            if !cur.is_empty() {
                out.entry(cur.clone()).or_default().insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
    }
    out
}

/// the profiles in the two files (the config file writes `[profile x]`, the credentials file `[x]`)
pub fn profiles_in(dir: &Path) -> Vec<Profile> {
    let config = std::fs::read_to_string(dir.join("config")).map(|t| ini(&t)).unwrap_or_default();
    let credentials = std::fs::read_to_string(dir.join("credentials")).map(|t| ini(&t)).unwrap_or_default();
    let mut names: Vec<String> = vec![];
    for sect in config.keys() {
        let name = sect.strip_prefix("profile ").unwrap_or(sect).trim().to_string();
        if sect.starts_with("sso-session ") || sect.starts_with("services ") {
            continue;
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    for sect in credentials.keys() {
        if !names.contains(sect) {
            names.push(sect.clone());
        }
    }
    names.sort_by_key(|n| (n != "default", n.clone()));
    names.into_iter().map(|name| {
        let c = config.get(&format!("profile {name}")).or_else(|| config.get(&name));
        let k = credentials.get(&name);
        let get = |key: &str| k.and_then(|m| m.get(key)).or_else(|| c.and_then(|m| m.get(key))).cloned();
        let kind = if get("aws_access_key_id").is_some() {
            "keys"
        } else if get("sso_session").is_some() || get("sso_start_url").is_some() {
            "sso"
        } else if get("role_arn").is_some() {
            "role"
        } else if get("credential_process").is_some() {
            "process"
        } else if get("web_identity_token_file").is_some() {
            "web_identity"
        } else {
            "unknown"
        };
        Profile { name, kind: kind.into(), region: get("region"), key_hint: get("aws_access_key_id").map(|k| format!("{}…", k.chars().take(4).collect::<String>())) }
    }).collect()
}

pub fn profiles() -> Vec<Profile> {
    profiles_in(&aws_dir())
}

/// a pasted access key id looks like AKIA… (long-term) or ASIA… (temporary, needs a session token); 16 to 128 characters
pub fn check_keys(id: &str, secret: &str, token: &str) -> Result<(), String> {
    let id = id.trim();
    let secret = secret.trim();
    if id.len() < 16 || id.len() > 128 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("The access key id should be 16 to 128 letters and digits (it starts with AKIA or ASIA).".into());
    }
    if secret.len() < 16 || secret.chars().any(char::is_whitespace) {
        return Err("The secret access key is at least 16 characters, without spaces.".into());
    }
    if id.starts_with("ASIA") && token.trim().is_empty() {
        return Err("A temporary key (ASIA…) also needs its session token.".into());
    }
    Ok(())
}

/// the read-only IAM policy that covers everything this version does (listing and importing; nothing is created)
pub fn read_only_policy() -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Sid": "AwsInfraBuilderReadOnly",
            "Effect": "Allow",
            "Action": ["ec2:Describe*", "sts:GetCallerIdentity", "pricing:GetProducts"],
            "Resource": "*"
        }, {
            "Sid": "AwsInfraBuilderKeyPairs",
            "Effect": "Allow",
            "Action": ["ec2:CreateKeyPair"],
            "Resource": "*"
        }]
    })).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_from_both_files() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("config"), "[default]\nregion = ap-south-1\noutput = json\n\n[profile work]\nsso_session = my-sso\nsso_account_id = 123456789012\nregion = us-east-1\n\n[sso-session my-sso]\nsso_start_url = https://example.awsapps.com/start\nsso_region = us-east-1\n\n[profile admin]\nrole_arn = arn:aws:iam::123456789012:role/Admin\nsource_profile = default\n").unwrap();
        // the secret lines of a real file are not needed by the parser and are left out of this fixture on purpose
        std::fs::write(t.path().join("credentials"), "# keys\n[default]\naws_access_key_id = AKIATESTKEY00001\n\n[lab]\naws_access_key_id = AKIATESTKEY00002\n").unwrap();
        let p = profiles_in(t.path());
        let names: Vec<_> = p.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["default", "admin", "lab", "work"]);
        assert_eq!(p[0].kind, "keys");
        assert_eq!(p[0].region.as_deref(), Some("ap-south-1"));
        assert_eq!(p[0].key_hint.as_deref(), Some("AKIA…"));
        assert_eq!(p[1].kind, "role");
        assert_eq!(p[2].kind, "keys");
        assert_eq!(p[2].region, None);
        assert_eq!(p[3].kind, "sso");
        assert_eq!(p[3].region.as_deref(), Some("us-east-1"));
        let s = serde_json::to_string(&p).unwrap();
        assert!(!s.contains("AKIATESTKEY00001"), "only a hint of the key id leaves this module");
        assert!(profiles_in(&t.path().join("missing")).is_empty());
    }

    #[test]
    fn pasted_keys_are_checked() {
        assert!(check_keys("AKIATESTKEY00001", "abcdefghijklmnopqrstuvwxyz0123456789", "").is_ok());
        assert!(check_keys("short", "abcdefghijklmnopqrstuvwxyz0123456789", "").is_err());
        assert!(check_keys("AKIATESTKEY00001", "short", "").is_err());
        assert!(check_keys("ASIATESTKEY00001", "abcdefghijklmnopqrstuvwxyz0123456789", "").is_err());
        assert!(check_keys("ASIATESTKEY00001", "abcdefghijklmnopqrstuvwxyz0123456789", "tok").is_ok());
        assert!(read_only_policy().contains("ec2:Describe*"));
    }
}
