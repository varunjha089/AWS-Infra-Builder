//! Plan and apply, read from OpenTofu's machine-readable output and mapped back to the design: `tofu show -json plan.tfplan` gives
//! every resource change (create, update, replace, destroy); `tofu apply -json` streams one JSON line per event. Each resource
//! carries the design block's label in its name and its `design-block` tag, so the canvas can colour the block. No process runs
//! here; the app runs OpenTofu and hands the text to these functions.

use crate::model::Design;
use crate::pricing;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Change {
    /// "aws_subnet.public_a"
    pub address: String,
    pub resource_type: String,
    pub name: String,
    /// "create" | "update" | "replace" | "destroy" | "no-op" | "read"
    pub action: String,
    /// the design block it belongs to (by tag or by name), if any
    pub block_id: Option<String>,
    pub block_label: Option<String>,
    /// why a replace: the attributes that force it
    pub replace_because: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanSummary {
    pub creates: usize,
    pub updates: usize,
    pub replaces: usize,
    pub destroys: usize,
    pub changes: Vec<Change>,
    /// block id -> the strongest action among its resources (destroy > replace > update > create)
    pub by_block: HashMap<String, String>,
    /// estimated change of the monthly cost (USD) from the blocks created and destroyed
    pub cost_delta: f64,
    /// blocks created, in design terms
    pub summary: String,
    /// true when nothing would change
    pub empty: bool,
}

fn rank(a: &str) -> u8 {
    match a { "destroy" => 4, "replace" => 3, "update" => 2, "create" => 1, _ => 0 }
}

fn action_of(actions: &[Value]) -> String {
    let a: Vec<&str> = actions.iter().filter_map(|v| v.as_str()).collect();
    match a.as_slice() {
        ["no-op"] => "no-op".into(),
        ["create"] => "create".into(),
        ["update"] => "update".into(),
        ["delete"] => "destroy".into(),
        ["read"] => "read".into(),
        x if x.contains(&"delete") && x.contains(&"create") => "replace".into(),
        _ => a.join("+"),
    }
}

/// the block a resource belongs to: its `design-block` tag (after or before), else a design block with the resource's name
fn block_for<'a>(d: &'a Design, rc: &Value, name: &str) -> Option<&'a crate::model::Node> {
    let tag = ["after", "before"].iter().find_map(|side| rc["change"][side]["tags"]["design-block"].as_str().or_else(|| rc["change"][side]["tags_all"]["design-block"].as_str()));
    tag.and_then(|l| d.by_label(l)).or_else(|| d.by_label(name)).or_else(|| {
        // "nat_a_rt_0" style names: the longest label that is a prefix
        d.nodes.iter().filter(|n| name.starts_with(&n.label) && (name.len() == n.label.len() || name[n.label.len()..].starts_with('_'))).max_by_key(|n| n.label.len())
    })
}

/// `tofu show -json <planfile>` output as a summary against the design
pub fn parse_plan(d: &Design, show_json: &str) -> Result<PlanSummary, String> {
    let v: Value = serde_json::from_str(show_json).map_err(|e| format!("the plan is not JSON: {e}"))?;
    let mut s = PlanSummary::default();
    for rc in v["resource_changes"].as_array().cloned().unwrap_or_default() {
        let action = action_of(rc["change"]["actions"].as_array().map(|a| a.as_slice()).unwrap_or(&[]));
        if rc["mode"].as_str() == Some("data") || action == "read" {
            continue;
        }
        let name = rc["name"].as_str().unwrap_or("").to_string();
        let block = block_for(d, &rc, &name);
        let replace_because = rc["change"]["replace_paths"].as_array().map(|a| a.iter().filter_map(|p| p.as_array()).map(|p| p.iter().filter_map(|x| x.as_str().map(String::from).or_else(|| x.as_u64().map(|n| n.to_string()))).collect::<Vec<_>>().join(".")).collect()).unwrap_or_default();
        match action.as_str() {
            "create" => s.creates += 1,
            "update" => s.updates += 1,
            "replace" => s.replaces += 1,
            "destroy" => s.destroys += 1,
            _ => {}
        }
        if let Some(b) = block.filter(|_| action != "no-op") {
            let cur = s.by_block.entry(b.id.clone()).or_insert_with(|| "no-op".into());
            if rank(&action) > rank(cur) {
                *cur = action.clone();
            }
        }
        s.changes.push(Change { address: rc["address"].as_str().unwrap_or("").into(), resource_type: rc["type"].as_str().unwrap_or("").into(), name, action,
            block_id: block.map(|b| b.id.clone()), block_label: block.map(|b| b.label.clone()), replace_because });
    }
    s.changes.retain(|c| c.action != "no-op");
    s.empty = s.creates + s.updates + s.replaces + s.destroys == 0;
    // cost: the monthly price of the blocks that appear (create) minus those that go (destroy), by the design's own estimate
    let cost = pricing::cost(d);
    let price = |id: &str| cost.items.iter().filter(|i| i.id == id).map(|i| i.monthly).sum::<f64>();
    for (id, action) in &s.by_block {
        match action.as_str() {
            "create" => s.cost_delta += price(id),
            "destroy" => s.cost_delta -= price(id),
            _ => {}
        }
    }
    s.summary = if s.empty {
        "No changes: the account already matches the design.".into()
    } else {
        format!("create {}, change {}, replace {}, destroy {} · {}{:.2} USD/month", s.creates, s.updates, s.replaces, s.destroys, if s.cost_delta >= 0.0 { "+" } else { "−" }, s.cost_delta.abs())
    };
    Ok(s)
}

