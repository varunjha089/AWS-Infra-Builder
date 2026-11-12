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
//! ```
//! `kind label …`: the label names the block everywhere else (letters, digits, underscore). Typing changes the canvas once the
//! text is valid; blocks keep their place on the canvas when their label stays; moving blocks keeps the text.

use crate::model::{arrange, Design, Edge, Node, Params, KINDS, REGIONS, TIERS};
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
}

fn is_label(s: &str) -> bool {
    !s.is_empty() && s.chars().next().map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// the text as a design. `base`: the design the text belongs to, so that blocks keep their id and place on the canvas by label.
pub fn parse(text: &str, base: Option<&Design>) -> Result<Design, Vec<TextError>> {
    let mut errors = vec![];
    let mut d = Design::default();
    let mut lines: Vec<Line> = vec![];
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
        let mut l = Line { line: ln, kind: kw.into(), label: label.into(), params: Params::new(), parent: None, rt: None, routes: vec![] };
        // the positional arguments before the keywords
        let mut rest: Vec<&str> = words[2..].to_vec();
        let mut positional: Vec<&str> = vec![];
        let mut i = 0;
        while i < rest.len() {
            match rest[i] {
                "in" | "az" | "rt" => {
                    let Some(v) = rest.get(i + 1) else {
                        errors.push(err(ln, format!("`{}` needs a value after it", rest[i])));
                        break;
                    };
                    match rest[i] {
                        "in" => l.parent = Some(v.to_string()),
                        "az" => { l.params.insert("az".into(), json!(v)); }
                        _ => l.rt = Some(v.to_string()),
                    }
                    rest.drain(i..i + 2);
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
        if l.parent.is_none() && kw != "vpc" {
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
        let rank = |n: &Node| match n.kind.as_str() { "igw" => 0, "subnet" => 1, "nat" => 2, "rt" => 4, _ => 3 };
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
