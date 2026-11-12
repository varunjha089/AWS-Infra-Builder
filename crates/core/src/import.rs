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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Inventory {
    pub region: String,
    pub account: String,
    pub vpc: VpcInfo,
    pub subnets: Vec<SubnetInfo>,
    pub igws: Vec<(String, Option<String>)>,
    pub nats: Vec<NatInfo>,
    pub route_tables: Vec<RouteTableInfo>,
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
    arrange(&mut d);
    crate::dsl::sync(&mut d);
    notes.insert(0, format!("Imported read-only from account {} in {}: {} subnets, {} Internet Gateway{}, {} NAT Gateway{}, {} route tables. Nothing in the account was changed.",
        inv.account, inv.region, inv.subnets.len(), inv.igws.len(), if inv.igws.len() == 1 { "" } else { "s" }, inv.nats.len(), if inv.nats.len() == 1 { "" } else { "s" }, inv.route_tables.len()));
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
        assert_eq!(d.edges.iter().filter(|e| e.kind == "route").count(), 2, "the peering route is left out with a note");
        assert!(im.notes.iter().any(|n| n.contains("pcx-1")));
        assert!(im.notes.iter().any(|n| n.contains("main table")));
        assert!(im.notes[0].contains("123456789012"));
        let a = analyse(d);
        assert!(a.ok, "{:?}", a.problems);
        assert!(d.text.as_ref().unwrap().contains("subnet public_a public 10.0.0.0/20 az a in prod_vpc rt public"), "{}", d.text.as_ref().unwrap());
        assert_eq!(d.by_label("prod_vpc").unwrap().params["aws_id"], "vpc-0abc");
    }
}
