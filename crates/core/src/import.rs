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
    /// the Elastic IP's allocation id (for the import block of aws_eip)
    #[serde(default)]
    pub allocation_id: Option<String>,
}

/// one security group rule as AWS lists it (ec2:DescribeSecurityGroupRules): its id is what an import block needs
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SgRuleInfo {
    pub id: String,
    pub group_id: String,
    pub egress: bool,
    pub proto: String,
    pub from: i32,
    pub to: i32,
    pub cidr: Option<String>,
    pub group: Option<String>,
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
    /// the AMI it runs: an adopted instance plans against this, not the newest image by name
    #[serde(default)]
    pub image_id: Option<String>,
    /// the IAM instance profile's name, kept on an adopted instance
    #[serde(default)]
    pub iam_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListenerInfo {
    pub arn: String,
    pub port: i32,
    pub protocol: String,
    pub cert: Option<String>,
    pub target_group_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetGroupInfo {
    pub arn: String,
    pub name: String,
    pub port: i32,
    pub protocol: String,
    pub health_path: String,
    pub instance_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LbInfo {
    pub arn: String,
    pub name: String,
    /// "internet-facing" | "internal"
    pub scheme: String,
    /// "application" | "network" | "gateway"
    pub lb_type: String,
    pub subnet_ids: Vec<String>,
    pub sg_ids: Vec<String>,
    pub listeners: Vec<ListenerInfo>,
    pub target_groups: Vec<TargetGroupInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AsgInfo {
    pub name: String,
    pub min: i32,
    pub max: i32,
    pub desired: i32,
    pub subnet_ids: Vec<String>,
    pub launch_template_id: Option<String>,
    pub launch_template_name: Option<String>,
    pub instance_type: Option<String>,
    pub image_id: Option<String>,
    pub key: Option<String>,
    pub sg_ids: Vec<String>,
    pub spot: bool,
    pub target_group_arns: Vec<String>,
}

/// something in the VPC the palette has no block for: drawn grey and read-only
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OtherInfo {
    pub id: String,
    /// a short kind: "efs_mount_target", "rds", "lambda", "ecs_task", "elasticache", "network_interface", "peering", "transit_gateway" …
    pub what: String,
    pub name: Option<String>,
    pub subnet_id: Option<String>,
    pub note: String,
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
    pub sg_rules: Vec<SgRuleInfo>,
    #[serde(default)]
    pub lbs: Vec<LbInfo>,
    #[serde(default)]
    pub asgs: Vec<AsgInfo>,
    #[serde(default)]
    pub others: Vec<OtherInfo>,
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
        d.nodes.push(Node { id: nid, kind: "nat".into(), label: unique(label_for(&n.name, &n.id, "nat")), params: obj(json!({"aws_id": n.id, "eip_alloc": n.allocation_id.clone().unwrap_or_default()})), parent: Some(parent.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    }
    // what a route may point at that the palette has no block for: a grey block each, so the route is drawn, not lost
    for rt in &inv.route_tables {
        let unknown: Vec<&RouteInfo> = rt.routes.iter().filter(|r| r.target != "local" && !gw_node.contains_key(&r.target)).collect();
        for r in unknown {
            if gw_node.contains_key(&r.target) {
                continue;
            }
            let what = match r.target.split('-').next().unwrap_or("") { "pcx" => "peering", "tgw" => "transit_gateway", "eni" => "network_interface", "i" => "instance", "eigw" => "egress_only_igw", "vgw" => "vpn_gateway", "vpce" => "vpc_endpoint", _ => "resource" };
            if r.target == "?" || (what == "vpc_endpoint" && inv.endpoints.iter().any(|e| e.id == r.target)) {
                continue;
            }
            let nid = id();
            gw_node.insert(r.target.clone(), nid.clone());
            d.nodes.push(Node { id: nid, kind: "other".into(), label: unique(format!("{what}_{}", r.target.rsplit('-').next().unwrap_or("x"))),
                params: obj(json!({"what": what, "aws_id": r.target, "subnet": "", "note": format!("a route points at it ({})", r.target)})), parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        }
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
            let is_implicit = implicit.iter().any(|x| subnet_node.get(&x.id) == Some(&s));
            d.edges.push(Edge { id: format!("e{en}"), source: s, target: nid.clone(), kind: "attach".into(), params: if is_implicit { obj(json!({"implicit": true})) } else { Params::new() } });
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
            params: obj(json!({"egress": if egress_all { "all" } else { "rules" }, "description": g.description, "aws_id": g.id, "aws_default": g.group_name == "default", "group_name": g.group_name,
                "egress_rule_id": inv.sg_rules.iter().find(|r| r.group_id == g.id && r.egress && r.proto == "-1" && r.cidr.as_deref() == Some("0.0.0.0/0")).map(|r| r.id.clone()).unwrap_or_default()})),
            parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    }
    // the id of one rule, by what it says
    let rule_id = |group: &str, egress: bool, p: &PermInfo, cidr: Option<&str>, other: Option<&str>| -> Option<String> {
        inv.sg_rules.iter().find(|r| r.group_id == group && r.egress == egress && r.proto == p.proto && (r.proto == "-1" || (r.from == p.from && r.to == p.to)) && r.cidr.as_deref() == cidr && r.group.as_deref() == other).map(|r| r.id.clone())
    };
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
        // `rid` is the rule's id in AWS: on the arrow as aws_id (the ingress rule) or aws_id_out (the egress rule at the source)
        let mut push = |d: &mut Design, src: String, tgt: String, p: &PermInfo, rid: Option<String>, egress: bool| {
            let (proto, ports) = ports_of(p);
            let key = if egress { "aws_id_out" } else { "aws_id" };
            if let Some(e) = d.edges.iter_mut().find(|e| e.kind == "traffic" && e.source == src && e.target == tgt && e.params.get("proto").and_then(|v| v.as_str()) == Some(&proto) && e.params.get("ports").and_then(|v| v.as_str()) == Some(&ports)) {
                if let Some(r) = rid { e.params.insert(key.into(), json!(r)); }
                return;
            }
            let mut params = obj(json!({"proto": proto, "ports": ports}));
            if let Some(r) = rid { params.insert(key.into(), json!(r)); }
            d.edges.push(Edge { id: format!("e{en}"), source: src, target: tgt, kind: "traffic".into(), params });
            en += 1;
        };
        for p in &g.ingress {
            for c in &p.cidrs {
                let src = cidr_block(&mut d, c, &mut unique, &mut id);
                let rid = rule_id(&g.id, false, p, Some(c), None);
                push(&mut d, src, me.clone(), p, rid, false);
            }
            for other in &p.groups {
                match sg_node.get(other) {
                    Some(src) => { let rid = rule_id(&g.id, false, p, None, Some(other)); push(&mut d, src.clone(), me.clone(), p, rid, false) }
                    None => notes.push(format!("{}: a rule references group {other}, which is not in this VPC; left out.", g.group_name)),
                }
            }
        }
        if !egress_all {
            for p in &g.egress {
                for c in &p.cidrs {
                    let tgt = cidr_block(&mut d, c, &mut unique, &mut id);
                    let rid = rule_id(&g.id, true, p, Some(c), None);
                    push(&mut d, me.clone(), tgt, p, rid, true);
                }
                for other in &p.groups {
                    if let Some(tgt) = sg_node.get(other) {
                        let rid = rule_id(&g.id, true, p, None, Some(other));
                        push(&mut d, me.clone(), tgt.clone(), p, rid, true);
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
        let os = if image.contains("deep learning base") { "dlami_base" } else if image.contains("deep learning") { "dlami" } else if image.contains("openvpn") { "openvpn" }
            else if image.contains("jammy") || image.contains("22.04") { "ubuntu22" } else if image.contains("ubuntu") { "ubuntu" } else if image.contains("debian") { "debian" } else { "al2023" };
        let sgs: Vec<String> = i.sgs.iter().filter_map(|g| sg_node.get(g).cloned()).collect();
        let nid = id();
        d.nodes.push(Node { id: nid, kind: "ec2".into(), label: unique(label_for(&i.name, &i.id, "instance")),
            params: obj(json!({"type": i.instance_type, "os": os, "key": i.key.clone().unwrap_or_default(), "public_ip": i.public_ip, "sgs": sgs, "aws_id": i.id, "state": i.state, "spot": i.spot, "disk": i.disk.unwrap_or(8), "address": i.address.clone().unwrap_or_default(), "private_ip": i.private_ip.clone().unwrap_or_default(), "image_id": i.image_id.clone().unwrap_or_default(), "iam_profile": i.iam_profile.clone().unwrap_or_default()})),
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
    // the rest: grey, read-only blocks in the VPC (the subnet is named on the block)
    for o in &inv.others {
        let subnet = o.subnet_id.as_ref().and_then(|s| subnet_node.get(s)).and_then(|n| d.node(n)).map(|n| n.label.clone()).unwrap_or_default();
        let nid = id();
        d.nodes.push(Node { id: nid, kind: "other".into(), label: unique(label_for(&o.name, &o.id, &o.what)),
            params: obj(json!({"what": o.what, "aws_id": o.id, "subnet": subnet, "note": o.note})), parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    }
    // Auto Scaling groups: one block each, in the tier of their subnets; their instances are already drawn as instances
    let tier_of_subnets = |ids: &[String], d: &Design| -> String {
        ids.iter().find_map(|s| subnet_node.get(s)).and_then(|n| d.node(n)).and_then(|n| n.params.get("tier").and_then(|v| v.as_str()).map(String::from)).unwrap_or_else(|| "private".into())
    };
    let mut asg_node: HashMap<String, String> = HashMap::new();
    for g in &inv.asgs {
        let tier = tier_of_subnets(&g.subnet_ids, &d);
        let sgs: Vec<String> = g.sg_ids.iter().filter_map(|s| sg_node.get(s).cloned()).collect();
        let t = g.instance_type.clone().unwrap_or_else(|| "t4g.micro".into());
        if g.instance_type.is_none() {
            notes.push(format!("Scaling group {}: its launch template names no instance type (mixed instances?); drawn as t4g.micro.", g.name));
        }
        let nid = id();
        asg_node.insert(g.name.clone(), nid.clone());
        d.nodes.push(Node { id: nid, kind: "asg".into(), label: unique(label_for(&Some(g.name.clone()), &g.name, "asg")),
            params: obj(json!({"type": t, "os": "al2023", "key": g.key.clone().unwrap_or_default(), "tier": tier, "min": g.min.max(0), "max": g.max.max(1), "desired": g.desired.max(0), "sgs": sgs, "spot": g.spot, "disk": 8, "hours": 730,
                "aws_id": g.name, "lt_id": g.launch_template_id.clone().unwrap_or_default(), "lt_name": g.launch_template_name.clone().unwrap_or_default(), "image_id": g.image_id.clone().unwrap_or_default()})),
            parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    }
    // load balancers: one block each, a target arrow per instance or scaling group behind them
    let ec2_node: HashMap<String, String> = d.nodes.iter().filter(|n| n.kind == "ec2").filter_map(|n| Some((n.params.get("aws_id")?.as_str()?.to_string(), n.id.clone()))).collect();
    for lb in &inv.lbs {
        let tier = tier_of_subnets(&lb.subnet_ids, &d);
        let sg = lb.sg_ids.first().and_then(|s| sg_node.get(s)).cloned().unwrap_or_default();
        let listener = lb.listeners.first();
        let tg = listener.and_then(|l| l.target_group_arn.as_ref()).and_then(|a| lb.target_groups.iter().find(|t| &t.arn == a)).or(lb.target_groups.first());
        if lb.lb_type != "application" {
            notes.push(format!("{} is a {} load balancer; the canvas draws it as a load balancer (its listeners keep their ports).", lb.name, lb.lb_type));
        }
        let nid = id();
        d.nodes.push(Node { id: nid.clone(), kind: "alb".into(), label: unique(label_for(&Some(lb.name.clone()), &lb.arn, "lb")),
            params: obj(json!({"scheme": if lb.scheme == "internal" { "internal" } else { "internet" }, "tier": tier, "sg": sg, "port": listener.map(|l| l.port).unwrap_or(80),
                "cert": listener.and_then(|l| l.cert.clone()).unwrap_or_default(), "health": tg.map(|t| t.health_path.clone()).filter(|h| !h.is_empty()).unwrap_or_else(|| "/".into()),
                "aws_id": lb.arn, "lb_name": lb.name, "lb_type": lb.lb_type, "tg_arn": tg.map(|t| t.arn.clone()).unwrap_or_default(), "tg_name": tg.map(|t| t.name.clone()).unwrap_or_default(),
                "listener_arn": listener.map(|l| l.arn.clone()).unwrap_or_default()})),
            parent: Some(vpc_id.clone()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        if let Some(t) = tg {
            for i in &t.instance_ids {
                if let Some(target) = ec2_node.get(i) {
                    d.edges.push(Edge { id: format!("e{en}"), source: nid.clone(), target: target.clone(), kind: "target".into(), params: obj(json!({"port": t.port})) });
                    en += 1;
                }
            }
            for g in inv.asgs.iter().filter(|g| g.target_group_arns.contains(&t.arn)) {
                if let Some(target) = asg_node.get(&g.name) {
                    d.edges.push(Edge { id: format!("e{en}"), source: nid.clone(), target: target.clone(), kind: "target".into(), params: obj(json!({"port": t.port})) });
                    en += 1;
                }
            }
            if lb.target_groups.len() > 1 {
                notes.push(format!("{} has {} target groups; the canvas shows the listener's one.", lb.name, lb.target_groups.len()));
            }
        }
    }
    if !inv.others.is_empty() {
        notes.push(format!("{} thing{} the palette has no block for {} drawn grey and read-only: {}.", inv.others.len(), if inv.others.len() == 1 { "" } else { "s" }, if inv.others.len() == 1 { "is" } else { "are" },
            inv.others.iter().map(|o| format!("{} ({})", o.name.clone().unwrap_or(o.id.clone()), o.what.replace('_', " "))).collect::<Vec<_>>().join(", ")));
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
            lbs: vec![LbInfo { arn: "arn:aws:elasticloadbalancing:ap-south-1:123456789012:loadbalancer/app/web-lb/abc".into(), name: "web-lb".into(), scheme: "internet-facing".into(), lb_type: "application".into(),
                subnet_ids: vec!["subnet-1".into(), "subnet-2".into()], sg_ids: vec!["sg-web".into()],
                listeners: vec![ListenerInfo { arn: "arn:lsn".into(), port: 80, protocol: "HTTP".into(), cert: None, target_group_arn: Some("arn:tg".into()) }],
                target_groups: vec![TargetGroupInfo { arn: "arn:tg".into(), name: "web-tg".into(), port: 8080, protocol: "HTTP".into(), health_path: "/health".into(), instance_ids: vec!["i-1".into()] }] }],
            asgs: vec![AsgInfo { name: "workers".into(), min: 1, max: 3, desired: 2, subnet_ids: vec!["subnet-3".into()], launch_template_id: Some("lt-9".into()), launch_template_name: Some("workers-lt".into()),
                instance_type: Some("t3.small".into()), image_id: Some("ami-0w".into()), key: Some("lab".into()), sg_ids: vec!["sg-db".into()], spot: true, target_group_arns: vec!["arn:tg".into()] }],
            others: vec![OtherInfo { id: "fsmt-1".into(), what: "efs_mount_target".into(), name: None, subnet_id: Some("subnet-3".into()), note: "EFS mount target for fs-1".into() }],
            sg_rules: vec![
                SgRuleInfo { id: "sgr-web443".into(), group_id: "sg-web".into(), egress: false, proto: "tcp".into(), from: 443, to: 443, cidr: Some("0.0.0.0/0".into()), group: None },
                SgRuleInfo { id: "sgr-webout".into(), group_id: "sg-web".into(), egress: true, proto: "-1".into(), from: 0, to: 0, cidr: Some("0.0.0.0/0".into()), group: None },
            ],
            account: "123456789012".into(),
            vpc: VpcInfo { id: "vpc-0abc".into(), name: Some("Prod VPC".into()), cidr: "10.0.0.0/16".into(), is_default: false, subnets: 4 },
            subnets: vec![
                SubnetInfo { id: "subnet-1".into(), name: Some("public-a".into()), cidr: "10.0.0.0/20".into(), az: "ap-south-1a".into(), map_public_ip: true },
                SubnetInfo { id: "subnet-2".into(), name: Some("public-b".into()), cidr: "10.0.16.0/20".into(), az: "ap-south-1b".into(), map_public_ip: true },
                SubnetInfo { id: "subnet-3".into(), name: None, cidr: "10.0.32.0/20".into(), az: "ap-south-1a".into(), map_public_ip: false },
                SubnetInfo { id: "subnet-4".into(), name: Some("db".into()), cidr: "10.0.64.0/24".into(), az: "ap-south-1c".into(), map_public_ip: false },
            ],
            igws: vec![("igw-9".into(), None)],
            nats: vec![NatInfo { id: "nat-7".into(), name: Some("nat a".into()), subnet_id: "subnet-1".into(), state: "available".into(), allocation_id: Some("eipalloc-7".into()) },
                       NatInfo { id: "nat-8".into(), name: None, subnet_id: "subnet-2".into(), state: "deleted".into(), allocation_id: None }],
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
                InstanceInfo { id: "i-1".into(), name: Some("web 1".into()), instance_type: "t3.micro".into(), subnet_id: "subnet-1".into(), sgs: vec!["sg-web".into()], public_ip: true, key: Some("lab".into()), state: "running".into(), image_name: Some("ubuntu/images/hvm-ssd/ubuntu-jammy".into()), spot: true, disk: Some(50), address: Some("3.7.1.2".into()), private_ip: Some("10.0.0.5".into()), image_id: Some("ami-0web".into()), iam_profile: Some("web-role".into()) },
                InstanceInfo { id: "i-2".into(), name: None, instance_type: "x9.odd".into(), subnet_id: "subnet-3".into(), sgs: vec!["sg-db".into()], public_ip: false, key: None, state: "stopped".into(), image_name: None, spot: false, disk: None, address: None, private_ip: None, image_id: None, iam_profile: None },
                InstanceInfo { id: "i-3".into(), name: None, instance_type: "t3.micro".into(), subnet_id: "subnet-1".into(), sgs: vec![], public_ip: false, key: None, state: "terminated".into(), image_name: None, spot: false, disk: None, address: None, private_ip: None, image_id: None, iam_profile: None },
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
        assert_eq!(d.edges.iter().filter(|e| e.kind == "route").count(), 4, "igw, nat, the S3 endpoint and the peering connection (a grey block)");
        let peer = d.nodes.iter().find(|n| n.kind == "other" && n.params["what"] == "peering").expect("the peering connection as a grey block");
        assert_eq!(peer.params["aws_id"], "pcx-1");
        assert!(d.edges.iter().any(|e| e.kind == "route" && e.target == peer.id));
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
        assert_eq!(web1.params["os"], "ubuntu22");
        assert_eq!(web1.params["public_ip"], true);
        assert_eq!(web1.params["address"], "3.7.1.2");
        assert_eq!(web1.params["image_id"], "ami-0web");
        // the ids an adopt needs: the rule's id on its arrow, the allow-all egress rule's id on the group, the EIP allocation
        let web_sg = d.by_label("web").or_else(|| d.nodes.iter().find(|n| n.kind == "sg" && n.params["aws_id"] == "sg-web")).unwrap();
        assert_eq!(web_sg.params["egress_rule_id"], "sgr-webout");
        assert!(d.edges.iter().any(|e| e.kind == "traffic" && e.target == web_sg.id && e.params.get("aws_id").and_then(|v| v.as_str()) == Some("sgr-web443")), "{:?}", d.edges);
        assert_eq!(d.nodes.iter().find(|n| n.kind == "nat").unwrap().params["eip_alloc"], "eipalloc-7");
        // the subnet on the main table by default is an implicit attachment: no association resource, nothing to import
        let main_rt = d.nodes.iter().find(|n| n.kind == "rt" && n.params["main"] == true).unwrap();
        assert!(d.edges.iter().any(|e| e.kind == "attach" && e.target == main_rt.id && e.params.get("implicit") == Some(&json!(true))));
        let files = crate::hcl::generate(d);
        let imports = &files["imports.tf"];
        for s in ["to = aws_vpc.prod_vpc\n  id = \"vpc-0abc\"", "to = aws_vpc_security_group_ingress_rule.", "id = \"sgr-web443\"", "id = \"sgr-webout\"", "to = aws_eip.", "id = \"eipalloc-7\"", "to = aws_instance.web_1\n  id = \"i-1\"", "id = \"subnet-1/rtb-pub\""] {
            assert!(imports.contains(s), "missing {s}\n{imports}");
        }
        assert!(files["main.tf"].contains("ami                         = \"ami-0web\""), "the adopted instance keeps its image");
        assert!(files["main.tf"].contains("iam_instance_profile        = \"web-role\"") && files["main.tf"].contains("key_name                    = \"lab\""), "{}", files["main.tf"]);
        assert!(files["main.tf"].contains("name        = \"web\"") || files["main.tf"].contains("name        = \"prod-web\""), "the group keeps its real name: {}", files["main.tf"]);
        assert!(!files["main.tf"].contains("resource \"aws_route_table_association\" \"db\""), "an implicit attachment writes no association");
        // the balancer, its target arrows, the scaling group and the grey block
        let lb = d.nodes.iter().find(|n| n.kind == "alb").expect("a balancer");
        assert_eq!((lb.params["scheme"].as_str(), lb.params["tier"].as_str(), lb.params["port"].as_i64(), lb.params["health"].as_str()), (Some("internet"), Some("public"), Some(80), Some("/health")));
        let asg = d.nodes.iter().find(|n| n.kind == "asg").expect("a scaling group");
        assert_eq!((asg.params["type"].as_str(), asg.params["tier"].as_str(), asg.params["desired"].as_i64(), asg.params["spot"].as_bool()), (Some("t3.small"), Some("private"), Some(2), Some(true)));
        assert_eq!(d.edges.iter().filter(|e| e.kind == "target" && e.source == lb.id).count(), 2, "{:?}", d.edges);
        let other = d.nodes.iter().find(|n| n.kind == "other" && n.params["aws_id"] == "fsmt-1").expect("a grey block");
        assert_eq!((other.params["what"].as_str(), other.params["subnet"].as_str()), (Some("efs_mount_target"), Some("subnet_3")));
        for s in ["to = aws_lb.", "loadbalancer/app/web-lb/abc", "to = aws_lb_target_group.", "id = \"arn:tg\"", "to = aws_lb_listener.", "id = \"arn:lsn\"", "to = aws_autoscaling_group.", "id = \"workers\"", "to = aws_launch_template.", "id = \"lt-9\""] {
            assert!(imports.contains(s), "missing {s}\n{imports}");
        }
        for s in ["name               = \"web-lb\"", "name     = \"web-tg\"", "name                = \"workers\"", "name          = \"workers-lt\"", "image_id      = \"ami-0w\"", "# other: "] {
            assert!(files["main.tf"].contains(s), "missing {s}\n{}", files["main.tf"]);
        }
        assert!(im.notes.iter().any(|n| n.contains("grey")), "{:?}", im.notes);
        assert_eq!(web1.params["key"], "lab");
        assert_eq!(web1.params["spot"], true);
        assert_eq!(web1.params["disk"], 50);
        assert_eq!(web1.params["sgs"].as_array().unwrap().len(), 1);
        assert_eq!(d.nodes.iter().filter(|n| n.kind == "ec2").count(), 2, "the terminated one is gone");
        assert!(im.notes.iter().any(|n| n.contains("x9.odd")));
        assert!(im.notes.iter().any(|n| n.contains("is stopped")));
        assert!(d.text.as_ref().unwrap().contains("rule internet -> web tcp 443"), "{}", d.text.as_ref().unwrap());
        assert!(d.text.as_ref().unwrap().contains("rt app in prod_vpc: 0.0.0.0/0 -> nat_a, 10.9.0.0/16 -> peering_1, pl-s3 -> s3_endpoint"), "{}", d.text.as_ref().unwrap());
        assert!(d.text.as_ref().unwrap().contains("vpce ssm_ep_2 ssm in prod_vpc tier private sg prod_db"), "{}", d.text.as_ref().unwrap());
        for line in ["other efs_mount_target_1 efs_mount_target in prod_vpc", "alb web_lb internet in prod_vpc tier public sg web health /health", "asg workers t3.small in prod_vpc tier private min 1 max 3 desired 2 sg prod_db key lab spot", "target web_lb -> workers 8080"] {
            assert!(d.text.as_ref().unwrap().contains(line), "missing {line}\n{}", d.text.as_ref().unwrap());
        }
        // the text round-trips with the grey blocks and the balancer in it
        let again = crate::dsl::parse(d.text.as_ref().unwrap(), Some(d)).unwrap();
        assert_eq!(again.nodes.iter().filter(|n| n.kind == "other").count(), 2);
        let a = analyse(d);
        // the unknown type is a warning on its block (the owner may know it); nothing is red
        assert_eq!(a.errors, 0, "{:?}", a.problems);
        assert!(a.problems.iter().any(|p| p.contains("x9.odd") && p.contains("not in the app's table")));
        assert!(d.text.as_ref().unwrap().contains("subnet public_a public 10.0.0.0/20 az a in prod_vpc rt public"), "{}", d.text.as_ref().unwrap());
        assert_eq!(d.by_label("prod_vpc").unwrap().params["aws_id"], "vpc-0abc");
    }
}
