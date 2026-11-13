//! The .aib text language: one line per block, kept in step with the canvas both ways.
//!
//! ```text
//! net "Two-tier web app"          # the design's name
//! region ap-south-1
//! azs 2                           # Availability Zones a, b
//!
//! vpc    main  10.0.0.0/16
//! igw    igw   in main
//! subnet pub_a public  10.0.0.0/20  az a in main rt pub     # rt = the route table the subnet uses
//! subnet app_a private 10.0.16.0/20 az a in main rt app_a
//! nat    nat_a in pub_a
//! rt     pub   in main: 0.0.0.0/0 -> igw                    # routes: destination -> gateway, separated by commas
//! rt     app_a in main: 0.0.0.0/0 -> nat_a
//! sg     web   in main                                      # a security group (egress all, AWS's default)
//! sg     db    in main egress rules                         # egress only what the rules say
//! ec2    web_1 t4g.micro in pub_a sg web key mykey public os al2023
//! cidr   internet 0.0.0.0/0                                 # an address range outside the design
//! rule   internet -> web tcp 443                            # traffic: the target lets it in from the source
//! rule   web -> db tcp postgres
//! ```
//! `kind label …`: the label names the block everywhere else (letters, digits, underscore). Typing changes the canvas once the
//! text is valid; blocks keep their place on the canvas when their label stays; moving blocks keeps the text.

use crate::model::{arrange, parse_ports, Design, Edge, Node, Params, KINDS, REGIONS, TIERS};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextError {
    /// 1-based line
    pub line: usize,
    pub message: String,
}

fn err(line: usize, message: impl Into<String>) -> TextError {
    TextError { line, message: message.into() }
}

/// what one block line says, before the labels are resolved
struct Line {
    line: usize,
    kind: String,
    label: String,
    params: Params,
    parent: Option<String>,
    /// subnet: the route table it uses
    rt: Option<String>,
    /// route table: (destination, target label)
    routes: Vec<(String, String)>,
    /// instance: the security groups it belongs to
    sgs: Vec<String>,
}

/// a `rule` line: traffic from one block to another
struct Rule {
    line: usize,
    source: String,
    target: String,
    proto: String,
    ports: String,
}

