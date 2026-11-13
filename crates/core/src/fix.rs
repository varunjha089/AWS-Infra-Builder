//! Fixes: for every problem the checks can repair, what the app would change at a click. `fixes(design)` mirrors the rules in
//! `checks.rs` (same conditions, so a fix exists exactly when its problem shows); `apply` makes the change. A fix is `safe` when it
//! costs nothing, opens nothing and needs nothing from outside; "Fix all safe ones" runs those. Allowing a port only from the
//! owner's own address needs the address (the app looks it up and passes it in).
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cidr::{next_free, Cidr};
use crate::model::{instance_type, parse_ports, Design, Edge, Node, PORTS};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Fix {
    /// "<kind>:<target>", stable while the design does not change
    pub id: String,
    pub kind: String,
    /// the block or arrow it changes
    pub target: String,
    pub target_label: String,
    /// the button: what will be done, in a few words
    pub title: String,
    /// what it costs or changes beyond the problem ("" = nothing)
    pub note: String,
    /// costs nothing, opens nothing, needs nothing looked up
    pub safe: bool,
    /// needs the owner's public address
    pub needs_ip: bool,
}

fn p<'a>(n: &'a Node, k: &str) -> &'a str {
    n.params.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

fn ep<'a>(e: &'a Edge, k: &str) -> &'a str {
    e.params.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

/// the type the app knows that is nearest to an unknown one: the same size in a family of the same letter, else the smallest
pub fn nearest_type(t: &str) -> &'static str {
    let (family, size) = t.split_once('.').unwrap_or((t, "micro"));
    let first = family.chars().next().unwrap_or('t');
    if first == 'g' || first == 'p' {
        return "g4dn.xlarge";
    }
    let families: &[&str] = match first { 'm' => &["m5", "m6g"], 'c' => &["c5", "c6g"], 'r' => &["r6g"], _ => &["t3", "t4g"] };
    for f in families {
        if let Some(it) = instance_type(&format!("{f}.{size}")) {
            return it.0;
        }
    }
    "t3.micro"
}

fn fix(kind: &str, n: &Node, title: String, note: &str, safe: bool) -> Fix {
    Fix { id: format!("{kind}:{}", n.id), kind: kind.into(), target: n.id.clone(), target_label: n.label.clone(), title, note: note.into(), safe, needs_ip: false }
}