/// one line of `tofu apply -json` / `tofu plan -json`, turned into a log line and, when a resource finished, the block it belongs to
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplyEvent {
    /// "apply_start" | "apply_complete" | "apply_errored" | "diagnostic" | "change_summary" | "outputs" | "log" | "version" …
    pub kind: String,
    pub message: String,
    pub address: Option<String>,
    pub block_id: Option<String>,
    /// the AWS id of a resource that was created (i-…, vpc-…, subnet-…)
    pub aws_id: Option<String>,
    pub error: bool,
}

pub fn parse_apply_line(d: &Design, line: &str) -> Option<ApplyEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Some(ApplyEvent { kind: "log".into(), message: line.to_string(), ..Default::default() });
    };
    let kind = v["type"].as_str().unwrap_or("log").to_string();
    let message = v["@message"].as_str().unwrap_or("").to_string();
    let level = v["@level"].as_str().unwrap_or("info");
    let address = v["hook"]["resource"]["addr"].as_str().map(String::from).or_else(|| v["diagnostic"]["address"].as_str().map(String::from));
    let name = v["hook"]["resource"]["resource_name"].as_str().unwrap_or("");
    let block_id = address.as_ref().and_then(|_| d.by_label(name).or_else(|| d.nodes.iter().filter(|n| name.starts_with(&n.label)).max_by_key(|n| n.label.len())).map(|n| n.id.clone()));
    let aws_id = if kind == "apply_complete" { v["hook"]["id_value"].as_str().map(String::from) } else { None };
    let mut msg = message;
    if kind == "diagnostic" {
        let detail = v["diagnostic"]["detail"].as_str().unwrap_or("").trim();
        if !detail.is_empty() {
            msg = format!("{msg}: {detail}");
        }
    }
    Some(ApplyEvent { kind: kind.clone(), message: msg, address, block_id, aws_id, error: level == "error" || kind == "apply_errored" })
}

