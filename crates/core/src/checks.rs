//! The checks: every block and arrow is looked at while the design is drawn. "error" = AWS would refuse it or the design contradicts
//! itself; "warn" = it works but is probably not what you want. Each message says what to change. Numbers (usable addresses) are
//! shown with their arithmetic, as NN Builder shows its shape formula.

use crate::cidr::Cidr;
use crate::model::{az_letters, edge_kind, instance_type, parent_kind, parse_ports, region_azs, title, traffic_label, Design, Node, PORTS, TIERS};
use crate::pricing::{self, CostReport};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeResult {
    pub id: String,
    pub kind: String,
    pub title: String,
    /// "ok" | "warn" | "error"
    pub status: String,
    pub messages: Vec<String>,
    /// facts about the block (the CIDR arithmetic, the AZ name, what the block carries)
    pub info: Vec<String>,
    /// one line for the block itself
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EdgeResult {
    pub id: String,
    pub status: String,
    pub messages: Vec<String>,
    /// what the arrow carries, drawn on it
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Analysis {
    pub nodes: Vec<NodeResult>,
    pub edges: Vec<EdgeResult>,
    /// every error and warning once, in reading order, with the block's label
    pub problems: Vec<String>,
    /// no errors (warnings are allowed)
    pub ok: bool,
    pub errors: usize,
    pub warnings: usize,
    /// blocks per kind
    pub counts: HashMap<String, usize>,
    /// how many Availability Zones have at least one subnet
    pub az_spread: usize,
    /// the region's AZ names for the design's letters
    pub az_names: Vec<String>,
    /// what would tidy the design (merge or delete security groups and ranges), biggest first; see `tidy.rs`
    #[serde(default)]
    pub recommendations: Vec<crate::tidy::Recommendation>,
    /// what the app can repair at a click, one per fixable problem (plus the Graviton saving); see `fix.rs`
    #[serde(default)]
    pub fixes: Vec<crate::fix::Fix>,
    /// what the design costs a month while idle, and how to pay less
    pub cost: CostReport,
}

struct Ctx {
    status: HashMap<String, String>,
    msgs: HashMap<String, Vec<String>>,
}

impl Ctx {
    fn add(&mut self, id: &str, level: &str, msg: impl Into<String>) {
        self.msgs.entry(id.into()).or_default().push(msg.into());
        let cur = self.status.entry(id.into()).or_insert_with(|| "ok".into());
        if level == "error" || (level == "warn" && cur != "error") {
            *cur = level.into();
        }
    }
    fn error(&mut self, id: &str, msg: impl Into<String>) {
        self.add(id, "error", msg)
    }
    fn warn(&mut self, id: &str, msg: impl Into<String>) {
        self.add(id, "warn", msg)
    }
}

fn p<'a>(n: &'a Node, key: &str) -> &'a str {
    n.params.get(key).and_then(|v| v.as_str()).unwrap_or("")
}
fn ep<'a>(e: &'a crate::model::Edge, key: &str) -> &'a str {
    e.params.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// the route-table arrows that leave the table a subnet is attached to
fn routes_of_subnet<'a>(d: &'a Design, subnet: &str) -> Option<(&'a Node, Vec<(&'a crate::model::Edge, &'a Node)>)> {
    let attach = d.edges.iter().find(|e| e.kind == "attach" && e.source == subnet)?;
    let rt = d.node(&attach.target)?;
    let routes = d.edges.iter().filter(|e| e.kind == "route" && e.source == rt.id).filter_map(|e| Some((e, d.node(&e.target)?))).collect();
    Some((rt, routes))
}

/// "HH:MM", 24-hour
pub fn valid_time(t: &str) -> bool {
    let Some((h, m)) = t.split_once(':') else { return false };
    h.len() == 2 && m.len() == 2 && h.parse::<u8>().map(|h| h < 24).unwrap_or(false) && m.parse::<u8>().map(|m| m < 60).unwrap_or(false)
}