/// every fix the design offers, in the order of the checks
pub fn fixes(d: &Design) -> Vec<Fix> {
    let mut out = vec![];
    let world = |id: &str| d.node(id).map(|s| s.kind == "cidr" && Cidr::parse(p(s, "cidr")).map(|c| c.prefix == 0).unwrap_or(false)).unwrap_or(false);

    for v in d.nodes.iter().filter(|n| n.kind == "vpc") {
        if let Ok(c) = Cidr::parse(p(v, "cidr")) {
            if c.to_string() != p(v, "cidr").trim() {
                out.push(fix("network_cidr", v, format!("Write {c} (the network address)"), "", true));
            }
        }
        let igws: Vec<&Node> = d.children(&v.id).into_iter().filter(|n| n.kind == "igw").collect();
        if igws.len() > 1 {
            // keep the gateway that routes point at
            let used = |g: &Node| d.edges.iter().any(|e| e.kind == "route" && e.target == g.id);
            for g in igws.iter().filter(|g| !used(g)).skip(if igws.iter().any(|g| used(g)) { 0 } else { 1 }) {
                out.push(fix("delete", g, "Delete this extra gateway".into(), "", true));
            }
        }
    }

    for s in d.nodes.iter().filter(|n| n.kind == "subnet") {
        let vpc = s.parent.as_ref().and_then(|pid| d.node(pid)).filter(|n| n.kind == "vpc");
        let own = Cidr::parse(p(s, "cidr"));
        let bad = match (&own, vpc.and_then(|v| Cidr::parse(p(v, "cidr")).ok())) {
            (Err(_), _) => true,
            (Ok(c), Some(vc)) => !vc.contains(c) || d.children(&vpc.unwrap().id).into_iter().any(|o| o.kind == "subnet" && o.id != s.id && Cidr::parse(p(o, "cidr")).map(|oc| oc.overlaps(c)).unwrap_or(false)),
            _ => false,
        };
        if let Ok(c) = &own {
            if !bad && c.to_string() != p(s, "cidr").trim() {
                out.push(fix("network_cidr", s, format!("Write {c} (the network address)"), "", true));
            }
        }
        if bad {
            if let Some(c) = propose(d, s) {
                out.push(fix("propose_cidr", s, format!("Use the free block {c}"), "", true));
            }
        }
        let tier = p(s, "tier");
        let igw = vpc.and_then(|v| d.children(&v.id).into_iter().find(|n| n.kind == "igw"));
        match d.edges.iter().find(|e| e.kind == "attach" && e.source == s.id).and_then(|e| d.node(&e.target)) {
            None if tier == "public" && igw.is_some() => out.push(fix("route_to_igw", s, "Attach a route table with 0.0.0.0/0 → Internet Gateway".into(), "a public table of this VPC is reused, else one is added", true)),
            Some(rt) => {
                let default_to: Vec<&Node> = d.edges.iter().filter(|e| e.kind == "route" && e.source == rt.id && ep(e, "dest").trim() == "0.0.0.0/0").filter_map(|e| d.node(&e.target)).collect();
                let to_igw = default_to.iter().any(|t| t.kind == "igw");
                let to_nat = default_to.iter().any(|t| t.kind == "nat");
                match tier {
                    "public" if !to_igw && igw.is_some() && default_to.is_empty() => out.push(fix("route_to_igw", s, format!("Add 0.0.0.0/0 → Internet Gateway to {}", rt.label), "every subnet using that table becomes public", true)),
                    "public" if !to_igw => out.push(fix("tier", s, "Mark the subnet private".into(), "", true)),
                    "private" if to_igw => out.push(fix("tier", s, "Mark the subnet public".into(), "", true)),
                    "isolated" if to_igw => out.push(fix("tier", s, "Mark the subnet public".into(), "", true)),
                    "isolated" if to_nat => out.push(fix("tier", s, "Mark the subnet private".into(), "", true)),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // a NAT Gateway in a private subnet: move it to a public one of the VPC (same AZ first), else make its subnet public
    for n in d.nodes.iter().filter(|n| n.kind == "nat") {
        let Some(s) = n.parent.as_ref().and_then(|pid| d.node(pid)).filter(|s| s.kind == "subnet" && p(s, "tier") != "public") else { continue };
        match nat_home(d, n).and_then(|id| d.node(&id)) {
            Some(home) => out.push(fix("nat_move", n, format!("Move it to {} (public, AZ {})", home.label, p(home, "az")), "", true)),
            None => out.push(fix("tier", s, "Mark the subnet public".into(), "it then needs a route to the Internet Gateway too (a Fix appears for that)", true)),
        }
    }

    // the image's own needs: a big enough root volume, an x86 type for an x86-only image
    for n in d.nodes.iter().filter(|n| n.kind == "ec2") {
        let Some(o) = crate::model::os_info(p(n, "os")) else { continue };
        let disk = n.params.get("disk").and_then(|v| v.as_u64()).unwrap_or(8);
        if disk < o.5 {
            out.push(fix("disk_min", n, format!("Set the root volume to {} GiB", o.5), &format!("what the image needs; about USD {:.2} a month more", (o.5 - disk) as f64 * 0.08), false));
        }
        if o.4 == "x86_64" && instance_type(p(n, "type")).map(|it| it.3 == "arm64").unwrap_or(false) {
            let twin = x86_twin(p(n, "type"));
            out.push(fix("os_arch", n, format!("Use {twin} (x86_64) for this image"), "the same size on x86; a little dearer than Graviton", false));
        }
    }

    for c in d.nodes.iter().filter(|n| n.kind == "cidr") {
        if !d.edges.iter().any(|e| e.source == c.id || e.target == c.id) {
            out.push(fix("delete", c, "Delete this range".into(), "", true));
        }
    }

    for g in d.nodes.iter().filter(|n| n.kind == "sg") {
        let members = d.nodes.iter().any(|n| ((n.kind == "ec2" || n.kind == "asg") && n.params.get("sgs").and_then(|v| v.as_array()).map(|a| a.iter().any(|v| v.as_str() == Some(&g.id))).unwrap_or(false)) || ((n.kind == "alb" || n.kind == "vpce") && p(n, "sg") == g.id));
        if !members {
            out.push(fix("delete", g, if g.label == "default" { "Delete it from the design (AWS keeps the real default group)".into() } else { "Delete this group".into() }, "", true));
        }
        if p(g, "egress") != "rules" && d.edges.iter().any(|e| e.kind == "traffic" && e.source == g.id && d.node(&e.target).map(|t| t.kind == "cidr").unwrap_or(false)) {
            out.push(fix("egress_rules", g, "Set egress to \"rules\" (only the arrows out)".into(), "instances in the group can then reach only what the arrows say: updates and package downloads may need an arrow to the internet on 443", false));
        }
    }

    let risky = |a: u16, b: u16| -> Vec<&'static str> { PORTS.iter().filter(|(n, port)| matches!(*n, "ssh" | "rdp" | "mysql" | "postgres" | "mssql" | "mongodb" | "redis") && a <= *port && *port <= b).map(|(n, _)| *n).collect() };
    for e in d.edges.iter().filter(|e| e.kind == "traffic") {
        if world(&e.source) && d.node(&e.target).map(|t| t.kind == "sg").unwrap_or(false) {
            if let Ok((a, b)) = parse_ports(ep(e, "proto"), ep(e, "ports")) {
                let r = risky(a, b);
                if !r.is_empty() {
                    let t = d.node(&e.target).map(|t| t.label.clone()).unwrap_or_default();
                    out.push(Fix { id: format!("my_ip:{}", e.id), kind: "my_ip".into(), target: e.id.clone(), target_label: format!("internet → {t}"),
                        title: format!("Allow {} only from my current address", r.join(", ")), note: "your public IPv4 address is looked up (checkip.amazonaws.com) and put in an Address range block \"my_ip\"; a home connection's address changes now and then".into(), safe: false, needs_ip: true });
                }
            }
        }
    }

    for n in d.nodes.iter().filter(|n| n.kind == "ec2" || n.kind == "asg") {
        let t = p(n, "type");
        match instance_type(t) {
            None => out.push(fix("type", n, format!("Use {} instead of {t}", nearest_type(t)), "the nearest type the app knows; keep yours if you know it exists", false)),
            Some(it) if it.3 == "x86_64" => {
                if let Some(arm) = crate::pricing::arm_equivalent(t) {
                    let save = (crate::pricing::instance_hourly(t, &d.region) - crate::pricing::instance_hourly(arm, &d.region)) * crate::pricing::hours_of(n);
                    if save > 0.0 {
                        out.push(fix("graviton", n, format!("Use {arm} (Graviton) and save USD {save:.2} a month"), "arm64: most Linux software runs unchanged, but a running instance would be replaced with a new AMI", false));
                    }
                }
            }
            _ => {}
        }
        if n.kind != "ec2" {
            continue;
        }
        let public_ip = n.params.get("public_ip").and_then(|v| v.as_bool()).unwrap_or(false);
        let sgs: Vec<&Node> = n.params.get("sgs").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).filter_map(|id| d.node(id)).collect()).unwrap_or_default();
        if let Some(s) = n.parent.as_ref().and_then(|pid| d.node(pid)).filter(|s| s.kind == "subnet") {
            let tier = p(s, "tier");
            let behind_lb = d.edges.iter().any(|e| e.kind == "target" && e.target == n.id);
            if public_ip && (tier != "public" || behind_lb) {
                out.push(fix("public_ip_off", n, "Turn the public IP off".into(), "saves USD 3.65 a month", true));
            }
            let wants_in = sgs.iter().any(|g| d.edges.iter().any(|e| e.kind == "traffic" && e.target == g.id && d.node(&e.source).map(|src| src.kind == "cidr").unwrap_or(false)));
            if tier == "public" && !public_ip && wants_in {
                out.push(fix("public_ip_on", n, "Turn the public IP on".into(), "+USD 3.65 a month; the alternative is SSM Session Manager with no inbound port", false));
            }
        }
    }

    for g in d.nodes.iter().filter(|n| n.kind == "asg") {
        let (min, desired, max) = (g.params.get("min").and_then(|v| v.as_u64()).unwrap_or(1), g.params.get("desired").and_then(|v| v.as_u64()).unwrap_or(1), g.params.get("max").and_then(|v| v.as_u64()).unwrap_or(2));
        if !(min <= desired && desired <= max) {
            out.push(fix("asg_sizes", g, format!("Set min {min} ≤ desired {} ≤ max {}", desired.clamp(min, max.max(min)), max.max(min)), "", true));
        }
    }

    for b in d.nodes.iter().filter(|n| n.kind == "s3") {
        let gb = b.params.get("gb").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if gb >= 50.0 && p(b, "lifecycle") == "none" {
            out.push(fix("lifecycle", b, "Move objects older than 30 days to Infrequent Access".into(), "about half the storage price for what is rarely read", true));
        }
    }
    out
}

/// a free block for a subnet whose CIDR is missing, outside the VPC or overlapping (the inspector's proposal, on the Rust side)
fn propose(d: &Design, s: &Node) -> Option<Cidr> {
    let vpc = s.parent.as_ref().and_then(|pid| d.node(pid)).filter(|n| n.kind == "vpc")?;
    let vc = Cidr::parse(p(vpc, "cidr")).ok()?;
    let taken: Vec<Cidr> = d.children(&vpc.id).into_iter().filter(|o| o.kind == "subnet" && o.id != s.id).filter_map(|o| Cidr::parse(p(o, "cidr")).ok()).collect();
    let prefix = Cidr::parse(p(s, "cidr")).ok().map(|c| c.prefix).filter(|p| (16..=28).contains(p)).unwrap_or((vc.prefix + 4).min(28));
    next_free(&vc, &taken, prefix)
}

/// make the change; `my_ip` is the owner's public IPv4 address for the fixes that need it
/// the public subnet of the NAT's VPC it should move to: the same AZ when there is one
fn nat_home(d: &Design, nat: &Node) -> Option<String> {
    let s = nat.parent.as_ref().and_then(|pid| d.node(pid))?;
    let vpc = s.parent.as_ref()?;
    let public: Vec<&Node> = d.children(vpc).into_iter().filter(|o| o.kind == "subnet" && p(o, "tier") == "public").collect();
    public.iter().find(|o| p(o, "az") == p(s, "az")).or(public.first()).map(|o| o.id.clone())
}

/// the x86 type of the same size as a Graviton one (t4g -> t3, m6g -> m5, c6g -> c5), else t3 of that size, else t3.micro
fn x86_twin(t: &str) -> String {
    let (family, size) = t.split_once('.').unwrap_or((t, "micro"));
    let fam = match family { "t4g" => "t3", "m6g" => "m5", "c6g" => "c5", "r6g" => "m5", _ => "t3" };
    let want = format!("{fam}.{size}");
    if instance_type(&want).is_some() { want } else if instance_type(&format!("t3.{size}")).is_some() { format!("t3.{size}") } else { "t3.micro".into() }
}

pub fn apply(d: &mut Design, f: &Fix, my_ip: Option<&str>) -> Result<(), String> {
    let set = |d: &mut Design, k: &str, v: serde_json::Value| { if let Some(n) = d.node_mut(&f.target) { n.params.insert(k.into(), v); } };
    match f.kind.as_str() {
        "delete" => d.remove(std::slice::from_ref(&f.target)),
        "network_cidr" => {
            let c = d.node(&f.target).and_then(|n| Cidr::parse(p(n, "cidr")).ok()).ok_or("no CIDR")?;
            set(d, "cidr", json!(c.to_string()));
        }
        "propose_cidr" => {
            let s = d.node(&f.target).cloned().ok_or("block gone")?;
            let c = propose(d, &s).ok_or("no free block left in the VPC: use a smaller block or a bigger VPC")?;
            set(d, "cidr", json!(c.to_string()));
        }
        "tier" => {
            let tier = f.title.rsplit(' ').next().unwrap_or("private").to_string();
            set(d, "tier", json!(tier));
        }
        "nat_move" => {
            let nat = d.node(&f.target).cloned().ok_or("block gone")?;
            let home = nat_home(d, &nat).ok_or("no public subnet in the VPC: mark one public first")?;
            if let Some(n) = d.node_mut(&f.target) { n.parent = Some(home); n.x = 0.0; n.y = 0.0; }
        }
        "route_to_igw" => {
            let s = d.node(&f.target).cloned().ok_or("block gone")?;
            let vpc = s.parent.clone().ok_or("the subnet is in no VPC")?;
            let igw = d.children(&vpc).into_iter().find(|n| n.kind == "igw").map(|n| n.id.clone()).ok_or("no Internet Gateway in the VPC")?;
            let attached = d.edges.iter().find(|e| e.kind == "attach" && e.source == s.id).map(|e| e.target.clone());
            let rt = match attached {
                Some(rt) => rt,
                None => {
                    // a table of this VPC that already routes to the gateway, else a new one
                    let public = d.children(&vpc).into_iter().filter(|n| n.kind == "rt").find(|rt| d.edges.iter().any(|e| e.kind == "route" && e.source == rt.id && e.target == igw)).map(|n| n.id.clone());
                    let rt = match public { Some(rt) => rt, None => { let rt = d.add("rt", Some(&vpc)).ok_or("could not add a route table")?; let l = format!("public_{}", d.nodes.iter().filter(|n| n.kind == "rt").count()); d.node_mut(&rt).unwrap().label = l; rt } };
                    d.connect(&s.id, &rt);
                    rt
                }
            };
            if !d.edges.iter().any(|e| e.kind == "route" && e.source == rt && e.target == igw) {
                d.connect(&rt, &igw);
            }
        }
        "egress_rules" => set(d, "egress", json!("rules")),
        "my_ip" => {
            let ip = my_ip.ok_or("your public address could not be looked up (no internet?); add an Address range block with it by hand")?.trim().to_string();
            let cidr = format!("{ip}/32");
            Cidr::parse(&cidr).map_err(|e| format!("the address {ip} is not an IPv4 address: {e}"))?;
            let range = match d.nodes.iter().find(|n| n.kind == "cidr" && p(n, "cidr") == cidr) {
                Some(n) => n.id.clone(),
                None => {
                    let id = d.add("cidr", None).ok_or("could not add the range")?;
                    let n = d.node_mut(&id).unwrap();
                    n.label = "my_ip".into();
                    n.params.insert("cidr".into(), json!(cidr));
                    id
                }
            };
            d.ensure_labels();
            let e = d.edges.iter_mut().find(|e| e.id == f.target).ok_or("the arrow is gone")?;
            e.source = range;
            // the same rule may already exist from that range
            let mut seen = std::collections::BTreeSet::new();
            d.edges.retain(|e| seen.insert(format!("{}|{}|{}|{}", e.source, e.target, e.kind, serde_json::to_string(&e.params).unwrap_or_default())));
        }
        "type" => {
            let t = d.node(&f.target).map(|n| nearest_type(p(n, "type")).to_string()).ok_or("block gone")?;
            set(d, "type", json!(t));
        }
        "disk_min" => {
            let min = d.node(&f.target).and_then(|n| crate::model::os_info(p(n, "os"))).map(|o| o.5).ok_or("unknown image")?;
            set(d, "disk", json!(min));
        }
        "os_arch" => {
            let t = d.node(&f.target).map(|n| x86_twin(p(n, "type"))).ok_or("block gone")?;
            set(d, "type", json!(t));
        }
        "graviton" => {
            let arm = d.node(&f.target).and_then(|n| crate::pricing::arm_equivalent(p(n, "type"))).ok_or("no Graviton type of that size")?;
            set(d, "type", json!(arm));
        }
        "public_ip_off" => set(d, "public_ip", json!(false)),
        "public_ip_on" => set(d, "public_ip", json!(true)),
        "asg_sizes" => {
            let n = d.node(&f.target).ok_or("block gone")?;
            let (min, desired, max) = (n.params.get("min").and_then(|v| v.as_u64()).unwrap_or(1), n.params.get("desired").and_then(|v| v.as_u64()).unwrap_or(1), n.params.get("max").and_then(|v| v.as_u64()).unwrap_or(2));
            let max = max.max(min);
            set(d, "max", json!(max));
            set(d, "desired", json!(desired.clamp(min, max)));
        }
        "lifecycle" => set(d, "lifecycle", json!("ia-30")),
        other => return Err(format!("unknown fix {other}")),
    }
    Ok(())
}

/// apply every safe fix, one after the other (the list is made again after each, since ids and conditions move); returns how many
pub fn apply_all_safe(d: &mut Design) -> usize {
    let mut done = std::collections::BTreeSet::new();
    let mut count = 0;
    for _ in 0..200 {
        let Some(f) = fixes(d).into_iter().find(|f| f.safe && !done.contains(&f.id)) else { break };
        done.insert(f.id.clone());
        if apply(d, &f, None).is_ok() {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::analyse;
    use crate::dsl::parse;

    const TEXT: &str = "net t\nregion ap-south-1\nazs 2\n\nvpc main 10.0.0.0/16\nigw gw in main\nsubnet pa public 10.0.0.0/20 az a in main\nsubnet pb public 10.0.0.0/20 az b in main\nsubnet priv private 10.0.32.0/20 az a in main\n\
ec2 srv t3.large in pa sg web key k\nec2 dbhost t3.small in priv sg db key k public\nec2 box t3.micro in pb sg web key k\n\
rt pub in main: 0.0.0.0/0 -> gw\nsg web in main\nsg db in main\nsg idle in main\n\ncidr internet 0.0.0.0/0\ncidr old 198.51.100.0/24\n\n\
rule internet -> web tcp 22\nrule internet -> web tcp 443\nrule web -> db tcp 5432\n";

    #[test]
    fn every_fixable_problem_has_a_fix_and_the_safe_ones_clear_it() {
        let mut d = parse(TEXT, None).unwrap();
        // pa uses the public table; pb has none
        let pa = d.by_label("pa").unwrap().id.clone();
        let pub_rt = d.by_label("pub").unwrap().id.clone();
        d.connect(&pa, &pub_rt);
        // a type the app does not know (an import brings such types in; the text language refuses them)
        let srv = d.by_label("srv").unwrap().id.clone();
        d.node_mut(&srv).unwrap().params.insert("type".into(), json!("z1.weird"));
        let fx = fixes(&d);
        let kinds: Vec<&str> = fx.iter().map(|f| f.kind.as_str()).collect();
        for k in ["propose_cidr", "route_to_igw", "delete", "my_ip", "type", "graviton", "public_ip_off", "public_ip_on"] {
            assert!(kinds.contains(&k), "{k} missing in {kinds:?}");
        }
        let unknown = fx.iter().find(|f| f.kind == "type").unwrap();
        assert_eq!(unknown.title, "Use t3.micro instead of z1.weird");
        assert_eq!(nearest_type("t2.medium"), "t3.medium");
        assert_eq!(nearest_type("g5.xlarge"), "g4dn.xlarge");
        assert_eq!(nearest_type("m5.large"), "m5.large");

        let before = analyse(&d);
        let n = apply_all_safe(&mut d);
        assert!(n >= 5, "{n}");
        let after = analyse(&d);
        assert!(after.errors == 0, "{:?}", after.problems);
        assert!(after.warnings < before.warnings, "{} -> {}: {:?}", before.warnings, after.warnings, after.problems);
        assert!(d.by_label("idle").is_none() && d.by_label("old").is_none());
        // the first of the two overlapping subnets moved to the free block
        let (ca, cb) = (d.param_str(&d.by_label("pa").unwrap().id, "cidr").to_string(), d.param_str(&d.by_label("pb").unwrap().id, "cidr").to_string());
        assert!(ca != cb && (ca == "10.0.16.0/20" || cb == "10.0.16.0/20"), "{ca} {cb}");
        assert!(d.edges.iter().any(|e| e.kind == "attach" && e.source == d.by_label("pb").unwrap().id && e.target == pub_rt), "pb reuses the public table");
        assert_eq!(d.by_label("srv").unwrap().params["type"], "z1.weird", "the type is the owner's choice: not changed by the safe fixes");
        assert_eq!(d.by_label("dbhost").unwrap().params["public_ip"], false);
        // the unsafe ones are still offered
        let left: Vec<String> = fixes(&d).into_iter().map(|f| f.kind).collect();
        assert!(left.iter().any(|k| k == "my_ip") && left.iter().any(|k| k == "graviton") && left.iter().any(|k| k == "type"), "{left:?}");

        // SSH only from my address: a new range, the arrow re-pointed, 443 still open to the world
        let f = fixes(&d).into_iter().find(|f| f.kind == "my_ip").unwrap();
        assert!(apply(&mut d, &f, None).is_err());
        apply(&mut d, &f, Some("203.0.113.9\n")).unwrap();
        let me = d.by_label("my_ip").unwrap();
        assert_eq!(me.params["cidr"], "203.0.113.9/32");
        let web = d.by_label("web").unwrap().id.clone();
        let internet = d.by_label("internet").unwrap().id.clone();
        assert!(d.edges.iter().any(|e| e.source == me.id && e.target == web && e.params["ports"] == "22"));
        assert!(d.edges.iter().any(|e| e.source == internet && e.target == web && e.params["ports"] == "443"));
        assert!(fixes(&d).iter().all(|f| f.kind != "my_ip"));
        let text = crate::dsl::print(&d);
        assert!(text.contains("rule my_ip -> web tcp 22"), "{text}");
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;
    use crate::{checks, templates};

    #[test]
    fn a_deep_learning_image_on_a_small_graviton_box_gets_its_fixes() {
        let mut d = crate::dsl::parse("net \"g\"\nregion ap-south-1\nazs 1\n\nvpc main 10.0.0.0/16\nsubnet a public 10.0.0.0/24 az a in main\nec2 box t4g.micro in a os dlami\n", None).unwrap();
        let a = checks::analyse(&d);
        let msgs = a.nodes.iter().find(|n| n.title == "box" || n.id == d.by_label("box").unwrap().id).map(|n| n.messages.join(" | ")).unwrap_or_default();
        assert!(msgs.contains("x86_64 only") && msgs.contains("at least 30 GiB") && msgs.contains("meant for GPU"), "{msgs}");
        let fs = fixes(&d);
        let disk = fs.iter().find(|f| f.kind == "disk_min").unwrap();
        let arch = fs.iter().find(|f| f.kind == "os_arch").unwrap();
        assert_eq!(arch.title, "Use t3.micro (x86_64) for this image");
        apply(&mut d, disk, None).unwrap();
        apply(&mut d, arch, None).unwrap();
        let b = d.by_label("box").unwrap();
        assert_eq!((b.params["disk"].as_u64(), b.params["type"].as_str()), (Some(30), Some("t3.micro")));
        assert_eq!(x86_twin("m6g.xlarge"), "m5.xlarge");
        assert_eq!(x86_twin("r6g.2xlarge"), "t3.2xlarge");
        // an unknown type is a warning, not an error, and its fix is not among the safe ones
        d.nodes.iter_mut().find(|n| n.label == "box").unwrap().params.insert("type".into(), serde_json::json!("m7i.large"));
        let a = checks::analyse(&d);
        let r = a.nodes.iter().find(|n| n.id == d.by_label("box").unwrap().id).unwrap();
        assert_eq!(r.status, "warn", "{:?}", r.messages);
        assert!(fixes(&d).iter().any(|f| f.kind == "type" && !f.safe));
    }

    #[test]
    fn the_spot_the_mistakes_template_is_repaired_by_the_safe_fixes() {
        let mut d = templates::build("broken").unwrap();
        let before = checks::analyse(&d);
        assert!(!before.ok, "the template should start with errors");
        let n = apply_all_safe(&mut d);
        assert!(n > 0);
        let after = checks::analyse(&d);
        assert!(after.ok, "left: {:?}", after.problems);
        // the NAT now sits in a public subnet
        let nat = d.nodes.iter().find(|n| n.kind == "nat").unwrap();
        let sub = d.node(nat.parent.as_ref().unwrap()).unwrap();
        assert_eq!(sub.params["tier"], "public");
    }
}
