//! The checks: every block and arrow is looked at while the design is drawn. "error" = AWS would refuse it or the design contradicts
//! itself; "warn" = it works but is probably not what you want. Each message says what to change. Numbers (usable addresses) are
//! shown with their arithmetic, as NN Builder shows its shape formula.

use crate::cidr::Cidr;
use crate::model::{az_letters, edge_kind, parent_kind, region_azs, title, Design, Node, TIERS};
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
                    cx.warn(&v.id, format!("{} is not a network address; it is read as {c}.", p(v, "cidr").trim()));
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
                    cx.warn(&s.id, format!("{} is not a network address; it is read as {c}.", p(s, "cidr").trim()));
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
                    "private" if !to_nat => cx.warn(&s.id, format!("Marked private, but route table {} has no 0.0.0.0/0 → NAT Gateway: instances cannot reach the internet. Add the route, or mark the subnet isolated.", rt.label)),
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

    // ---- arrows between the wrong kinds (made by hand in the file)
    let mut edge_results = vec![];
    for e in &d.edges {
        let (Some(s), Some(t)) = (d.node(&e.source), d.node(&e.target)) else {
            edge_results.push(EdgeResult { id: e.id.clone(), status: "error".into(), messages: vec!["This arrow's block no longer exists.".into()], label: String::new() });
            continue;
        };
        if edge_kind(&s.kind, &t.kind) != Some(e.kind.as_str()) {
            cx.error(&e.id, format!("An arrow from a {} to a {} means nothing here. Subnets attach to route tables; route tables route to gateways.", title(&s.kind), title(&t.kind)));
        }
        if e.kind == "route" && s.parent != t.parent.as_ref().and_then(|pid| d.node(pid)).and_then(|n| if n.kind == "subnet" { n.parent.clone() } else { Some(n.id.clone()) }) {
            cx.error(&e.id, format!("{} and {} are in different VPCs: a route table can only route to gateways of its own VPC.", s.label, t.label));
        }
        let label = match e.kind.as_str() {
            "route" => ep(e, "dest").to_string(),
            "attach" => "uses".into(),
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
    Analysis { nodes, edges: edge_results, problems, ok: errors == 0, errors, warnings, counts, az_spread: azs.len(), az_names, cost: pricing::cost(d) }
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