fn is_label(s: &str) -> bool {
    !s.is_empty() && s.chars().next().map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// the text as a design. `base`: the design the text belongs to, so that blocks keep their id and place on the canvas by label.
pub fn parse(text: &str, base: Option<&Design>) -> Result<Design, Vec<TextError>> {
    let mut errors = vec![];
    let mut d = Design::default();
    let mut lines: Vec<Line> = vec![];
    let mut rules: Vec<Rule> = vec![];
    let mut labels: HashMap<String, usize> = HashMap::new();
    for (i, raw) in text.lines().enumerate() {
        let ln = i + 1;
        let src = raw.split('#').next().unwrap_or("").trim();
        if src.is_empty() {
            continue;
        }
        let (head, tail) = match src.split_once(':') {
            Some((h, t)) => (h.trim(), Some(t.trim())),
            None => (src, None),
        };
        let words: Vec<&str> = head.split_whitespace().collect();
        let kw = words[0];
        match kw {
            "net" => {
                let name = head[3..].trim().trim_matches('"').trim();
                d.name = name.to_string();
                continue;
            }
            "region" => {
                match words.get(1) {
                    Some(r) if words.len() == 2 => d.region = r.to_string(),
                    _ => errors.push(err(ln, "region: write one region, such as `region ap-south-1`")),
                }
                continue;
            }
            "azs" => {
                match words.get(1).and_then(|v| v.parse::<u8>().ok()) {
                    Some(n) if (1..=6).contains(&n) && words.len() == 2 => d.azs = n,
                    _ => errors.push(err(ln, "azs: write how many Availability Zones to use, 1 to 6, such as `azs 2`")),
                }
                continue;
            }
            "rule" => {
                // rule <source> -> <target> <proto> [ports]
                match words.as_slice() {
                    [_, src, "->", dst, proto, rest @ ..] if rest.len() <= 1 && is_label(src) && is_label(dst) => {
                        let ports = rest.first().copied().unwrap_or("").to_string();
                        if let Err(e) = parse_ports(proto, &ports) {
                            errors.push(err(ln, format!("rule: {e}")));
                        }
                        rules.push(Rule { line: ln, source: src.to_string(), target: dst.to_string(), proto: proto.to_lowercase(), ports });
                    }
                    _ => errors.push(err(ln, "rule: `rule <from> -> <to> <tcp|udp|icmp|all> [port | range | name]`, such as `rule internet -> web tcp 443`")),
                }
                continue;
            }
            _ => {}
        }
        if !KINDS.iter().any(|k| k.0 == kw) {
            errors.push(err(ln, format!("\"{kw}\" is not a block kind. Lines start with net, region, azs, or one of {}.", KINDS.iter().map(|k| k.0).collect::<Vec<_>>().join(", "))));
            continue;
        }
        let Some(label) = words.get(1).copied() else {
            errors.push(err(ln, format!("{kw}: give the block a name, such as `{kw} {kw}1 …`")));
            continue;
        };
        if !is_label(label) {
            errors.push(err(ln, format!("\"{label}\" is not a valid name: letters, digits and underscore, not starting with a digit")));
            continue;
        }
        if let Some(first) = labels.get(label) {
            errors.push(err(ln, format!("\"{label}\" is already used on line {first}. Names must be unique.")));
            continue;
        }
        labels.insert(label.to_string(), ln);
        let mut l = Line { line: ln, kind: kw.into(), label: label.into(), params: Params::new(), parent: None, rt: None, routes: vec![], sgs: vec![] };
        // the positional arguments before the keywords
        let mut rest: Vec<&str> = words[2..].to_vec();
        let mut positional: Vec<&str> = vec![];
        let mut i = 0;
        while i < rest.len() {
            match rest[i] {
                "in" | "az" | "rt" | "sg" | "key" | "os" | "egress" => {
                    let Some(v) = rest.get(i + 1) else {
                        errors.push(err(ln, format!("`{}` needs a value after it", rest[i])));
                        break;
                    };
                    match rest[i] {
                        "in" => l.parent = Some(v.to_string()),
                        "az" => { l.params.insert("az".into(), json!(v)); }
                        "rt" => l.rt = Some(v.to_string()),
                        "sg" => l.sgs = v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                        "key" => { l.params.insert("key".into(), json!(v)); }
                        "os" => { l.params.insert("os".into(), json!(v)); }
                        _ => {
                            if *v != "all" && *v != "rules" {
                                errors.push(err(ln, "egress: write `egress all` (AWS's default) or `egress rules`"));
                            }
                            l.params.insert("egress".into(), json!(v));
                        }
                    }
                    rest.drain(i..i + 2);
                }
                "public" if kw == "ec2" => {
                    l.params.insert("public_ip".into(), json!(true));
                    rest.remove(i);
                }
                "spot" if kw == "ec2" => {
                    l.params.insert("spot".into(), json!(true));
                    rest.remove(i);
                }
                "disk" if kw == "ec2" => {
                    match rest.get(i + 1).and_then(|v| v.parse::<u64>().ok()) {
                        Some(g) if (8..=16384).contains(&g) => { l.params.insert("disk".into(), json!(g)); rest.drain(i..i + 2); }
                        _ => { errors.push(err(ln, "disk: write the root volume size in GiB, 8 to 16384, such as `disk 100`")); break; }
                    }
                }
                w => {
                    positional.push(w);
                    i += 1;
                }
            }
        }
        match kw {
            "vpc" => match positional.as_slice() {
                [cidr] => { l.params.insert("cidr".into(), json!(cidr)); }
                _ => errors.push(err(ln, "vpc: `vpc <name> <cidr>`, such as `vpc main 10.0.0.0/16`")),
            },
            "subnet" => match positional.as_slice() {
                [tier, cidr] => {
                    if !TIERS.contains(tier) {
                        errors.push(err(ln, format!("subnet: the tier must be public, private or isolated, not \"{tier}\"")));
                    }
                    l.params.insert("tier".into(), json!(tier));
                    l.params.insert("cidr".into(), json!(cidr));
                    if !l.params.contains_key("az") {
                        l.params.insert("az".into(), json!("a"));
                    }
                }
                _ => errors.push(err(ln, "subnet: `subnet <name> <public|private|isolated> <cidr> az <a|b|c> in <vpc> [rt <route table>]`")),
            },
            "ec2" => match positional.as_slice() {
                [t] => {
                    if crate::model::instance_type(t).is_none() {
                        errors.push(err(ln, format!("ec2: \"{t}\" is not an instance type the app knows ({} …)", crate::model::INSTANCE_TYPES.iter().take(4).map(|i| i.0).collect::<Vec<_>>().join(", "))));
                    }
                    l.params.insert("type".into(), json!(t));
                    for (k, v) in [("os", json!("al2023")), ("key", json!("")), ("public_ip", json!(false)), ("spot", json!(false)), ("disk", json!(8))] {
                        l.params.entry(k).or_insert(v);
                    }
                }
                _ => errors.push(err(ln, "ec2: `ec2 <name> <type> in <subnet> [sg <group,group>] [key <key pair>] [public] [spot] [disk <GiB>] [os al2023|ubuntu|debian]`")),
            },
            "cidr" => match positional.as_slice() {
                [c] => { l.params.insert("cidr".into(), json!(c)); }
                _ => errors.push(err(ln, "cidr: `cidr <name> <range>`, such as `cidr internet 0.0.0.0/0`")),
            },
            "sg" => {
                l.params.entry("egress").or_insert(json!("all"));
                l.params.entry("description").or_insert(json!(""));
                if !positional.is_empty() {
                    errors.push(err(ln, format!("sg: unexpected \"{}\"; the form is `sg <name> in <vpc> [egress all|rules]`", positional.join(" "))));
                }
            }
            _ => {
                if !positional.is_empty() {
                    errors.push(err(ln, format!("{kw}: unexpected \"{}\"; the form is `{kw} <name> in <block>`{}", positional.join(" "), if kw == "rt" { ", then `: <destination> -> <gateway>, …`" } else { "" })));
                }
            }
        }
        if let Some(t) = tail {
            if kw != "rt" {
                errors.push(err(ln, "only a route table has routes after `:`"));
            } else {
                for part in t.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    match part.split_once("->") {
                        Some((dest, target)) if is_label(target.trim()) => l.routes.push((dest.trim().to_string(), target.trim().to_string())),
                        _ => errors.push(err(ln, format!("route \"{part}\": write `<destination> -> <gateway name>`, such as `0.0.0.0/0 -> igw`"))),
                    }
                }
            }
        }
        if l.parent.is_none() && !crate::model::is_region_level(kw) {
            errors.push(err(ln, format!("{kw} {label}: say where it lives with `in <{}>`", KINDS.iter().find(|k| k.0 == kw).map(|k| k.2).unwrap_or("block"))));
        }
        lines.push(l);
    }

    // resolve the labels
    let label_ok = |lab: &str| labels.contains_key(lab);
    for l in &lines {
        if let Some(p) = &l.parent {
            if !label_ok(p) {
                errors.push(err(l.line, format!("there is no block called \"{p}\"")));
            }
        }
        if let Some(r) = &l.rt {
            if !label_ok(r) {
                errors.push(err(l.line, format!("there is no route table called \"{r}\"")));
            }
        }
        for (_, t) in &l.routes {
            if !label_ok(t) {
                errors.push(err(l.line, format!("there is no gateway called \"{t}\"")));
            }
        }
        for g in &l.sgs {
            if !label_ok(g) {
                errors.push(err(l.line, format!("there is no security group called \"{g}\"")));
            }
        }
    }
    for r in &rules {
        for lab in [&r.source, &r.target] {
            if !label_ok(lab) {
                errors.push(err(r.line, format!("there is no block called \"{lab}\"")));
            }
        }
    }
    if !errors.is_empty() {
        errors.sort_by_key(|e| e.line);
        return Err(errors);
    }

    // build the design: a block with the same label as before keeps its id and place
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut fresh: Vec<String> = vec![];
    let mut next = base.map(|b| b.next_id()).unwrap_or_else(|| "n1".into());
    let bump = |next: &mut String| {
        let n: u64 = next[1..].parse().unwrap_or(0);
        let id = next.clone();
        *next = format!("n{}", n + 1);
        id
    };
    for l in &lines {
        let old = base.and_then(|b| b.by_label(&l.label)).filter(|n| n.kind == l.kind);
        let id = match old {
            Some(n) => n.id.clone(),
            None => { let id = bump(&mut next); fresh.push(id.clone()); id }
        };
        ids.insert(l.label.clone(), id.clone());
        let (x, y, w, h) = old.map(|n| (n.x, n.y, n.w, n.h)).unwrap_or_else(|| { let (w, h) = crate::model::default_size(&l.kind); (0.0, 0.0, w, h) });
        d.nodes.push(Node { id, kind: l.kind.clone(), label: l.label.clone(), params: l.params.clone(), parent: None, x, y, w, h });
    }
    for l in &lines {
        let id = ids[&l.label].clone();
        let parent = l.parent.as_ref().map(|p| ids[p].clone());
        let old_parent = base.and_then(|b| b.node(&id)).and_then(|n| n.parent.clone());
        if parent != old_parent {
            // moved into another block: its old place means nothing there
            let n = d.node_mut(&id).unwrap();
            n.x = 0.0;
            n.y = 0.0;
            fresh.push(id.clone());
        }
        d.node_mut(&id).unwrap().parent = parent;
        if l.kind == "ec2" {
            let sgs: Vec<String> = l.sgs.iter().map(|g| ids[g].clone()).collect();
            d.node_mut(&id).unwrap().params.insert("sgs".into(), json!(sgs));
        }
    }
    let mut en: u64 = base.map(|b| b.next_edge_id()[1..].parse().unwrap_or(1)).unwrap_or(1);
    let mut edge_id = |base_edge: Option<&Edge>| match base_edge {
        Some(e) => e.id.clone(),
        None => { let id = format!("e{en}"); en += 1; id }
    };
    for l in &lines {
        let src = ids[&l.label].clone();
        if let Some(r) = &l.rt {
            let tgt = ids[r].clone();
            let old = base.and_then(|b| b.edges.iter().find(|e| e.kind == "attach" && e.source == src && e.target == tgt));
            d.edges.push(Edge { id: edge_id(old), source: src.clone(), target: tgt, kind: "attach".into(), params: Params::new() });
        }
        for (dest, t) in &l.routes {
            let tgt = ids[t].clone();
            let old = base.and_then(|b| b.edges.iter().find(|e| e.kind == "route" && e.source == src && e.target == tgt));
            let mut params = Params::new();
            params.insert("dest".into(), json!(dest));
            d.edges.push(Edge { id: edge_id(old), source: src.clone(), target: tgt, kind: "route".into(), params });
        }
    }
    for r in &rules {
        let (src, tgt) = (ids[&r.source].clone(), ids[&r.target].clone());
        let old = base.and_then(|b| b.edges.iter().find(|e| e.kind == "traffic" && e.source == src && e.target == tgt
            && e.params.get("proto").and_then(|v| v.as_str()) == Some(&r.proto) && e.params.get("ports").and_then(|v| v.as_str()).unwrap_or("") == r.ports));
        let mut params = Params::new();
        params.insert("proto".into(), json!(r.proto));
        params.insert("ports".into(), json!(r.ports));
        d.edges.push(Edge { id: edge_id(old), source: src, target: tgt, kind: "traffic".into(), params });
    }
    if base.is_none() {
        arrange(&mut d);
    } else if !fresh.is_empty() {
        // new or moved blocks take their grid place; the others stay where they were (a container may grow to fit)
        let mut laid = d.clone();
        arrange(&mut laid);
        for n in d.nodes.iter_mut() {
            let Some(l) = laid.node(&n.id) else { continue };
            if fresh.contains(&n.id) {
                (n.x, n.y, n.w, n.h) = (l.x, l.y, l.w, l.h);
            } else if crate::model::is_container(&n.kind) {
                n.w = n.w.max(l.w);
                n.h = n.h.max(l.h);
            }
        }
        d.sort_parents_first();
    } else {
        d.sort_parents_first();
    }
    d.text = Some(text.to_string());
    Ok(d)
}

