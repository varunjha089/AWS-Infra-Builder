//! Import: what an AWS account has (read with Describe* calls by the app) becomes a design. This module only maps an `Inventory`
//! (plain structs filled by the app, or by a test) to blocks and arrows, so it is tested without an account.

use crate::model::{arrange, az_letters, clean_label, Design, Edge, Node, Params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VpcInfo {
    pub id: String,
    pub name: Option<String>,
    pub cidr: String,
    pub is_default: bool,
    /// how many subnets, for the list before the import
    pub subnets: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubnetInfo {
    pub id: String,
    pub name: Option<String>,
    pub cidr: String,
    /// "ap-south-1a"
    pub az: String,
    pub map_public_ip: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NatInfo {
    pub id: String,
    pub name: Option<String>,
    pub subnet_id: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteInfo {
    pub dest: String,
    /// "igw-…", "nat-…", "local", "pcx-…", "vpce-…", "eni-…", "tgw-…"
    pub target: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteTableInfo {
    pub id: String,
    pub name: Option<String>,
    pub main: bool,
    pub subnet_ids: Vec<String>,
    pub routes: Vec<RouteInfo>,
}

/// one permission of a security group: a protocol and port range from (or to) address ranges and groups
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermInfo {
    /// "tcp", "udp", "icmp" or "-1"
    pub proto: String,
    pub from: i32,
    pub to: i32,
    pub cidrs: Vec<String>,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SgInfo {
    pub id: String,
    pub name: Option<String>,
    pub group_name: String,
    pub description: String,
    pub ingress: Vec<PermInfo>,
    pub egress: Vec<PermInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstanceInfo {
    pub id: String,
    pub name: Option<String>,
    pub instance_type: String,
    pub subnet_id: String,
    pub sgs: Vec<String>,
    pub public_ip: bool,
    pub key: Option<String>,
    pub state: String,
    /// the AMI's name, to guess the OS
    pub image_name: Option<String>,
    #[serde(default)]
    pub spot: bool,
    /// the root volume in GiB, when known
    #[serde(default)]
    pub disk: Option<u64>,
    /// the public IPv4 address (or DNS name), for the Terminal tab
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub private_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Inventory {
    pub region: String,
    pub account: String,
    pub vpc: VpcInfo,
    pub subnets: Vec<SubnetInfo>,
    pub igws: Vec<(String, Option<String>)>,
    pub nats: Vec<NatInfo>,
    pub route_tables: Vec<RouteTableInfo>,
    #[serde(default)]
    pub sgs: Vec<SgInfo>,
    #[serde(default)]
    pub instances: Vec<InstanceInfo>,
    #[serde(default)]
    pub endpoints: Vec<EndpointInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EndpointInfo {
    pub id: String,
    pub name: Option<String>,
    /// "com.amazonaws.<region>.s3"
    pub service_name: String,
    /// "Gateway" | "Interface"
    pub endpoint_type: String,
    pub route_table_ids: Vec<String>,
    pub subnet_ids: Vec<String>,
    pub sg_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Imported {
    pub design: Design,
    /// what the design cannot show (yet) and what was left out, in plain words
    pub notes: Vec<String>,
}

fn label_for(name: &Option<String>, id: &str, fallback: &str) -> String {
    let base = name.as_deref().filter(|s| !s.trim().is_empty()).map(clean_label).unwrap_or_else(|| {
        let short: String = id.rsplit('-').next().unwrap_or(id).chars().take(6).collect();
        format!("{fallback}_{short}")
    });
    base.to_lowercase()
}

/// the inventory as a design: subnets in their AZ (the design gets as many AZs as the VPC's subnets span), tiers from the routes
/// (a table with 0.0.0.0/0 → igw makes its subnets public, → nat private, else isolated), NAT Gateways inside their subnet, route
/// tables with routes to gateways the canvas has
pub fn to_design(inv: &Inventory) -> Imported {
    let mut d = Design { name: inv.vpc.name.clone().unwrap_or_else(|| inv.vpc.id.clone()), region: inv.region.clone(), ..Default::default() };
    let mut notes = vec![];
    let mut next = 1;
    let mut id = || { let s = format!("n{next}"); next += 1; s };
    let mut used: Vec<String> = vec![];
    let mut unique = |want: String| {
        let mut l = want.clone();
        let mut k = 1;
        while used.contains(&l) { k += 1; l = format!("{want}{k}"); }
        used.push(l.clone());
        l
    };

    // AZ letters: the VPC's subnets span a…; letters follow the region name (ap-south-1a -> a)
    let mut letters: Vec<String> = inv.subnets.iter().filter_map(|s| s.az.strip_prefix(&inv.region).map(String::from)).collect();
    letters.sort();
    letters.dedup();
    let max_letter = letters.iter().filter_map(|l| l.chars().next()).max().unwrap_or('a');
    d.azs = ((max_letter as u8).saturating_sub(b'a') + 1).clamp(1, 6);
    let az_ok = az_letters(d.azs);

    let vpc_id = id();
    d.nodes.push(Node { id: vpc_id.clone(), kind: "vpc".into(), label: unique(label_for(&inv.vpc.name, &inv.vpc.id, "vpc")), params: obj(json!({"cidr": inv.vpc.cidr, "aws_id": inv.vpc.id})), parent: None, x: 0.0, y: 0.0, w: 0.0, h: 0.0 });

    // the route table of each subnet (explicit association, else the main table)
    let main = inv.route_tables.iter().find(|r| r.main);
    let table_of = |sid: &str| inv.route_tables.iter().find(|r| r.subnet_ids.iter().any(|s| s == sid)).or(main);
    let tier_of = |rt: Option<&RouteTableInfo>| -> &str {
        let Some(rt) = rt else { return "isolated" };
        let def: Vec<&RouteInfo> = rt.routes.iter().filter(|r| r.dest == "0.0.0.0/0").collect();
        if def.iter().any(|r| r.target.starts_with("igw-")) { "public" } else if def.iter().any(|r| r.target.starts_with("nat-")) { "private" } else { "isolated" }
    };

    let mut subnet_node: HashMap<String, String> = HashMap::new();
    for s in &inv.subnets {
        let letter = s.az.strip_prefix(&inv.region).unwrap_or("a").to_string();
        if !az_ok.contains(&letter) {
            notes.push(format!("Subnet {} is in {}, which the design cannot show; it is drawn in AZ a.", s.id, s.az));
        }
        let nid = id();
        subnet_node.insert(s.id.clone(), nid.clone());
        d.nodes.push(Node { id: nid, kind: "subnet".into(), label: unique(label_for(&s.name, &s.id, "subnet")),
            params: obj(json!({"cidr": s.cidr, "az": if az_ok.contains(&letter) { letter } else { "a".into() }, "tier": tier_of(table_of(&s.id)), "aws_id": s.id, "map_public_ip": s.map_public_ip})),
            parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    }
    let mut gw_node: HashMap<String, String> = HashMap::new();
    for (gid, name) in &inv.igws {
        let nid = id();
        gw_node.insert(gid.clone(), nid.clone());
        d.nodes.push(Node { id: nid, kind: "igw".into(), label: unique(label_for(name, gid, "igw")), params: obj(json!({"aws_id": gid})), parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    }
    for n in &inv.nats {
        if n.state == "deleted" || n.state == "deleting" {
            continue;
        }
        let Some(parent) = subnet_node.get(&n.subnet_id) else { notes.push(format!("NAT Gateway {} is in a subnet the VPC does not list; left out.", n.id)); continue };
        let nid = id();
        gw_node.insert(n.id.clone(), nid.clone());
        d.nodes.push(Node { id: nid, kind: "nat".into(), label: unique(label_for(&n.name, &n.id, "nat")), params: obj(json!({"aws_id": n.id})), parent: Some(parent.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    }
    let mut en = 1;
    for rt in &inv.route_tables {
        // the main table is drawn only when a subnet uses it implicitly and it has a route beyond local
        let implicit: Vec<&SubnetInfo> = if rt.main { inv.subnets.iter().filter(|s| !inv.route_tables.iter().any(|r| r.subnet_ids.contains(&s.id))).collect() } else { vec![] };
        let used_by: Vec<String> = rt.subnet_ids.iter().chain(implicit.iter().map(|s| &s.id)).filter_map(|s| subnet_node.get(s).cloned()).collect();
        let routes: Vec<&RouteInfo> = rt.routes.iter().filter(|r| r.target != "local").collect();
        if rt.main && used_by.is_empty() && routes.is_empty() {
            continue;
        }
        let nid = id();
        let fallback = if rt.main { "main_rt" } else { "rt" };
        d.nodes.push(Node { id: nid.clone(), kind: "rt".into(), label: unique(label_for(&rt.name, &rt.id, fallback)), params: obj(json!({"aws_id": rt.id, "main": rt.main})), parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        for s in used_by {
            d.edges.push(Edge { id: format!("e{en}"), source: s, target: nid.clone(), kind: "attach".into(), params: Params::new() });
            en += 1;
        }
        if rt.main && !implicit.is_empty() {
            notes.push(format!("{} subnet{} had no route table of their own and use the VPC's main table ({}); the design shows that as an attachment.", implicit.len(), if implicit.len() == 1 { "" } else { "s" }, rt.id));
        }
        for r in routes {
            match gw_node.get(&r.target) {
                Some(t) => {
                    d.edges.push(Edge { id: format!("e{en}"), source: nid.clone(), target: t.clone(), kind: "route".into(), params: obj(json!({"dest": r.dest})) });
                    en += 1;
                }
                None => notes.push(format!("Route {} → {} in {} goes to something the canvas cannot show yet (peering, endpoint, instance, transit gateway); left out.", r.dest, r.target, rt.name.clone().unwrap_or(rt.id.clone()))),
            }
        }
    }
    // security groups: the group, then its permissions as traffic arrows (address ranges become cidr blocks, one per range)
    let mut sg_node: HashMap<String, String> = HashMap::new();
    for g in &inv.sgs {
        let nid = id();
        sg_node.insert(g.id.clone(), nid.clone());
        let egress_all = g.egress.len() == 1 && g.egress[0].proto == "-1" && g.egress[0].cidrs.iter().any(|c| c == "0.0.0.0/0") && g.egress[0].groups.is_empty();
        let want = if g.group_name == "default" { "default".to_string() } else { g.name.clone().unwrap_or_else(|| g.group_name.clone()) };
        d.nodes.push(Node { id: nid, kind: "sg".into(), label: unique(label_for(&Some(want), &g.id, "sg")),
            params: obj(json!({"egress": if egress_all { "all" } else { "rules" }, "description": g.description, "aws_id": g.id})), parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    }
    let mut cidr_node: HashMap<String, String> = HashMap::new();
    let mut cidr_block = |d: &mut Design, c: &str, unique: &mut dyn FnMut(String) -> String, id: &mut dyn FnMut() -> String| -> String {
        if let Some(n) = cidr_node.get(c) {
            return n.clone();
        }
        let nid = id();
        let want = if c == "0.0.0.0/0" { "internet".to_string() } else { format!("net_{}", c.replace(['.', '/'], "_")) };
        d.nodes.push(Node { id: nid.clone(), kind: "cidr".into(), label: unique(want), params: obj(json!({"cidr": c})), parent: None, x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        cidr_node.insert(c.to_string(), nid.clone());
        nid
    };
    let ports_of = |p: &PermInfo| -> (String, String) {
        match p.proto.as_str() {
            "-1" => ("all".into(), String::new()),
            "icmp" | "icmpv6" => ("icmp".into(), String::new()),
            proto => (proto.to_string(), if p.from <= 0 && p.to >= 65535 { "all".into() } else if p.from == p.to { p.from.to_string() } else { format!("{}-{}", p.from, p.to) }),
        }
    };
    for g in &inv.sgs {
        let me = sg_node[&g.id].clone();
        let egress_all = d.node(&me).map(|n| n.params.get("egress").and_then(|v| v.as_str()) == Some("all")).unwrap_or(true);
        // one arrow stands for both ends of a rule: an egress permission that mirrors an ingress one already drawn is not drawn again
        let mut push = |d: &mut Design, src: String, tgt: String, p: &PermInfo| {
            let (proto, ports) = ports_of(p);
            if d.edges.iter().any(|e| e.kind == "traffic" && e.source == src && e.target == tgt && e.params.get("proto").and_then(|v| v.as_str()) == Some(&proto) && e.params.get("ports").and_then(|v| v.as_str()) == Some(&ports)) {
                return;
            }
            d.edges.push(Edge { id: format!("e{en}"), source: src, target: tgt, kind: "traffic".into(), params: obj(json!({"proto": proto, "ports": ports})) });
            en += 1;
        };
        for p in &g.ingress {
            for c in &p.cidrs {
                let src = cidr_block(&mut d, c, &mut unique, &mut id);
                push(&mut d, src, me.clone(), p);
            }
            for other in &p.groups {
                match sg_node.get(other) {
                    Some(src) => push(&mut d, src.clone(), me.clone(), p),
                    None => notes.push(format!("{}: a rule references group {other}, which is not in this VPC; left out.", g.group_name)),
                }
            }
        }
        if !egress_all {
            for p in &g.egress {
                for c in &p.cidrs {
                    let tgt = cidr_block(&mut d, c, &mut unique, &mut id);
                    push(&mut d, me.clone(), tgt, p);
                }
                for other in &p.groups {
                    if let Some(tgt) = sg_node.get(other) {
                        push(&mut d, me.clone(), tgt.clone(), p);
                    }
                }
            }
        }
    }
    // instances (running or stopped; terminated ones are gone)
    for i in inv.instances.iter().filter(|i| i.state != "terminated" && i.state != "shutting-down") {
        let Some(parent) = subnet_node.get(&i.subnet_id) else { notes.push(format!("Instance {} is in a subnet the VPC does not list; left out.", i.id)); continue };
        let known = crate::model::instance_type(&i.instance_type).is_some();
        if !known {
            notes.push(format!("Instance {}: type {} is not in the app's table; its cost is shown as 0 until the table knows it.", i.name.clone().unwrap_or(i.id.clone()), i.instance_type));
        }
        let image = i.image_name.clone().unwrap_or_default().to_lowercase();
        let os = if image.contains("ubuntu") { "ubuntu" } else if image.contains("debian") { "debian" } else { "al2023" };
        let sgs: Vec<String> = i.sgs.iter().filter_map(|g| sg_node.get(g).cloned()).collect();
        let nid = id();
        d.nodes.push(Node { id: nid, kind: "ec2".into(), label: unique(label_for(&i.name, &i.id, "instance")),
            params: obj(json!({"type": i.instance_type, "os": os, "key": i.key.clone().unwrap_or_default(), "public_ip": i.public_ip, "sgs": sgs, "aws_id": i.id, "state": i.state, "spot": i.spot, "disk": i.disk.unwrap_or(8), "address": i.address.clone().unwrap_or_default(), "private_ip": i.private_ip.clone().unwrap_or_default()})),
            parent: Some(parent.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        if i.state == "stopped" {
            notes.push(format!("Instance {} is stopped: it costs only its volume and public address until started.", i.name.clone().unwrap_or(i.id.clone())));
        }
    }
    // VPC endpoints: gateway ones get a route arrow from each table; interface ones take the tier of their first subnet
    let rt_nodes: HashMap<String, String> = d.nodes.iter().filter(|n| n.kind == "rt").filter_map(|n| Some((n.params.get("aws_id")?.as_str()?.to_string(), n.id.clone()))).collect();
    let rt_node_of = |rt_id: &str| rt_nodes.get(rt_id).cloned();
    for epi in &inv.endpoints {
        let svc = epi.service_name.rsplit('.').next().unwrap_or("").to_string();
        let svc = if epi.service_name.ends_with(".ecr.api") { "ecr.api".to_string() } else if epi.service_name.ends_with(".ecr.dkr") { "ecr.dkr".to_string() } else { svc };
        if crate::model::endpoint_type(&svc).is_none() {
            notes.push(format!("VPC endpoint {} ({}) is for a service the canvas does not know; left out.", epi.id, epi.service_name));
            continue;
        }
        let tier = epi.subnet_ids.first().and_then(|s| subnet_node.get(s)).and_then(|n| d.node(n)).and_then(|n| n.params.get("tier").and_then(|v| v.as_str()).map(String::from)).unwrap_or_else(|| "private".into());
        let sg = epi.sg_ids.first().and_then(|g| sg_node.get(g)).cloned().unwrap_or_default();
        let nid = id();
        d.nodes.push(Node { id: nid.clone(), kind: "vpce".into(), label: unique(label_for(&epi.name, &epi.id, &format!("{}_ep", svc.replace('.', "_")))),
            params: obj(json!({"service": svc, "tier": tier, "sg": sg, "aws_id": epi.id})), parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        if epi.endpoint_type.eq_ignore_ascii_case("gateway") {
            for rt in &epi.route_table_ids {
                if let Some(src) = rt_node_of(rt) {
                    d.edges.push(Edge { id: format!("e{en}"), source: src, target: nid.clone(), kind: "route".into(), params: obj(json!({"dest": format!("pl-{svc}")})) });
                    en += 1;
                }
            }
        }
    }
    arrange(&mut d);
    crate::dsl::sync(&mut d);
    notes.insert(0, format!("Imported read-only from account {} in {}: {} subnets, {} Internet Gateway{}, {} NAT Gateway{}, {} route tables, {} security groups, {} instances. Nothing in the account was changed.",
        inv.account, inv.region, inv.subnets.len(), inv.igws.len(), if inv.igws.len() == 1 { "" } else { "s" }, inv.nats.len(), if inv.nats.len() == 1 { "" } else { "s" }, inv.route_tables.len(), inv.sgs.len(), inv.instances.len()));
    Imported { design: d, notes }
}

fn obj(v: serde_json::Value) -> Params {
    v.as_object().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::analyse;

    fn inventory() -> Inventory {
        Inventory {
            region: "ap-south-1".into(),
            account: "123456789012".into(),
            vpc: VpcInfo { id: "vpc-0abc".into(), name: Some("Prod VPC".into()), cidr: "10.0.0.0/16".into(), is_default: false, subnets: 4 },
            subnets: vec![
                SubnetInfo { id: "subnet-1".into(), name: Some("public-a".into()), cidr: "10.0.0.0/20".into(), az: "ap-south-1a".into(), map_public_ip: true },
                SubnetInfo { id: "subnet-2".into(), name: Some("public-b".into()), cidr: "10.0.16.0/20".into(), az: "ap-south-1b".into(), map_public_ip: true },
                SubnetInfo { id: "subnet-3".into(), name: None, cidr: "10.0.32.0/20".into(), az: "ap-south-1a".into(), map_public_ip: false },
                SubnetInfo { id: "subnet-4".into(), name: Some("db".into()), cidr: "10.0.64.0/24".into(), az: "ap-south-1c".into(), map_public_ip: false },
            ],
            igws: vec![("igw-9".into(), None)],
            nats: vec![NatInfo { id: "nat-7".into(), name: Some("nat a".into()), subnet_id: "subnet-1".into(), state: "available".into() },
                       NatInfo { id: "nat-8".into(), name: None, subnet_id: "subnet-2".into(), state: "deleted".into() }],
            sgs: vec![
                SgInfo { id: "sg-web".into(), name: Some("web".into()), group_name: "prod-web".into(), description: "web".into(),
                    ingress: vec![PermInfo { proto: "tcp".into(), from: 443, to: 443, cidrs: vec!["0.0.0.0/0".into()], groups: vec![] }, PermInfo { proto: "tcp".into(), from: 22, to: 22, cidrs: vec!["203.0.113.0/24".into()], groups: vec![] }],
                    egress: vec![PermInfo { proto: "-1".into(), from: 0, to: 0, cidrs: vec!["0.0.0.0/0".into()], groups: vec![] }] },
                SgInfo { id: "sg-db".into(), name: None, group_name: "prod-db".into(), description: String::new(),
                    ingress: vec![PermInfo { proto: "tcp".into(), from: 5432, to: 5432, cidrs: vec![], groups: vec!["sg-web".into(), "sg-gone".into()] }],
                    egress: vec![PermInfo { proto: "tcp".into(), from: 443, to: 443, cidrs: vec!["0.0.0.0/0".into()], groups: vec![] }] },
                // a pair of groups whose egress and ingress mirror each other: drawn once
                SgInfo { id: "sg-out".into(), name: None, group_name: "nfs-out".into(), description: String::new(), ingress: vec![],
                    egress: vec![PermInfo { proto: "tcp".into(), from: 2049, to: 2049, cidrs: vec![], groups: vec!["sg-in".into()] }] },
                SgInfo { id: "sg-in".into(), name: None, group_name: "nfs-in".into(), description: String::new(),
                    ingress: vec![PermInfo { proto: "tcp".into(), from: 2049, to: 2049, cidrs: vec![], groups: vec!["sg-out".into()] }],
                    egress: vec![PermInfo { proto: "-1".into(), from: 0, to: 0, cidrs: vec!["0.0.0.0/0".into()], groups: vec![] }] },
            ],
            instances: vec![
                InstanceInfo { id: "i-1".into(), name: Some("web 1".into()), instance_type: "t3.micro".into(), subnet_id: "subnet-1".into(), sgs: vec!["sg-web".into()], public_ip: true, key: Some("lab".into()), state: "running".into(), image_name: Some("ubuntu/images/hvm-ssd/ubuntu-jammy".into()), spot: true, disk: Some(50), address: Some("3.7.1.2".into()), private_ip: Some("10.0.0.5".into()) },
                InstanceInfo { id: "i-2".into(), name: None, instance_type: "x9.odd".into(), subnet_id: "subnet-3".into(), sgs: vec!["sg-db".into()], public_ip: false, key: None, state: "stopped".into(), image_name: None, spot: false, disk: None, address: None, private_ip: None },
                InstanceInfo { id: "i-3".into(), name: None, instance_type: "t3.micro".into(), subnet_id: "subnet-1".into(), sgs: vec![], public_ip: false, key: None, state: "terminated".into(), image_name: None, spot: false, disk: None, address: None, private_ip: None },
            ],
            endpoints: vec![
                EndpointInfo { id: "vpce-1".into(), name: Some("s3 endpoint".into()), service_name: "com.amazonaws.ap-south-1.s3".into(), endpoint_type: "Gateway".into(), route_table_ids: vec!["rtb-app".into()], subnet_ids: vec![], sg_ids: vec![] },
                EndpointInfo { id: "vpce-2".into(), name: None, service_name: "com.amazonaws.ap-south-1.ssm".into(), endpoint_type: "Interface".into(), route_table_ids: vec![], subnet_ids: vec!["subnet-3".into()], sg_ids: vec!["sg-db".into()] },
            ],
            route_tables: vec![
                RouteTableInfo { id: "rtb-main".into(), name: None, main: true, subnet_ids: vec![], routes: vec![RouteInfo { dest: "10.0.0.0/16".into(), target: "local".into(), state: "active".into() }] },
                RouteTableInfo { id: "rtb-pub".into(), name: Some("public".into()), main: false, subnet_ids: vec!["subnet-1".into(), "subnet-2".into()],
                    routes: vec![RouteInfo { dest: "10.0.0.0/16".into(), target: "local".into(), state: "active".into() }, RouteInfo { dest: "0.0.0.0/0".into(), target: "igw-9".into(), state: "active".into() }] },
                RouteTableInfo { id: "rtb-app".into(), name: Some("app".into()), main: false, subnet_ids: vec!["subnet-3".into()],
                    routes: vec![RouteInfo { dest: "0.0.0.0/0".into(), target: "nat-7".into(), state: "active".into() }, RouteInfo { dest: "10.9.0.0/16".into(), target: "pcx-1".into(), state: "active".into() }] },
            ],
        }
    }

    #[test]
    fn inventory_becomes_a_checked_design() {
        let im = to_design(&inventory());
        let d = &im.design;
        assert_eq!(d.name, "Prod VPC");
        assert_eq!(d.azs, 3, "subnets span a, b, c");
        let tier = |l: &str| d.by_label(l).unwrap().params["tier"].as_str().unwrap().to_string();
        assert_eq!(tier("public_a"), "public");
        assert_eq!(tier("public_b"), "public");
        assert_eq!(tier("subnet_3"), "private");
        assert_eq!(tier("db"), "isolated", "the main table has only the local route");
        assert_eq!(d.by_label("db").unwrap().params["az"], "c");
        assert!(d.by_label("nat_a").is_some());
        assert!(d.nodes.iter().filter(|n| n.kind == "nat").count() == 1, "the deleted NAT is left out");
        assert_eq!(d.by_label("nat_a").unwrap().parent.as_deref(), Some(d.by_label("public_a").unwrap().id.as_str()));
        // route tables: public, app, and the main one (db uses it implicitly)
        assert_eq!(d.nodes.iter().filter(|n| n.kind == "rt").count(), 3);
        assert_eq!(d.edges.iter().filter(|e| e.kind == "attach").count(), 4);
        assert_eq!(d.edges.iter().filter(|e| e.kind == "route").count(), 3, "igw, nat and the S3 endpoint; the peering route is left out with a note");
        assert!(im.notes.iter().any(|n| n.contains("pcx-1")));
        assert!(im.notes.iter().any(|n| n.contains("main table")));
        assert!(im.notes[0].contains("123456789012"));
        // security groups and instances
        assert_eq!(d.by_label("web").unwrap().params["egress"], "all");
        assert_eq!(d.by_label("prod_db").unwrap().params["egress"], "rules");
        assert!(d.by_label("internet").is_some() && d.by_label("net_203_0_113_0_24").is_some());
        let traffic: Vec<_> = d.edges.iter().filter(|e| e.kind == "traffic").collect();
        assert_eq!(traffic.len(), 5, "443 + ssh in to web, web -> db 5432, db out 443, nfs-out -> nfs-in once: {traffic:?}");
        assert!(im.notes.iter().any(|n| n.contains("sg-gone")));
        let web1 = d.by_label("web_1").unwrap();
        assert_eq!(web1.params["os"], "ubuntu");
        assert_eq!(web1.params["public_ip"], true);
        assert_eq!(web1.params["address"], "3.7.1.2");
        assert_eq!(web1.params["key"], "lab");
        assert_eq!(web1.params["spot"], true);
        assert_eq!(web1.params["disk"], 50);
        assert_eq!(web1.params["sgs"].as_array().unwrap().len(), 1);
        assert_eq!(d.nodes.iter().filter(|n| n.kind == "ec2").count(), 2, "the terminated one is gone");
        assert!(im.notes.iter().any(|n| n.contains("x9.odd")));
        assert!(im.notes.iter().any(|n| n.contains("is stopped")));
        assert!(d.text.as_ref().unwrap().contains("rule internet -> web tcp 443"), "{}", d.text.as_ref().unwrap());
        assert!(d.text.as_ref().unwrap().contains("rt app in prod_vpc: 0.0.0.0/0 -> nat_a, pl-s3 -> s3_endpoint"), "{}", d.text.as_ref().unwrap());
        assert!(d.text.as_ref().unwrap().contains("vpce ssm_ep_2 ssm in prod_vpc tier private sg prod_db"), "{}", d.text.as_ref().unwrap());
        let a = analyse(d);
        // the unknown type is an error on its block; everything else is fine
        assert_eq!(a.errors, 1, "{:?}", a.problems);
        assert!(a.problems.iter().any(|p| p.contains("x9.odd")));
        assert!(d.text.as_ref().unwrap().contains("subnet public_a public 10.0.0.0/20 az a in prod_vpc rt public"), "{}", d.text.as_ref().unwrap());
        assert_eq!(d.by_label("prod_vpc").unwrap().params["aws_id"], "vpc-0abc");
    }
}