/// `tofu output -json` as label -> value text
pub fn parse_outputs(json: &str) -> Vec<(String, String)> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return vec![] };
    let mut out: Vec<(String, String)> = v.as_object().map(|m| m.iter().map(|(k, o)| (k.clone(), match &o["value"] { Value::String(s) => s.clone(), other => other.to_string() })).collect()).unwrap_or_default();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;

    #[test]
    fn plan_json_maps_to_blocks_and_costs() {
        let d = templates::build("two_tier").unwrap();
        let nat_a = d.by_label("nat_a").unwrap().id.clone();
        let show = serde_json::json!({
            "format_version": "1.2",
            "resource_changes": [
                {"address": "aws_vpc.main", "mode": "managed", "type": "aws_vpc", "name": "main", "change": {"actions": ["create"], "after": {"tags": {"design-block": "main"}}}},
                {"address": "aws_subnet.public_a", "mode": "managed", "type": "aws_subnet", "name": "public_a", "change": {"actions": ["create"], "after": {"tags": {"design-block": "public_a"}}}},
                {"address": "aws_eip.nat_a", "mode": "managed", "type": "aws_eip", "name": "nat_a", "change": {"actions": ["create"], "after": {"tags": {"design-block": "nat_a"}}}},
                {"address": "aws_nat_gateway.nat_a", "mode": "managed", "type": "aws_nat_gateway", "name": "nat_a", "change": {"actions": ["create"], "after": {"tags": {"design-block": "nat_a"}}}},
                {"address": "aws_route.public_0", "mode": "managed", "type": "aws_route", "name": "public_0", "change": {"actions": ["update"], "after": {}}},
                {"address": "aws_nat_gateway.nat_b", "mode": "managed", "type": "aws_nat_gateway", "name": "nat_b", "change": {"actions": ["delete"], "before": {"tags": {"design-block": "nat_b"}}}},
                {"address": "aws_subnet.app_a", "mode": "managed", "type": "aws_subnet", "name": "app_a", "change": {"actions": ["delete", "create"], "replace_paths": [["cidr_block"]], "after": {"tags": {"design-block": "app_a"}}}},
                {"address": "aws_internet_gateway.igw", "mode": "managed", "type": "aws_internet_gateway", "name": "igw", "change": {"actions": ["no-op"], "after": {}}},
                {"address": "data.aws_ami.al2023_arm64", "mode": "data", "type": "aws_ami", "name": "al2023_arm64", "change": {"actions": ["read"]}}
            ]
        }).to_string();
        let p = parse_plan(&d, &show).unwrap();
        assert_eq!((p.creates, p.updates, p.replaces, p.destroys), (4, 1, 1, 1));
        assert_eq!(p.by_block[&nat_a], "create");
        assert_eq!(p.by_block[&d.by_label("public").unwrap().id], "update", "aws_route.public_0 belongs to route table public");
        assert_eq!(p.by_block[&d.by_label("app_a").unwrap().id], "replace");
        assert_eq!(p.by_block[&d.by_label("nat_b").unwrap().id], "destroy");
        assert!(!p.by_block.contains_key(&d.by_label("igw").unwrap().id), "no-op blocks are not marked");
        let nat = 0.056 * 730.0;
        assert!((p.cost_delta - (nat - nat)).abs() < 0.01, "one NAT comes, one goes: {}", p.cost_delta);
        assert!(p.summary.starts_with("create 4, change 1, replace 1, destroy 1"), "{}", p.summary);
        let app_a = p.changes.iter().find(|c| c.name == "app_a").unwrap();
        assert_eq!(app_a.replace_because, ["cidr_block"]);
        assert!(!p.changes.iter().any(|c| c.action == "no-op" || c.resource_type == "aws_ami"));
        let empty = parse_plan(&d, r#"{"resource_changes": []}"#).unwrap();
        assert!(empty.empty && empty.summary.contains("No changes"));
        assert!(parse_plan(&d, "not json").is_err());
    }

    #[test]
    fn apply_lines_name_the_block() {
        let d = templates::build("single_public").unwrap();
        let line = r#"{"@level":"info","@message":"aws_vpc.main: Creation complete after 2s [id=vpc-0abc]","type":"apply_complete","hook":{"resource":{"addr":"aws_vpc.main","resource_type":"aws_vpc","resource_name":"main"},"action":"create","id_key":"id","id_value":"vpc-0abc","elapsed_seconds":2}}"#;
        let e = parse_apply_line(&d, line).unwrap();
        assert_eq!(e.kind, "apply_complete");
        assert_eq!(e.block_id.as_deref(), Some(d.by_label("main").unwrap().id.as_str()));
        assert_eq!(e.aws_id.as_deref(), Some("vpc-0abc"));
        assert!(!e.error);
        let err = parse_apply_line(&d, r#"{"@level":"error","@message":"Error: creating EC2 VPC","type":"diagnostic","diagnostic":{"severity":"error","summary":"creating EC2 VPC","detail":"UnauthorizedOperation: You are not authorized","address":"aws_vpc.main"}}"#).unwrap();
        assert!(err.error && err.message.contains("UnauthorizedOperation"));
        let plain = parse_apply_line(&d, "Plan: 5 to add, 0 to change, 0 to destroy.").unwrap();
        assert_eq!(plain.kind, "log");
        assert!(parse_apply_line(&d, "   ").is_none());
        let route = parse_apply_line(&d, r#"{"@message":"aws_route.public_0: Creating...","type":"apply_start","hook":{"resource":{"addr":"aws_route.public_0","resource_name":"public_0"}}}"#).unwrap();
        assert_eq!(route.block_id.as_deref(), Some(d.by_label("public").unwrap().id.as_str()));
        let outs = parse_outputs(r#"{"vpc_main_id":{"sensitive":false,"type":"string","value":"vpc-0abc"},"subnet_ids":{"value":{"public_a":"subnet-1"}}}"#);
        assert_eq!(outs[1], ("vpc_main_id".to_string(), "vpc-0abc".to_string()));
        assert!(outs[0].1.contains("subnet-1"));
    }
}