fn tier_rank(t: &str) -> usize {
    TIERS.iter().position(|x| *x == t).unwrap_or(TIERS.len())
}

/// the design as text (stable order: per VPC its gateway, subnets by AZ and tier, NATs, route tables)
pub fn print(d: &Design) -> String {
    let mut out = String::new();
    if !d.name.is_empty() {
        out.push_str(&format!("net \"{}\"\n", d.name.replace('"', "'")));
    }
    out.push_str(&format!("region {}\n", d.region));
    out.push_str(&format!("azs {}\n", d.azs));
    let label = |id: &str| d.node(id).map(|n| n.label.clone()).unwrap_or_else(|| "?".into());
    let p = |n: &Node, k: &str| n.params.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let rt_of = |n: &Node| d.edges.iter().find(|e| e.kind == "attach" && e.source == n.id).map(|e| label(&e.target));
    let routes_of = |n: &Node| d.edges.iter().filter(|e| e.kind == "route" && e.source == n.id).map(|e| format!("{} -> {}", e.params.get("dest").and_then(|v| v.as_str()).unwrap_or("?"), label(&e.target))).collect::<Vec<_>>();
    let line = |n: &Node| -> String {
        let place = n.parent.as_ref().map(|pid| format!(" in {}", label(pid))).unwrap_or_default();
        match n.kind.as_str() {
            "vpc" => format!("vpc {} {}{place}", n.label, p(n, "cidr")),
            "cidr" => format!("cidr {} {}", n.label, p(n, "cidr")),
            "sg" => format!("sg {}{place}{}", n.label, if p(n, "egress") == "rules" { " egress rules" } else { "" }),
            "ec2" => {
                let sgs: Vec<String> = n.params.get("sgs").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).map(label).collect()).unwrap_or_default();
                let mut s = format!("ec2 {} {}{place}", n.label, p(n, "type"));
                if !sgs.is_empty() { s.push_str(&format!(" sg {}", sgs.join(","))); }
                if !p(n, "key").is_empty() { s.push_str(&format!(" key {}", p(n, "key"))); }
                if n.params.get("public_ip").and_then(|v| v.as_bool()).unwrap_or(false) { s.push_str(" public"); }
                if n.params.get("spot").and_then(|v| v.as_bool()).unwrap_or(false) { s.push_str(" spot"); }
                if let Some(g) = n.params.get("disk").and_then(|v| v.as_u64()).filter(|g| *g != 8) { s.push_str(&format!(" disk {g}")); }
                if !p(n, "os").is_empty() && p(n, "os") != "al2023" { s.push_str(&format!(" os {}", p(n, "os"))); }
                s
            }
            "subnet" => format!("subnet {} {} {} az {}{place}{}", n.label, p(n, "tier"), p(n, "cidr"), p(n, "az"), rt_of(n).map(|r| format!(" rt {r}")).unwrap_or_default()),
            "rt" => {
                let r = routes_of(n);
                format!("rt {}{place}{}", n.label, if r.is_empty() { String::new() } else { format!(": {}", r.join(", ")) })
            }
            k => format!("{k} {}{place}", n.label),
        }
    };
    let mut done: Vec<String> = vec![];
    for v in d.nodes.iter().filter(|n| n.kind == "vpc") {
        out.push('\n');
        out.push_str(&line(v));
        out.push('\n');
        done.push(v.id.clone());
        let mut kids: Vec<&Node> = d.descendants(&v.id).iter().skip(1).filter_map(|id| d.node(id)).collect();
        let rank = |n: &Node| match n.kind.as_str() { "igw" => 0, "subnet" => 1, "nat" => 2, "ec2" => 3, "rt" => 5, "sg" => 6, _ => 4 };
        // route tables in the order of the tiers they serve (public first), so the text reads top-down like the canvas
        let rt_rank = |n: &Node| d.edges.iter().filter(|e| e.kind == "attach" && e.target == n.id).filter_map(|e| d.node(&e.source)).map(|s| tier_rank(&p(s, "tier"))).min().unwrap_or(99);
        kids.sort_by_key(|n| (rank(n), if n.kind == "rt" { rt_rank(n) } else { tier_rank(&p(n, "tier")) }, p(n, "az"), n.label.clone()));
        for k in kids {
            out.push_str(&line(k));
            out.push('\n');
            done.push(k.id.clone());
        }
    }
    let loose: Vec<&Node> = d.nodes.iter().filter(|n| !done.contains(&n.id)).collect();
    if !loose.is_empty() {
        out.push('\n');
        for n in loose {
            out.push_str(&line(n));
            out.push('\n');
        }
    }
    let traffic: Vec<&Edge> = d.edges.iter().filter(|e| e.kind == "traffic").collect();
    if !traffic.is_empty() {
        out.push('\n');
        for e in traffic {
            let ports = e.params.get("ports").and_then(|v| v.as_str()).unwrap_or("");
            out.push_str(&format!("rule {} -> {} {}{}\n", label(&e.source), label(&e.target), e.params.get("proto").and_then(|v| v.as_str()).unwrap_or("tcp"), if ports.is_empty() { String::new() } else { format!(" {ports}") }));
        }
    }
    out
}