pub fn analyse(d: &Design) -> Analysis {
    let mut cx = Ctx { status: HashMap::new(), msgs: HashMap::new() };
    let mut info: HashMap<String, Vec<String>> = HashMap::new();
    let mut summary: HashMap<String, String> = HashMap::new();
    let letters = az_letters(d.azs);
    let az_names: Vec<String> = letters.iter().map(|l| format!("{}{}", d.region, l)).collect();

    // ---- the region and its AZ count
    match region_azs(&d.region) {
        None => {
            for n in d.nodes.iter().filter(|n| n.kind == "vpc") {
                cx.warn(&n.id, format!("The region \"{}\" is not in the app's list; the AZ names are guessed as {}a, {}b …", d.region, d.region, d.region));
            }
        }
        Some(have) if d.azs > have => {
            for n in d.nodes.iter().filter(|n| n.kind == "vpc") {
                cx.error(&n.id, format!("{} has {} Availability Zones, but the design uses {}. Lower the AZ count or choose another region.", d.region, have, d.azs));
            }
        }
        _ => {}
    }

    // ---- nesting: each block lives in the right kind of block
    for n in &d.nodes {
        match parent_kind(&n.kind) {
            None => cx.error(&n.id, format!("Unknown block kind \"{}\".", n.kind)),
            Some(want) => {
                let have = n.parent.as_ref().and_then(|p| d.node(p)).map(|p| p.kind.as_str()).unwrap_or("");
                if n.parent.is_some() && have.is_empty() {
                    cx.error(&n.id, "This block's container no longer exists. Drag it into a VPC or subnet.");
                } else if want != have {
                    let want_t = if want.is_empty() { "the region (outside every VPC)".to_string() } else { format!("a {}", title(want)) };
                    let have_t = if have.is_empty() { "the region".to_string() } else { format!("a {}", title(have)) };
                    cx.error(&n.id, format!("A {} lives in {want_t}, not in {have_t}. Drag it there.", title(&n.kind)));
                }
            }
        }
    }

    // ---- VPCs: CIDR
    let mut vpc_cidr: HashMap<String, Cidr> = HashMap::new();
    for v in d.nodes.iter().filter(|n| n.kind == "vpc") {
        match Cidr::parse(p(v, "cidr")) {
            Err(e) => cx.error(&v.id, format!("CIDR: {e}.")),
            Ok(c) => {
                if c.prefix < 16 || c.prefix > 28 {
                    cx.error(&v.id, format!("A VPC CIDR must be between /16 and /28 (AWS rule); {c} is /{}.", c.prefix));
                } else if !c.is_private() {
                    cx.warn(&v.id, format!("{c} is public address space. AWS allows it, but instances cannot reach the real owners of those addresses; use 10.0.0.0/8, 172.16.0.0/12 or 192.168.0.0/16 unless you mean it."));
                }
                if c.to_string() != p(v, "cidr").trim() {
                    cx.error(&v.id, format!("{} is not a network address (AWS refuses it); write {c}.", p(v, "cidr").trim()));
                }
                info.entry(v.id.clone()).or_default().push(format!("{c}: 2^(32−{}) = {} addresses", c.prefix, c.size()));
                summary.insert(v.id.clone(), format!("{c} · {} addresses", c.size()));
                vpc_cidr.insert(v.id.clone(), c);
            }
        }
        let igws: Vec<&Node> = d.children(&v.id).into_iter().filter(|n| n.kind == "igw").collect();
        if igws.len() > 1 {
            for g in &igws {
                cx.error(&g.id, "A VPC can have only one Internet Gateway (AWS rule). Delete the extra one.");
            }
        }
        let subs: Vec<&Node> = d.children(&v.id).into_iter().filter(|n| n.kind == "subnet").collect();
        if subs.is_empty() {
            cx.warn(&v.id, "This VPC has no subnets yet. Add a subnet from the palette.");
        }
        // peering comes later; two VPCs with the same range cannot be joined, say so now
        for other in d.nodes.iter().filter(|n| n.kind == "vpc" && n.id != v.id) {
            if let (Some(a), Some(b)) = (vpc_cidr.get(&v.id), Cidr::parse(p(other, "cidr")).ok().as_ref()) {
                if a.overlaps(b) && v.id < other.id {
                    cx.warn(&v.id, format!("{} and {} overlap ({a} / {b}); they could never be peered.", v.label, other.label));
                }
            }
        }
    }

    // ---- subnets: CIDR inside the VPC, no overlap, AZ, usable addresses
    let mut subnet_cidr: HashMap<String, Cidr> = HashMap::new();
    for s in d.nodes.iter().filter(|n| n.kind == "subnet") {
        let tier = p(s, "tier");
        let az = p(s, "az");
        if !TIERS.contains(&tier) {
            cx.error(&s.id, format!("Tier \"{tier}\" is not one of public, private, isolated."));
        }
        if !letters.contains(&az.to_string()) {
            cx.error(&s.id, format!("AZ \"{az}\" is not one of the design's zones ({}). Raise the AZ count or choose another zone.", letters.join(", ")));
        }
        let az_name = letters.iter().position(|l| l == az).and_then(|i| az_names.get(i)).cloned().unwrap_or_else(|| format!("{}{az}", d.region));
        let vpc = s.parent.as_ref().and_then(|pid| d.node(pid)).filter(|n| n.kind == "vpc");
        match Cidr::parse(p(s, "cidr")) {
            Err(e) => cx.error(&s.id, format!("CIDR: {e}. The inspector proposes a free block.")),
            Ok(c) => {
                if c.prefix < 16 || c.prefix > 28 {
                    cx.error(&s.id, format!("A subnet CIDR must be between /16 and /28 (AWS rule); {c} is /{}.", c.prefix));
                }
                if c.to_string() != p(s, "cidr").trim() {
                    cx.error(&s.id, format!("{} is not a network address (AWS refuses it); write {c}.", p(s, "cidr").trim()));
                }
                if let Some(v) = vpc {
                    if let Some(vc) = vpc_cidr.get(&v.id) {
                        if !vc.contains(&c) {
                            cx.error(&s.id, format!("{c} lies outside the VPC's {vc}. Choose a block inside it (the inspector proposes one)."));
                        }
                    }
                    // overlap with sister subnets (reported on both)
                    for o in d.children(&v.id).into_iter().filter(|n| n.kind == "subnet" && n.id != s.id) {
                        if let Ok(oc) = Cidr::parse(p(o, "cidr")) {
                            if oc.overlaps(&c) {
                                cx.error(&s.id, format!("{c} overlaps {} ({oc}). Subnets of one VPC must not overlap.", o.label));
                            }
                        }
                    }
                }
                info.entry(s.id.clone()).or_default().push(format!("{c}: 2^(32−{}) = {} addresses − 5 reserved by AWS = {} usable", c.prefix, c.size(), c.usable()));
                summary.insert(s.id.clone(), format!("{tier} · {c} · {} usable · {az_name}", c.usable()));
                subnet_cidr.insert(s.id.clone(), c);
            }
        }
        info.entry(s.id.clone()).or_default().push(format!("Availability Zone {az_name}"));
        if !summary.contains_key(&s.id) {
            summary.insert(s.id.clone(), format!("{tier} · no CIDR · {az_name}"));
        }

        // routing: what the subnet's route table says against what the tier promises
        match routes_of_subnet(d, &s.id) {
            None => {
                info.entry(s.id.clone()).or_default().push("Route table: the VPC's main table (local routes only)".into());
                match tier {
                    "public" => cx.error(&s.id, "A public subnet needs a route table with 0.0.0.0/0 → Internet Gateway. Draw an arrow from the subnet to a route table."),
                    "private" => cx.warn(&s.id, "No route table: instances here cannot reach the internet (no NAT). Attach a route table with 0.0.0.0/0 → NAT Gateway, or mark the subnet isolated."),
                    _ => {}
                }
            }
            Some((rt, routes)) => {
                info.entry(s.id.clone()).or_default().push(format!("Route table {}", rt.label));
                let default_to = routes.iter().filter(|(e, _)| ep(e, "dest").trim() == "0.0.0.0/0").map(|(_, t)| t).collect::<Vec<_>>();
                let to_igw = default_to.iter().any(|t| t.kind == "igw");
                let to_nat = default_to.iter().any(|t| t.kind == "nat");
                match tier {
                    "public" if !to_igw => cx.error(&s.id, format!("Marked public, but route table {} has no 0.0.0.0/0 → Internet Gateway. Add that route, or mark the subnet private.", rt.label)),
                    "private" if to_igw => cx.error(&s.id, format!("Marked private, but route table {} sends 0.0.0.0/0 to an Internet Gateway: that makes it public. Route to a NAT Gateway instead, or mark the subnet public.", rt.label)),
                    "private" if !to_nat => cx.warn(&s.id, format!("Marked private, but route table {} has no 0.0.0.0/0 → NAT Gateway: instances cannot reach the internet{}. Add the route, or mark the subnet isolated.", rt.label, if routes.iter().any(|(_, t)| t.kind == "vpce") { " (only the endpoint's service)" } else { "" })),
                    "isolated" if to_igw || to_nat => cx.error(&s.id, format!("Marked isolated, but route table {} has a default route to the internet. Remove it, or mark the subnet private/public.", rt.label)),
                    _ => {}
                }
            }
        }
    }

    // ---- NAT Gateways: in a public subnet, one per AZ
    let mut nat_azs: HashMap<String, Vec<String>> = HashMap::new(); // vpc -> az letters with a NAT
    for n in d.nodes.iter().filter(|n| n.kind == "nat") {
        let sub = n.parent.as_ref().and_then(|pid| d.node(pid)).filter(|s| s.kind == "subnet");
        if let Some(s) = sub {
            if p(s, "tier") != "public" {
                cx.error(&n.id, format!("A NAT Gateway must sit in a public subnet; {} is {}. Move it, or make that subnet public.", s.label, p(s, "tier")));
            }
            if let Some(vpc) = s.parent.clone() {
                nat_azs.entry(vpc).or_default().push(p(s, "az").to_string());
            }
            info.entry(n.id.clone()).or_default().push(format!("In {} ({}{})", s.label, d.region, p(s, "az")));
        }
        let monthly = pricing::nat_hourly(&d.region) * pricing::HOURS;
        info.entry(n.id.clone()).or_default().push(format!("Needs an Elastic IP (created with it). Costs about USD {monthly:.0} a month in {} before any traffic, plus USD {} per GB.", d.region, pricing::nat_per_gb(&d.region)));
        summary.insert(n.id.clone(), format!("≈ USD {monthly:.0} / month + data"));
        if !d.edges.iter().any(|e| e.kind == "route" && e.target == n.id) {
            cx.warn(&n.id, "No route table sends traffic here: the NAT Gateway would cost money and carry nothing. Draw an arrow from a private route table to it.");
        }
    }
    // private subnets whose NAT is in another AZ
    for s in d.nodes.iter().filter(|n| n.kind == "subnet" && p(n, "tier") == "private") {
        if let Some((_, routes)) = routes_of_subnet(d, &s.id) {
            for (e, t) in routes.iter().filter(|(e, t)| t.kind == "nat" && ep(e, "dest").trim() == "0.0.0.0/0") {
                let nat_az = t.parent.as_ref().and_then(|pid| d.node(pid)).map(|ns| p(ns, "az").to_string()).unwrap_or_default();
                if nat_az != p(s, "az") {
                    cx.warn(&s.id, format!("Its NAT Gateway {} is in AZ {}: an outage of that zone stops this subnet's internet access, and cross-AZ traffic is charged. Add a NAT in AZ {} (or accept the saving).", t.label, nat_az, p(s, "az")));
                }
                let _ = e;
            }
        }
    }

    // ---- Internet Gateways: used or not
    for g in d.nodes.iter().filter(|n| n.kind == "igw") {
        if !d.edges.iter().any(|e| e.kind == "route" && e.target == g.id) {
            cx.warn(&g.id, "No route table sends traffic here. Draw an arrow from a public route table to it (destination 0.0.0.0/0).");
        }
        summary.insert(g.id.clone(), "to and from the internet".into());
    }

    // ---- route tables and their routes
    for rt in d.nodes.iter().filter(|n| n.kind == "rt") {
        let routes: Vec<&crate::model::Edge> = d.edges.iter().filter(|e| e.kind == "route" && e.source == rt.id).collect();
        let attached: Vec<&Node> = d.edges.iter().filter(|e| e.kind == "attach" && e.target == rt.id).filter_map(|e| d.node(&e.source)).collect();
        if attached.is_empty() {
            cx.warn(&rt.id, "No subnet uses this route table. Draw an arrow from a subnet to it.");
        }
        let mut dests: Vec<String> = vec![];
        for e in &routes {
            let dest = ep(e, "dest").trim().to_string();
            let to_endpoint = d.node(&e.target).map(|t| t.kind == "vpce").unwrap_or(false);
            if to_endpoint {
                if !dest.starts_with("pl-") {
                    cx.error(&e.id, format!("A route to a VPC endpoint goes to the service's prefix list, written pl-<service> (here pl-{}), not to {dest}.", d.node(&e.target).map(|t| p(t, "service")).unwrap_or("s3")));
                }
                if d.node(&e.target).map(|t| crate::model::endpoint_type(p(t, "service")) != Some("gateway")).unwrap_or(false) {
                    cx.error(&e.id, "Only gateway endpoints (S3, DynamoDB) are routed to. An interface endpoint is reached by DNS from the subnets of its tier; delete this arrow.");
                }
                continue;
            }
            match Cidr::parse(&dest) {
                Err(err) => cx.error(&e.id, format!("Destination: {err}.")),
                Ok(c) => {
                    if dests.contains(&c.to_string()) {
                        cx.error(&e.id, format!("Two routes for {c} in {}: a route table has one target per destination.", rt.label));
                    }
                    dests.push(c.to_string());
                    if let Some(vc) = rt.parent.as_ref().and_then(|v| vpc_cidr.get(v)) {
                        if vc.contains(&c) {
                            cx.error(&e.id, format!("{c} is inside the VPC: the VPC's own local route already covers it and cannot be overridden."));
                        }
                    }
                }
            }
        }
        let mut lines: Vec<String> = vec![format!("{} local (the VPC itself)", rt.parent.as_ref().and_then(|v| vpc_cidr.get(v)).map(|c| c.to_string()).unwrap_or_else(|| "VPC".into()))];
        for e in &routes {
            let t = d.node(&e.target).map(|t| t.label.clone()).unwrap_or_default();
            lines.push(format!("{} → {t}", ep(e, "dest")));
        }
        info.entry(rt.id.clone()).or_default().extend(lines);
        summary.insert(rt.id.clone(), format!("{} route{} · {} subnet{}", routes.len() + 1, if routes.is_empty() { "" } else { "s" }, attached.len(), if attached.len() == 1 { "" } else { "s" }));
    }

    // ---- address ranges (the internet, an office)
    for c in d.nodes.iter().filter(|n| n.kind == "cidr") {
        match Cidr::parse(p(c, "cidr")) {
            Err(e) => cx.error(&c.id, format!("Range: {e}.")),
            Ok(r) => {
                summary.insert(c.id.clone(), if r.prefix == 0 { "anywhere (the whole internet)".into() } else { format!("{r} · {} addresses", r.size()) });
                if !d.edges.iter().any(|e| e.source == c.id || e.target == c.id) {
                    cx.warn(&c.id, "No rule uses this range. Draw an arrow from it to a security group (traffic in) or from a group to it (traffic out).");
                }
            }
        }
    }

    // ---- security groups and traffic arrows
    let risky = |a: u16, b: u16| -> Vec<&'static str> { PORTS.iter().filter(|(n, port)| matches!(*n, "ssh" | "rdp" | "mysql" | "postgres" | "mssql" | "mongodb" | "redis") && a <= *port && *port <= b).map(|(n, _)| *n).collect() };
    for g in d.nodes.iter().filter(|n| n.kind == "sg") {
        let inbound: Vec<&crate::model::Edge> = d.edges.iter().filter(|e| e.kind == "traffic" && e.target == g.id).collect();
        let outbound: Vec<&crate::model::Edge> = d.edges.iter().filter(|e| e.kind == "traffic" && e.source == g.id).collect();
        // instances and scaling groups tick groups; a balancer and an interface endpoint name one
        let members: Vec<&Node> = d.nodes.iter().filter(|n| ((n.kind == "ec2" || n.kind == "asg") && n.params.get("sgs").and_then(|v| v.as_array()).map(|a| a.iter().any(|v| v.as_str() == Some(&g.id))).unwrap_or(false))
            || ((n.kind == "alb" || n.kind == "vpce") && p(n, "sg") == g.id)).collect();
        let egress_all = p(g, "egress") != "rules";
        let mut lines = vec![];
        for e in &inbound {
            lines.push(format!("in: {} from {}", traffic_label(ep(e, "proto"), ep(e, "ports")), d.node(&e.source).map(|n| n.label.clone()).unwrap_or_default()));
        }
        if egress_all {
            lines.push("out: all traffic to anywhere (AWS's default)".into());
        } else {
            for e in &outbound {
                lines.push(format!("out: {} to {}", traffic_label(ep(e, "proto"), ep(e, "ports")), d.node(&e.target).map(|n| n.label.clone()).unwrap_or_default()));
            }
            if outbound.is_empty() {
                lines.push("out: nothing (egress rules, none drawn)".into());
            }
        }
        // an arrow to another group is that group's ingress rule; an arrow to an address range is only an egress rule, and with
        // egress "all" it adds nothing
        if egress_all && outbound.iter().any(|e| d.node(&e.target).map(|t| t.kind == "cidr").unwrap_or(false)) {
            cx.warn(&g.id, "Egress is \"all\" (AWS's default), so the arrows from this group to an address range add nothing. Set egress to \"rules\" to allow out only what the arrows say.");
        }
        if members.is_empty() {
            cx.warn(&g.id, "Nothing uses this security group. Tick it on an instance or a scaling group, choose it on a balancer, or delete it.");
        }
        if inbound.is_empty() && (egress_all || outbound.is_empty()) {
            cx.warn(&g.id, "No rule lets anything in: instances in this group cannot be reached. Draw an arrow from a group or an address range to it.");
        }
        info.entry(g.id.clone()).or_default().extend(lines);
        info.entry(g.id.clone()).or_default().push(format!("Used by {}", if members.is_empty() { "no instance".to_string() } else { members.iter().map(|m| m.label.clone()).collect::<Vec<_>>().join(", ") }));
        summary.insert(g.id.clone(), format!("{} in · {} · {} instance{}", inbound.len(), if egress_all { "all out".to_string() } else { format!("{} out", outbound.len()) }, members.len(), if members.len() == 1 { "" } else { "s" }));
    }
    for e in d.edges.iter().filter(|e| e.kind == "traffic") {
        let (Some(s), Some(t)) = (d.node(&e.source), d.node(&e.target)) else { continue };
        match parse_ports(ep(e, "proto"), ep(e, "ports")) {
            Err(m) => cx.error(&e.id, format!("Traffic: {m}.")),
            Ok((a, b)) => {
                // the world may reach a listening port: fine for web, a mistake for SSH, RDP and databases
                let world = s.kind == "cidr" && Cidr::parse(p(s, "cidr")).map(|c| c.prefix == 0).unwrap_or(false);
                if world && t.kind == "sg" {
                    let r = risky(a, b);
                    if !r.is_empty() {
                        cx.warn(&e.id, format!("{} open to the whole internet ({}). Allow it only from your own address range (add an Address range block with your office or home IP), or use SSM Session Manager and no inbound port at all.", r.join(", "), p(s, "cidr")));
                    }
                }
                if s.kind == "sg" && t.kind == "sg" && s.parent != t.parent {
                    cx.error(&e.id, format!("{} and {} are in different VPCs: a rule can only reference a group of its own VPC (use the other VPC's CIDR as an Address range instead).", s.label, t.label));
                }
            }
        }
    }

    // ---- instances
    for n in d.nodes.iter().filter(|n| n.kind == "ec2") {
        let t = p(n, "type");
        let sub = n.parent.as_ref().and_then(|pid| d.node(pid)).filter(|s| s.kind == "subnet");
        let public_ip = n.params.get("public_ip").and_then(|v| v.as_bool()).unwrap_or(false);
        let sgs: Vec<&Node> = n.params.get("sgs").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).filter_map(|id| d.node(id)).collect()).unwrap_or_default();
        match instance_type(t) {
            None => cx.warn(&n.id, format!("\"{t}\" is not in the app's table: its price is unknown and the app cannot check that it exists in {}. Keep it if you know it; \"Recommend a type…\" in the inspector lists the known ones.", d.region)),
            Some(it) => {
                let hourly = crate::pricing::instance_hourly(t, &d.region);
                info.entry(n.id.clone()).or_default().push(format!("{t}: {} vCPU, {} GiB, {} · USD {hourly:.4}/h × {:.0} h = USD {:.2} a month while running", it.1, it.2, it.3, crate::pricing::hours_of(n), hourly * crate::pricing::hours_of(n)));
                if it.3 == "x86_64" {
                    if let Some(arm) = crate::pricing::arm_equivalent(t) {
                        let save = (hourly - crate::pricing::instance_hourly(arm, &d.region)) * crate::pricing::hours_of(n);
                        if save > 0.0 {
                            info.entry(n.id.clone()).or_default().push(format!("{arm} (Graviton, same size) would save USD {save:.2} a month; most Linux software runs on arm64 unchanged."));
                        }
                    }
                }
            }
        }
        // the values are the owner's to type; what AWS would refuse is said here
        let disk = n.params.get("disk").and_then(|v| v.as_f64()).unwrap_or(8.0);
        if !(8.0..=16384.0).contains(&disk) || disk.fract() != 0.0 {
            cx.error(&n.id, format!("The root volume is {disk} GiB; AWS allows whole numbers from 8 to 16384 GiB (gp3)."));
        }
        if let Some(h) = n.params.get("hours").and_then(|v| v.as_f64()) {
            if !(1.0..=744.0).contains(&h) {
                cx.warn(&n.id, format!("{h} hours a month is outside 1 to 744 (a month has at most 744): the estimate uses 730 until it is changed."));
            }
        }
        // the image's own rules: architecture, the smallest root volume, what it is for
        let os = p(n, "os");
        if let Some(o) = crate::model::os_info(os) {
            let arch = instance_type(t).map(|it| it.3).unwrap_or("x86_64");
            if o.4 != "any" && arch != o.4 {
                cx.error(&n.id, format!("{} is {} only; {t} is {arch} (Graviton). Pick an x86 type (t3, m5, c5, g4dn, g5) or another OS.", o.1, o.4));
            }
            let disk = n.params.get("disk").and_then(|v| v.as_u64()).unwrap_or(8);
            if disk < o.5 {
                cx.error(&n.id, format!("{} needs a root volume of at least {} GiB (its image is that big; AWS refuses a smaller one): set the root volume to {}.", o.1, o.5, o.5));
            }
            if os.starts_with("dlami") && !crate::model::is_gpu(t) {
                cx.warn(&n.id, format!("The Deep Learning AMI is meant for GPU instances; on {t} its CUDA and drivers only cost disk space. Pick a g4dn or g5 type, or plain Ubuntu."));
            }
            if os == "openvpn" {
                let lets_in = |port: u16| sgs.iter().any(|g| d.edges.iter().filter(|e| e.kind == "traffic" && e.target == g.id).any(|e| parse_ports(ep(e, "proto"), ep(e, "ports")).map(|(a, b)| a <= port && port <= b).unwrap_or(false)));
                if !lets_in(1194) {
                    cx.warn(&n.id, "OpenVPN clients connect on udp 1194 and the admin page is tcp 943: draw arrows into the instance's group for them (and tcp 443 for the web login).".to_string());
                }
            }
            if !o.6.is_empty() {
                info.entry(n.id.clone()).or_default().push(format!("{}: {}.", o.1, o.6));
            }
        }
        if let Some(s) = sub {
            let tier = p(s, "tier");
            if public_ip && tier != "public" {
                cx.warn(&n.id, format!("A public IP on an instance in a {tier} subnet is unreachable (no route from the Internet Gateway) and still costs USD 3.65 a month. Turn it off, or move the instance to a public subnet."));
            }
            if public_ip {
                info.entry(n.id.clone()).or_default().push("Public IPv4 address: USD 0.005/h = USD 3.65 a month (charged since 2024).".into());
            }
            let wants_in = sgs.iter().any(|g| d.edges.iter().any(|e| e.kind == "traffic" && e.target == g.id && d.node(&e.source).map(|src| src.kind == "cidr").unwrap_or(false)));
            if tier == "public" && !public_ip && wants_in {
                cx.warn(&n.id, "A rule lets traffic in from outside, but the instance has no public IP: nothing outside the VPC can reach it. Turn the public IP on (USD 3.65 a month), or reach it through SSM Session Manager.");
            }
        }
        if sgs.is_empty() {
            cx.warn(&n.id, "No security group: AWS attaches the VPC's default group (all traffic between its members, everything out). Tick a group in the inspector.");
        }
        let spot = n.params.get("spot").and_then(|v| v.as_bool()).unwrap_or(false);
        if spot {
            info.entry(n.id.clone()).or_default().push("Spot: about 70 % cheaper; AWS may stop it with two minutes' notice when it needs the capacity, so the work on it must checkpoint and resume (a web server should not be spot).".into());
        }
        if let Some(g) = n.params.get("disk").and_then(|v| v.as_u64()) {
            if g > 8 {
                info.entry(n.id.clone()).or_default().push(format!("Root volume {g} GiB gp3: USD {:.2} a month, charged even while the instance is stopped.", g as f64 * crate::pricing::GP3_PER_GIB));
            }
        }
        let ssh_in = sgs.iter().any(|g| d.edges.iter().any(|e| e.kind == "traffic" && e.target == g.id && parse_ports(ep(e, "proto"), ep(e, "ports")).map(|(a, b)| a <= 22 && 22 <= b).unwrap_or(false)));
        if ssh_in && p(n, "key").trim().is_empty() {
            cx.warn(&n.id, "SSH is allowed in, but no key pair is named: you could not log in. Name an existing EC2 key pair, or use SSM Session Manager instead of SSH.");
        }
        summary.insert(n.id.clone(), format!("{t}{} · {} · USD {:.0}/month{}", if spot { " spot" } else { "" }, p(n, "os"), crate::pricing::instance_hourly(t, &d.region) * if spot { crate::pricing::SPOT_FACTOR } else { 1.0 } * crate::pricing::hours_of(n) + if public_ip { 3.65 } else { 0.0 }, if public_ip { " · public IP" } else { "" }));
        info.entry(n.id.clone()).or_default().push(format!("Security groups: {}", if sgs.is_empty() { "none (the VPC's default)".to_string() } else { sgs.iter().map(|g| g.label.clone()).collect::<Vec<_>>().join(", ") }));
    }

    // ---- load balancers and Auto Scaling groups
    let tier_subnets = |vpc: &Option<String>, tier: &str| -> Vec<&Node> { d.nodes.iter().filter(|s| s.kind == "subnet" && s.parent == *vpc && p(s, "tier") == tier).collect() };
    let azs_of = |subs: &[&Node]| { let mut a: Vec<&str> = subs.iter().map(|s| p(s, "az")).collect(); a.sort_unstable(); a.dedup(); a.len() };
    let allows_in = |sg_id: &str, from: Option<&str>, port: u16| d.edges.iter().any(|e| e.kind == "traffic" && e.target == sg_id && from.map(|f| e.source == f).unwrap_or(true) && parse_ports(ep(e, "proto"), ep(e, "ports")).map(|(a, b)| a <= port && port <= b).unwrap_or(false));
    for lb in d.nodes.iter().filter(|n| n.kind == "alb") {
        let tier = p(lb, "tier");
        let subs = tier_subnets(&lb.parent, tier);
        let azs = azs_of(&subs);
        let port = lb.params.get("port").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
        let cert = p(lb, "cert").trim();
        if azs < 2 {
            cx.error(&lb.id, format!("A load balancer needs subnets in at least two Availability Zones (AWS rule); the {tier} tier has {azs}. Add a {tier} subnet in another AZ, or raise the AZ count."));
        }
        if p(lb, "scheme") == "internet" && tier != "public" {
            cx.error(&lb.id, "An internet-facing load balancer must sit in public subnets (with a route to the Internet Gateway). Set its tier to public, or make it internal.");
        }
        if port == 443 && cert.is_empty() {
            cx.error(&lb.id, "HTTPS on 443 needs a certificate: name an ACM certificate ARN, or use port 80 (HTTP) for now.");
        }
        if !cert.is_empty() && !cert.starts_with("arn:aws:acm:") {
            cx.error(&lb.id, "The certificate must be an ACM certificate ARN (arn:aws:acm:…), in the same region.");
        }
        let sg = d.node(p(lb, "sg")).filter(|g| g.kind == "sg");
        match sg {
            None => cx.warn(&lb.id, "No security group: the balancer would use the VPC's default group, which lets nothing in from the internet. Make a group with 443 (or 80) in from anywhere and choose it."),
            Some(g) => {
                if !allows_in(&g.id, None, port) {
                    cx.warn(&lb.id, format!("Its group {} does not let port {port} in: nobody can reach the balancer. Draw an arrow from the internet (or the callers) to {} with tcp {port}.", g.label, g.label));
                }
            }
        }
        let targets: Vec<(&crate::model::Edge, &Node)> = d.edges.iter().filter(|e| e.kind == "target" && e.source == lb.id).filter_map(|e| Some((e, d.node(&e.target)?))).collect();
        if targets.is_empty() {
            cx.warn(&lb.id, "No target: draw an arrow from the balancer to an Auto Scaling group or an instance.");
        }
        for (e, t) in &targets {
            let tport = e.params.get("port").and_then(|v| v.as_u64()).unwrap_or(8080) as u16;
            let tsgs: Vec<&str> = t.params.get("sgs").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
            if let Some(g) = sg {
                if !tsgs.iter().any(|ts| allows_in(ts, Some(&g.id), tport)) {
                    cx.warn(&e.id, format!("{} does not let port {tport} in from the balancer's group {}: health checks would fail. Draw an arrow {} → {}'s group with tcp {tport}.", t.label, g.label, g.label, t.label));
                }
            }
            if t.kind == "ec2" && t.parent.as_ref().and_then(|s| d.node(s)).map(|s| p(s, "tier") == "public").unwrap_or(false) && t.params.get("public_ip").and_then(|v| v.as_bool()).unwrap_or(false) {
                cx.warn(&t.id, "Behind a load balancer the instance needs no public IP (USD 3.65 a month): turn it off and move it to a private subnet.");
            }
        }
        let monthly = crate::pricing::alb_hourly(&d.region) * crate::pricing::HOURS;
        info.entry(lb.id.clone()).or_default().push(format!("{} · {} subnets in {azs} AZ{} · USD {monthly:.2} a month idle plus capacity units (about USD 6 per LCU-month)", if p(lb, "scheme") == "internet" { "internet-facing" } else { "internal" }, subs.len(), if azs == 1 { "" } else { "s" }));
        summary.insert(lb.id.clone(), format!("{} · {} :{port} · USD {monthly:.0}/month + LCU", p(lb, "scheme"), if cert.is_empty() { "HTTP" } else { "HTTPS" }));
    }
    for g in d.nodes.iter().filter(|n| n.kind == "asg") {
        let tier = p(g, "tier");
        let subs = tier_subnets(&g.parent, tier);
        let num = |k: &str, dflt: u64| g.params.get(k).and_then(|v| v.as_u64()).unwrap_or(dflt);
        let (min, max, desired) = (num("min", 1), num("max", 2), num("desired", 1));
        if max < 1 {
            cx.error(&g.id, format!("max is at least 1; here max {max}."));
        }
        let disk = g.params.get("disk").and_then(|v| v.as_f64()).unwrap_or(8.0);
        if !(8.0..=16384.0).contains(&disk) || disk.fract() != 0.0 {
            cx.error(&g.id, format!("The root volume is {disk} GiB; AWS allows whole numbers from 8 to 16384 GiB."));
        }
        if let Some(h) = g.params.get("hours").and_then(|v| v.as_f64()) {
            if !(1.0..=744.0).contains(&h) {
                cx.warn(&g.id, format!("{h} hours a month is outside 1 to 744: the estimate uses 730 until it is changed."));
            }
        }
        let t = p(g, "type");
        if subs.is_empty() {
            cx.error(&g.id, format!("No {tier} subnet in this VPC to launch into. Choose another tier, or add a subnet."));
        }
        if !(min <= desired && desired <= max) {
            cx.error(&g.id, format!("min ≤ desired ≤ max must hold; here min {min}, desired {desired}, max {max}."));
        }
        if instance_type(t).is_none() {
            cx.warn(&g.id, format!("\"{t}\" is not in the app's table: its price is unknown and the app cannot check that it exists."));
        }
        let usable: u64 = subs.iter().filter_map(|s| Cidr::parse(p(s, "cidr")).ok()).map(|c| c.usable()).sum();
        if usable > 0 && max > usable {
            cx.warn(&g.id, format!("max {max} instances will not fit: the {tier} subnets have {usable} usable addresses together."));
        }
        if azs_of(&subs) < 2 && max > 1 {
            cx.warn(&g.id, "All instances are in one AZ: an outage of that zone stops the service. Add a subnet of this tier in another AZ.");
        }
        let sgs: Vec<&Node> = g.params.get("sgs").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).filter_map(|id| d.node(id)).collect()).unwrap_or_default();
        if sgs.is_empty() {
            cx.warn(&g.id, "No security group: the instances get the VPC's default group. Tick a group.");
        }
        let spot = g.params.get("spot").and_then(|v| v.as_bool()).unwrap_or(false);
        let h = crate::pricing::instance_hourly(t, &d.region) * if spot { crate::pricing::SPOT_FACTOR } else { 1.0 };
        info.entry(g.id.clone()).or_default().push(format!("{t}{} × {desired} desired ({min}–{max}) in {} {tier} subnet{} ({} AZ{}): USD {:.2} a month at the desired size", if spot { " spot" } else { "" }, subs.len(), if subs.len() == 1 { "" } else { "s" }, azs_of(&subs), if azs_of(&subs) == 1 { "" } else { "s" }, h * crate::pricing::HOURS * desired as f64));
        if spot {
            info.entry(g.id.clone()).or_default().push("Spot: the group replaces interrupted instances by itself, which makes Spot safe for stateless servers behind a balancer.".into());
        } else if desired >= 2 && d.edges.iter().any(|e| e.kind == "target" && e.target == g.id) {
            info.entry(g.id.clone()).or_default().push("Behind a balancer the instances are replaceable: Spot would cut this about 70 %, and the group replaces interrupted ones.".into());
        }
        summary.insert(g.id.clone(), format!("{desired}× {t}{} · {min}–{max} · USD {:.0}/month", if spot { " spot" } else { "" }, h * crate::pricing::HOURS * desired as f64));
    }

    // ---- VPC endpoints
    for ep_node in d.nodes.iter().filter(|n| n.kind == "vpce") {
        let svc = p(ep_node, "service");
        match crate::model::endpoint_type(svc) {
            None => cx.error(&ep_node.id, format!("\"{svc}\" is not a service the app knows. Choose one in the inspector.")),
            Some("gateway") => {
                let tables: Vec<&Node> = d.edges.iter().filter(|e| e.kind == "route" && e.target == ep_node.id).filter_map(|e| d.node(&e.source)).collect();
                if tables.is_empty() {
                    cx.warn(&ep_node.id, format!("No route table routes to this endpoint: nothing uses it. Draw an arrow from a route table to it (destination pl-{svc})."));
                }
                info.entry(ep_node.id.clone()).or_default().push(format!("Gateway endpoint for {}: free, no data charge. Subnets of {} reach {} privately, without a NAT Gateway.", svc.to_uppercase(), if tables.is_empty() { "no table".to_string() } else { tables.iter().map(|t| t.label.clone()).collect::<Vec<_>>().join(", ") }, svc.to_uppercase()));
                summary.insert(ep_node.id.clone(), format!("{} gateway · free · {} table{}", svc.to_uppercase(), tables.len(), if tables.len() == 1 { "" } else { "s" }));
            }
            Some(_) => {
                let tier = p(ep_node, "tier");
                let subs: Vec<&Node> = ep_node.parent.as_ref().map(|v| d.children(v)).unwrap_or_default().into_iter().filter(|n| n.kind == "subnet" && p(n, "tier") == tier).collect();
                if subs.is_empty() {
                    cx.error(&ep_node.id, format!("No {tier} subnet in this VPC to put the endpoint's interface in. Choose another tier, or add a subnet."));
                }
                if p(ep_node, "sg").is_empty() || d.node(p(ep_node, "sg")).is_none() {
                    cx.warn(&ep_node.id, "No security group: AWS uses the VPC's default group, which may not allow HTTPS (443) from the subnets. Make a group that lets 443 in from the VPC's range and choose it here.");
                }
                let monthly = subs.len() as f64 * crate::pricing::INTERFACE_ENDPOINT_HOURLY * crate::pricing::HOURS;
                info.entry(ep_node.id.clone()).or_default().push(format!("Interface endpoint for {svc}: one network interface per {tier} subnet ({}), USD {:.3}/h each = USD {monthly:.2} a month, plus USD 0.01 per GB.", subs.len(), crate::pricing::INTERFACE_ENDPOINT_HOURLY));
                if svc.starts_with("ssm") || svc == "ec2messages" {
                    info.entry(ep_node.id.clone()).or_default().push("Session Manager needs all three: ssm, ssmmessages and ec2messages.".into());
                }
                summary.insert(ep_node.id.clone(), format!("{svc} · {} AZ{} · USD {monthly:.0}/month", subs.len(), if subs.len() == 1 { "" } else { "s" }));
            }
        }
    }

    // ---- S3 buckets
    // ---- grey blocks: read-only, said so
    for o in d.nodes.iter().filter(|n| n.kind == "other") {
        let what = p(o, "what").replace('_', " ");
        info.entry(o.id.clone()).or_default().push(format!("Read-only: {what} ({}){}. The palette has no block for it; the app draws it so nothing is hidden and never changes it.{}", p(o, "aws_id"),
            if p(o, "subnet").is_empty() { String::new() } else { format!(" in {}", p(o, "subnet")) }, if p(o, "note").is_empty() { String::new() } else { format!(" {}.", p(o, "note")) }));
        summary.insert(o.id.clone(), format!("{what} · read-only{}", if p(o, "subnet").is_empty() { String::new() } else { format!(" · {}", p(o, "subnet")) }));
    }

    // ---- Lambda functions: what they act on, the schedule, the limits AWS has
    let stoppers: Vec<&Node> = d.nodes.iter().filter(|n| n.kind == "lambda" && p(n, "action") == "stop").collect();
    for f in d.nodes.iter().filter(|n| n.kind == "lambda") {
        let action = p(f, "action");
        let inst: Vec<String> = f.params.get("instances").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).map(String::from).collect()).unwrap_or_default();
        let names: Vec<String> = inst.iter().filter_map(|id| d.node(id)).map(|n| n.label.clone()).collect();
        if inst.iter().any(|id| d.node(id).map(|n| n.kind != "ec2").unwrap_or(true)) {
            cx.error(&f.id, "An instance this function acts on is no longer in the design: untick it.".to_string());
        }
        if matches!(action, "stop" | "start") && inst.is_empty() {
            cx.warn(&f.id, format!("Tick the instances to {action}: the function does nothing yet."));
        }
        let time = p(f, "time");
        if !time.is_empty() && !valid_time(time) {
            cx.error(&f.id, format!("\"{time}\" is not a time of day: write HH:MM, such as 19:00."));
        }
        if time.is_empty() {
            if matches!(action, "stop" | "start") {
                cx.warn(&f.id, "No schedule: set a time of day, else the function only runs when called by hand.".to_string());
            } else {
                info.entry(f.id.clone()).or_default().push("No schedule: the function runs when called (console, CLI, or another service).".into());
            }
        }
        let memory = f.params.get("memory").and_then(|v| v.as_f64()).unwrap_or(128.0);
        if !(128.0..=10240.0).contains(&memory) {
            cx.error(&f.id, format!("Memory is {memory} MB; Lambda allows 128 to 10240."));
        }
        let timeout = f.params.get("timeout").and_then(|v| v.as_f64()).unwrap_or(30.0);
        if !(1.0..=900.0).contains(&timeout) {
            cx.error(&f.id, format!("Timeout is {timeout} s; Lambda allows 1 to 900."));
        }
        let runs = match p(f, "days") { "daily" => 30, "weekends" => 8, _ => 22 };
        info.entry(f.id.clone()).or_default().push(format!("Free: Lambda's free tier is 1,000,000 requests and 400,000 GB-seconds a month and the schedule (EventBridge Scheduler) is free too; this runs about {runs} times a month.{}", if matches!(action, "stop" | "start") { " A stopped instance pays only for its volume and address." } else { "" }));
        summary.insert(f.id.clone(), if matches!(action, "stop" | "start") {
            format!("{action} {}{}", if names.is_empty() { "nothing yet".to_string() } else { names.join(", ") }, if time.is_empty() { String::new() } else { format!(" · {time} {} ({})", p(f, "days"), p(f, "tz")) })
        } else {
            format!("custom Python{}", if time.is_empty() { String::new() } else { format!(" · {time} {}", p(f, "days")) })
        });
    }
    // an instance priced for part of the month should really be stopped the rest of it
    for n in d.nodes.iter().filter(|n| n.kind == "ec2") {
        let hours = pricing::hours_of(n);
        let scheduled = stoppers.iter().any(|f| f.params.get("instances").and_then(|v| v.as_array()).map(|a| a.iter().any(|v| v.as_str() == Some(&n.id))).unwrap_or(false));
        if hours < pricing::HOURS && !scheduled {
            info.entry(n.id.clone()).or_default().push(format!("Priced for {hours:.0} h a month: that holds only if it is stopped the rest of the time. \"Schedule stop and start\" below adds the Lambda functions that do it."));
        }
    }

    for b in d.nodes.iter().filter(|n| n.kind == "s3") {
        let gb = b.params.get("gb").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let life = p(b, "lifecycle");
        info.entry(b.id.clone()).or_default().push("Public access blocked and server-side encryption on (both free); the bucket's name gets a unique suffix from AWS.".into());
        info.entry(b.id.clone()).or_default().push(format!("Storage: {gb:.0} GB × USD 0.023 = USD {:.2} a month in Standard{}", gb * 0.023, match life { "ia-30" => "; objects older than 30 days move to Infrequent Access (USD 0.0125/GB)".to_string(), "glacier-90" => "; older than 90 days to Glacier Instant Retrieval (USD 0.004/GB)".to_string(), "ia-30-glacier-90" => "; 30 days → Infrequent Access, 90 days → Glacier".to_string(), _ => String::new() }));
        if gb >= 50.0 && life == "none" {
            cx.warn(&b.id, "No lifecycle rule: everything stays in Standard storage for ever. Moving objects older than 30 days to Infrequent Access costs about half; Glacier a fifth.");
        }
        if b.params.get("gb").and_then(|v| v.as_f64()).unwrap_or(0.0) < 0.0 || b.params.get("expire_days").and_then(|v| v.as_f64()).unwrap_or(0.0) < 0.0 {
            cx.error(&b.id, "Storage and expiry days cannot be negative.".to_string());
        }
        if b.params.get("versioning").and_then(|v| v.as_bool()).unwrap_or(false) && b.params.get("expire_days").and_then(|v| v.as_u64()).unwrap_or(0) == 0 {
            info.entry(b.id.clone()).or_default().push("Versioning keeps every overwritten and deleted object (and charges for it); an expire rule bounds that.".into());
        }
        summary.insert(b.id.clone(), format!("{gb:.0} GB · USD {:.2}/month{}", crate::pricing::s3_monthly(b), if life != "none" { format!(" · {life}") } else { String::new() }));
    }

    // ---- arrows between the wrong kinds (made by hand in the file)
    let mut edge_results = vec![];
    for e in &d.edges {
        let (Some(s), Some(t)) = (d.node(&e.source), d.node(&e.target)) else {
            edge_results.push(EdgeResult { id: e.id.clone(), status: "error".into(), messages: vec!["This arrow's block no longer exists.".into()], label: String::new() });
            continue;
        };
        if edge_kind(&s.kind, &t.kind) != Some(e.kind.as_str()) {
            cx.error(&e.id, format!("An arrow from a {} to a {} means nothing here. Subnets attach to route tables; route tables route to gateways and gateway endpoints; security groups and address ranges carry traffic.", title(&s.kind), title(&t.kind)));
        }
        if e.kind == "route" && s.parent != t.parent.as_ref().and_then(|pid| d.node(pid)).and_then(|n| if n.kind == "subnet" { n.parent.clone() } else { Some(n.id.clone()) }) {
            cx.error(&e.id, format!("{} and {} are in different VPCs: a route table can only route to gateways of its own VPC.", s.label, t.label));
        }
        let label = match e.kind.as_str() {
            "route" => ep(e, "dest").to_string(),
            "attach" => "uses".into(),
            "traffic" => traffic_label(ep(e, "proto"), ep(e, "ports")),
            "target" => format!("→ :{}", e.params.get("port").and_then(|v| v.as_u64()).unwrap_or(8080)),
            _ => String::new(),
        };
        edge_results.push(EdgeResult { id: e.id.clone(), status: cx.status.get(&e.id).cloned().unwrap_or_else(|| "ok".into()), messages: cx.msgs.get(&e.id).cloned().unwrap_or_default(), label });
    }

    // ---- duplicate labels (the text needs them unique)
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for n in &d.nodes {
        *seen.entry(n.label.as_str()).or_default() += 1;
    }
    for n in &d.nodes {
        if seen.get(n.label.as_str()).copied().unwrap_or(0) > 1 {
            cx.error(&n.id, format!("Two blocks are called {}. Give one another name.", n.label));
        }
    }

    // ---- gather
    let mut nodes = vec![];
    let mut problems = vec![];
    let (mut errors, mut warnings) = (0, 0);
    for n in &d.nodes {
        let status = cx.status.get(&n.id).cloned().unwrap_or_else(|| "ok".into());
        let messages = cx.msgs.get(&n.id).cloned().unwrap_or_default();
        match status.as_str() {
            "error" => errors += 1,
            "warn" => warnings += 1,
            _ => {}
        }
        for m in &messages {
            problems.push(format!("{} ({}): {m}", n.label, title(&n.kind)));
        }
        nodes.push(NodeResult {
            id: n.id.clone(),
            kind: n.kind.clone(),
            title: title(&n.kind).into(),
            status,
            messages,
            info: info.remove(&n.id).unwrap_or_default(),
            summary: summary.remove(&n.id).unwrap_or_default(),
        });
    }
    for er in &edge_results {
        match er.status.as_str() {
            "error" => errors += 1,
            "warn" => warnings += 1,
            _ => {}
        }
        if let Some(e) = d.edges.iter().find(|e| e.id == er.id) {
            let s = d.node(&e.source).map(|n| n.label.as_str()).unwrap_or("?");
            let t = d.node(&e.target).map(|n| n.label.as_str()).unwrap_or("?");
            for m in &er.messages {
                problems.push(format!("{s} → {t}: {m}"));
            }
        }
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for n in &d.nodes {
        *counts.entry(n.kind.clone()).or_default() += 1;
    }
    let mut azs: Vec<&str> = d.nodes.iter().filter(|n| n.kind == "subnet").map(|n| p(n, "az")).collect();
    azs.sort_unstable();
    azs.dedup();
    Analysis { nodes, edges: edge_results, problems, ok: errors == 0, errors, warnings, counts, az_spread: azs.len(), az_names, recommendations: crate::tidy::recommend(d), fixes: crate::fix::fixes(d), cost: pricing::cost(d) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl;

    fn design(text: &str) -> Design {
        dsl::parse(text, None).unwrap_or_else(|e| panic!("{e:?}"))
    }
    fn status(a: &Analysis, d: &Design, label: &str) -> String {
        let id = &d.by_label(label).unwrap().id;
        a.nodes.iter().find(|n| &n.id == id).unwrap().status.clone()
    }
    fn messages(a: &Analysis, d: &Design, label: &str) -> String {
        let id = &d.by_label(label).unwrap().id;
        a.nodes.iter().find(|n| &n.id == id).unwrap().messages.join(" | ")
    }

    const GOOD: &str = "net \"two az\"\nregion ap-south-1\nazs 2\n\nvpc main 10.0.0.0/16\nigw igw in main\n\
        subnet pub_a public 10.0.0.0/20 az a in main rt pub\nsubnet pub_b public 10.0.16.0/20 az b in main rt pub\n\
        subnet app_a private 10.0.32.0/20 az a in main rt app_a_rt\nsubnet app_b private 10.0.48.0/20 az b in main rt app_b_rt\n\
        subnet db_a isolated 10.0.64.0/24 az a in main\nsubnet db_b isolated 10.0.65.0/24 az b in main\n\
        nat nat_a in pub_a\nnat nat_b in pub_b\n\
        rt pub in main: 0.0.0.0/0 -> igw\nrt app_a_rt in main: 0.0.0.0/0 -> nat_a\nrt app_b_rt in main: 0.0.0.0/0 -> nat_b\n";

    #[test]
    fn a_good_two_az_design_is_clean() {
        let d = design(GOOD);
        let a = analyse(&d);
        assert!(a.ok, "{:?}", a.problems);
        assert_eq!(a.warnings, 0, "{:?}", a.problems);
        assert_eq!(a.az_spread, 2);
        assert_eq!(a.counts["subnet"], 6);
        let pub_a = a.nodes.iter().find(|n| n.id == d.by_label("pub_a").unwrap().id).unwrap();
        assert!(pub_a.info[0].contains("4096 addresses − 5 reserved by AWS = 4091 usable"), "{:?}", pub_a.info);
        assert!(pub_a.summary.contains("ap-south-1a"));
    }

    #[test]
    fn cidr_errors_are_caught() {
        let d = design("vpc main 10.0.0.0/16\nvpc big 10.0.0.0/8\nsubnet s public 10.1.0.0/24 az a in main\nsubnet t public 10.0.0.0/20 az a in main\nsubnet u public 10.0.8.0/24 az b in main\nsubnet v public 10.0.0.0/29 az a in main\nsubnet w public hello az a in main\n");
        let a = analyse(&d);
        assert!(!a.ok);
        assert!(messages(&a, &d, "big").contains("between /16 and /28"));
        assert!(messages(&a, &d, "s").contains("outside the VPC"), "{}", messages(&a, &d, "s"));
        assert!(messages(&a, &d, "t").contains("overlaps u"));
        assert!(messages(&a, &d, "u").contains("overlaps t"));
        assert!(messages(&a, &d, "v").contains("/29"));
        assert!(messages(&a, &d, "w").contains("CIDR:"));
    }

    #[test]
    fn routing_contradictions() {
        let d = design("vpc main 10.0.0.0/16\nigw igw in main\nsubnet a public 10.0.0.0/24 az a in main\nsubnet b private 10.0.1.0/24 az a in main rt pub\n\
            subnet c isolated 10.0.2.0/24 az a in main rt pub\nsubnet d private 10.0.3.0/24 az a in main\nnat n in d\nrt pub in main: 0.0.0.0/0 -> igw\nrt lone in main\n");
        let a = analyse(&d);
        assert_eq!(status(&a, &d, "a"), "error");
        assert!(messages(&a, &d, "a").contains("needs a route table"));
        assert!(messages(&a, &d, "b").contains("makes it public"));
        assert!(messages(&a, &d, "c").contains("Marked isolated"));
        assert_eq!(status(&a, &d, "d"), "warn");
        assert!(messages(&a, &d, "n").contains("must sit in a public subnet"));
        assert!(messages(&a, &d, "n").contains("No route table sends traffic here"));
        assert!(messages(&a, &d, "lone").contains("No subnet uses"));
    }

    #[test]
    fn nat_in_another_az_and_region_limits() {
        let d = design("region us-west-1\nazs 3\nvpc main 10.0.0.0/16\nigw igw in main\nsubnet pub_a public 10.0.0.0/24 az a in main rt pub\n\
            subnet app_b private 10.0.1.0/24 az b in main rt app\nnat nat_a in pub_a\nrt pub in main: 0.0.0.0/0 -> igw\nrt app in main: 0.0.0.0/0 -> nat_a\n");
        let a = analyse(&d);
        assert!(messages(&a, &d, "app_b").contains("is in AZ a"));
        assert!(messages(&a, &d, "main").contains("us-west-1 has 2 Availability Zones"));
    }

    #[test]
    fn security_groups_and_instances() {
        let d = design("vpc main 10.0.0.0/16\nigw igw in main\nsubnet pub public 10.0.0.0/24 az a in main rt pub_rt\nsubnet app private 10.0.1.0/24 az a in main\n\
            ec2 web_1 t4g.micro in pub sg web public\nec2 job t3.small in app public\nec2 lonely t4g.nano in app sg db key k\nrt pub_rt in main: 0.0.0.0/0 -> igw\n\
            sg web in main\nsg db in main\nsg idle in main egress rules\n\ncidr internet 0.0.0.0/0\ncidr office 203.0.113.0/24\ncidr unused 198.51.100.0/24\n\n\
            rule internet -> web tcp 443\nrule internet -> web tcp ssh\nrule office -> db tcp postgres\nrule web -> db tcp 5432\nrule web -> internet tcp 80\n");
        let a = analyse(&d);
        assert!(messages(&a, &d, "web_1").contains("no key pair"), "{}", messages(&a, &d, "web_1"));
        assert!(messages(&a, &d, "job").contains("public IP on an instance in a private subnet"));
        assert!(messages(&a, &d, "job").contains("No security group"));
        assert_eq!(status(&a, &d, "lonely"), "ok", "{}", messages(&a, &d, "lonely"));
        assert!(messages(&a, &d, "web").contains("Egress is \"all\""), "web -> internet with egress all: {}", messages(&a, &d, "web"));
        assert!(!messages(&a, &d, "db").contains("Egress"), "{}", messages(&a, &d, "db"));
        assert!(messages(&a, &d, "idle").contains("Nothing uses"));
        assert!(messages(&a, &d, "unused").contains("No rule uses"));
        let em: Vec<String> = a.edges.iter().flat_map(|e| e.messages.clone()).collect();
        assert!(em.iter().any(|m| m.contains("ssh open to the whole internet")), "{em:?}");
        assert!(!em.iter().any(|m| m.contains("postgres open")), "office is not the world: {em:?}");
        let web = a.nodes.iter().find(|n| n.id == d.by_label("web").unwrap().id).unwrap();
        assert!(web.info.iter().any(|i| i == "in: tcp 443 (https) from internet"), "{:?}", web.info);
        assert!(web.summary.starts_with("2 in · all out · 1 instance"), "{}", web.summary);
        let job = a.nodes.iter().find(|n| n.id == d.by_label("job").unwrap().id).unwrap();
        assert!(job.info.iter().any(|i| i.contains("t4g.small (Graviton, same size) would save")), "{:?}", job.info);
        assert!(a.cost.items.iter().any(|i| i.kind == "ec2"));
        assert!(a.cost.total > 20.0, "{}", a.cost.total);
    }

    #[test]
    fn balancer_rules() {
        let d = design("region ap-south-1\nazs 2\nvpc main 10.0.0.0/16\nigw igw in main\nsubnet pub_a public 10.0.0.0/24 az a in main rt pub\nsubnet pub_b public 10.0.1.0/24 az b in main rt pub\nsubnet app_a private 10.0.2.0/24 az a in main\nrt pub in main: 0.0.0.0/0 -> igw\nsg app in main\nsg lb in main\n\
            alb web internet in main tier public sg lb port 443\nalb lonely internal in main tier private\nasg app_servers t4g.micro in main tier private min 1 max 300 desired 2 sg app\n\ncidr internet 0.0.0.0/0\n\ntarget web -> app_servers 8080\n\nrule internet -> lb tcp 443\n");
        let a = analyse(&d);
        assert!(messages(&a, &d, "web").contains("needs a certificate"), "{}", messages(&a, &d, "web"));
        assert_eq!(status(&a, &d, "web"), "error");
        assert!(messages(&a, &d, "lonely").contains("at least two Availability Zones"));
        assert!(messages(&a, &d, "lonely").contains("No security group"));
        assert!(messages(&a, &d, "app_servers").contains("will not fit"), "{}", messages(&a, &d, "app_servers"));
        assert!(messages(&a, &d, "app_servers").contains("one AZ"));
        let em: Vec<String> = a.edges.iter().flat_map(|e| e.messages.clone()).collect();
        assert!(em.iter().any(|m| m.contains("health checks would fail")), "{em:?}");
        assert!(a.cost.items.iter().any(|i| i.kind == "alb") && a.cost.items.iter().any(|i| i.kind == "asg"));
    }

    #[test]
    fn nesting_and_route_rules() {
        let mut d = design("vpc main 10.0.0.0/16\nvpc other 10.0.0.0/16\nigw g1 in main\nigw g2 in main\nsubnet s public 10.0.0.0/24 az a in main rt r\nrt r in main: 10.0.5.0/24 -> g1, 0.0.0.0/0 -> g1, 0.0.0.0/0 -> g2\n");
        // a NAT dropped straight into the VPC
        d.add("nat", Some(&d.by_label("main").unwrap().id.clone()));
        let a = analyse(&d);
        assert!(messages(&a, &d, "g1").contains("only one Internet Gateway"));
        assert!(messages(&a, &d, "nat1").contains("lives in a Subnet, not in a VPC"), "{}", messages(&a, &d, "nat1"));
        assert!(messages(&a, &d, "main").contains("overlap"));
        let edge_msgs: Vec<String> = a.edges.iter().flat_map(|e| e.messages.clone()).collect();
        assert!(edge_msgs.iter().any(|m| m.contains("inside the VPC")), "{edge_msgs:?}");
        assert!(edge_msgs.iter().any(|m| m.contains("Two routes for 0.0.0.0/0")), "{edge_msgs:?}");
        assert!(a.problems.iter().any(|p| p.starts_with("r → g1")));
    }
}