/// after a change on the canvas: labels made unique and the text printed again
pub fn sync(d: &mut Design) {
    d.ensure_labels();
    d.sort_parents_first();
    d.text = Some(print(d));
}

/// a table of the language for the Text tab's help
pub fn help() -> Vec<(String, String)> {
    vec![
        ("net \"name\"".to_string(), "the design's name".to_string()),
        ("region <id>".into(), format!("the AWS region ({} …)", REGIONS.iter().take(4).map(|r| r.0).collect::<Vec<_>>().join(", "))),
        ("azs <n>".into(), "how many Availability Zones: a, b, c …".into()),
        ("vpc <name> <cidr>".into(), "a VPC, /16 to /28".into()),
        ("subnet <name> <tier> <cidr> az <a|b|c> in <vpc> [rt <table>]".into(), "a subnet (public, private or isolated) and the route table it uses".into()),
        ("igw <name> in <vpc>".into(), "an Internet Gateway (one per VPC)".into()),
        ("nat <name> in <subnet>".into(), "a NAT Gateway in a public subnet".into()),
        ("rt <name> in <vpc>: <dest> -> <gateway>, …".into(), "a route table and its routes".into()),
        ("sg <name> in <vpc> [egress all|rules]".into(), "a security group; egress all = AWS's default, rules = only what the rules say".into()),
        ("ec2 <name> <type> in <subnet> [sg a,b] [key <pair>] [public] [spot] [disk <GiB>] [os al2023|ubuntu|debian]".into(), "an instance; public = a public IPv4 address (costs money); spot = about 70 % cheaper, may be interrupted".into()),
        ("cidr <name> <range>".into(), "an address range outside the design, such as `cidr internet 0.0.0.0/0`".into()),
        ("rule <from> -> <to> <tcp|udp|icmp|all> [port|range|name]".into(), "traffic the target lets in from the source (and the source lets out)".into()),
        ("# …".into(), "a comment".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::arrange;

    const TEXT: &str = "net \"demo\"\nregion ap-south-1\nazs 2\n\nvpc main 10.0.0.0/16\nigw igw in main\nsubnet pub_a public 10.0.0.0/20 az a in main rt pub\nsubnet app_a private 10.0.16.0/20 az a in main rt app\nnat nat_a in pub_a\nrt pub in main: 0.0.0.0/0 -> igw\nrt app in main: 0.0.0.0/0 -> nat_a\n";

    #[test]
    fn round_trip() {
        let d = parse(TEXT, None).unwrap();
        assert_eq!(d.name, "demo");
        assert_eq!(d.nodes.len(), 7);
        assert_eq!(d.edges.len(), 4);
        let printed = print(&d);
        assert_eq!(printed, TEXT, "\n{printed}");
        let again = parse(&printed, Some(&d)).unwrap();
        assert_eq!(again.nodes, d.nodes, "ids and places kept");
        assert_eq!(again.edges, d.edges);
    }

    #[test]
    fn positions_survive_edits_by_label() {
        let mut d = parse(TEXT, None).unwrap();
        arrange(&mut d);
        let id = d.by_label("pub_a").unwrap().id.clone();
        d.node_mut(&id).unwrap().x = 999.0;
        // change the CIDR of pub_a and add a subnet: pub_a keeps its place and id
        let text = TEXT.replace("10.0.0.0/20", "10.0.0.0/22") + "subnet db_a isolated 10.0.64.0/24 az a in main\n";
        let d2 = parse(&text, Some(&d)).unwrap();
        let n = d2.by_label("pub_a").unwrap();
        assert_eq!(n.id, id);
        assert_eq!(n.x, 999.0);
        assert_eq!(n.params["cidr"], "10.0.0.0/22");
        assert_eq!(d2.by_label("db_a").unwrap().id, "n8");
        assert!(d2.by_label("db_a").unwrap().y > 0.0, "the new block is placed");
        // the attach edge keeps its id
        let e1 = d.edges.iter().find(|e| e.kind == "attach" && e.source == id).unwrap().id.clone();
        assert!(d2.edges.iter().any(|e| e.id == e1 && e.source == id));
    }

    #[test]
    fn errors_name_the_line() {
        let e = parse("vpc main 10.0.0.0/16\nsubnet a public 10.0.0.0/24 az a in nowhere\nfoo bar\nsubnet a private 10.0.1.0/24 az a in main\nrt r in main: 0.0.0.0/0 -> \nigw g\nvpc\n", None).unwrap_err();
        let lines: Vec<usize> = e.iter().map(|x| x.line).collect();
        assert_eq!(lines, [2, 3, 4, 5, 6, 7], "{e:?}");
        assert!(e[0].message.contains("no block called \"nowhere\""));
        assert!(e[1].message.contains("not a block kind"));
        assert!(e[2].message.contains("already used on line 2"));
        assert!(e[3].message.contains("route"));
        assert!(e[4].message.contains("in <vpc>"));
        assert!(e[5].message.contains("give the block a name"));
        assert!(parse("vpc main 10.0.0.0/16\nsubnet s open 10.0.0.0/24 az a in main\n", None).unwrap_err()[0].message.contains("tier"));
        assert!(parse("region\n", None).unwrap_err()[0].message.contains("region"));
        assert!(parse("azs 9\n", None).unwrap_err()[0].message.contains("1 to 6"));
    }

    const TEXT2: &str = "region ap-south-1\nazs 1\n\nvpc main 10.0.0.0/16\nigw igw in main\nsubnet pub public 10.0.0.0/24 az a in main rt pub_rt\nec2 job t3.small in pub os ubuntu\nec2 web_1 t4g.micro in pub sg web key lab public\nrt pub_rt in main: 0.0.0.0/0 -> igw\nsg db in main egress rules\nsg web in main\n\ncidr internet 0.0.0.0/0\n\nrule internet -> web tcp 443\nrule web -> db tcp postgres\nrule db -> internet all\n";

    #[test]
    fn security_groups_instances_and_rules_round_trip() {
        let d = parse(TEXT2, None).unwrap();
        assert_eq!(print(&d), TEXT2, "\n{}", print(&d));
        let web = d.by_label("web_1").unwrap();
        assert_eq!(web.params["sgs"].as_array().unwrap().len(), 1);
        assert_eq!(web.params["public_ip"], true);
        assert_eq!(d.by_label("job").unwrap().params["os"], "ubuntu");
        assert_eq!(d.edges.iter().filter(|e| e.kind == "traffic").count(), 3);
        assert_eq!(d.by_label("db").unwrap().params["egress"], "rules");
        assert!(d.by_label("internet").unwrap().parent.is_none());
        let e = parse("vpc main 10.0.0.0/16\nsg web in main\nrule nobody -> web tcp 443\nrule web -> web tcp 99999\nec2 x t9.huge in main\nrule web web tcp 1\n", None).unwrap_err();
        let has = |t: &str| e.iter().any(|x| x.message.contains(t));
        assert!(has("not an instance type") && has("no block called \"nobody\"") && has("99999") && has("rule <from> -> <to>"), "{e:?}");
        assert_eq!(e.iter().map(|x| x.line).collect::<Vec<_>>(), [3, 4, 5, 6]);
    }

    #[test]
    fn sync_prints_labels() {
        let mut d = Design::default();
        let v = d.add("vpc", None).unwrap();
        let s = d.add("subnet", Some(&v)).unwrap();
        let rt = d.add("rt", Some(&v)).unwrap();
        d.connect(&s, &rt);
        sync(&mut d);
        let t = d.text.clone().unwrap();
        assert!(t.contains("vpc vpc1 10.0.0.0/16\n"), "{t}");
        assert!(t.contains("subnet subnet1 public 10.0.0.0/20 az a in vpc1 rt rt1\n"), "{t}");
        assert!(t.contains("rt rt1 in vpc1\n"), "{t}");
        let back = parse(&t, Some(&d)).unwrap();
        assert_eq!(back.nodes.len(), 3);
    }
}
