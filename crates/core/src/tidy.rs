//! Recommendations that tidy a design: security groups with the same rules merged into one, groups whose rules are part of
//! another's folded into it, the launch-wizard groups the console makes one per instance combined, unused groups and ranges
//! deleted. Every recommendation says what changes and what it would newly let in; nothing happens until `apply`.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::model::{clean_label, traffic_label, Design, Node};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recommendation {
    /// stable while the design does not change (what it does and which blocks), so the app can apply the one it showed
    pub id: String,
    /// "delete" | "merge" | "combine"
    pub kind: String,
    pub title: String,
    pub detail: String,
    /// the group that stays (merge, combine)
    pub keep: Option<String>,
    /// the blocks that go
    pub drop: Vec<String>,
    /// the kept group's new name (combine)
    pub label: Option<String>,
    /// what members would additionally let in or out, one line each; empty = nothing opens
    pub widens: Vec<String>,
}

/// one rule as text, independent of the arrow's id: "<source key>|<proto>|<ports>"
fn key(d: &Design, other: &str, e: &crate::model::Edge, me: &str) -> String {
    let who = if other == me { "self".to_string() } else { d.node(other).map(|n| if n.kind == "cidr" { format!("cidr:{}", n.params.get("cidr").and_then(|v| v.as_str()).unwrap_or("")) } else { n.id.clone() }).unwrap_or_default() };
    format!("{who}|{}|{}", e.params.get("proto").and_then(|v| v.as_str()).unwrap_or("tcp"), e.params.get("ports").and_then(|v| v.as_str()).unwrap_or(""))
}

/// a rule key as a sentence: "tcp 22 (ssh) from internet"
fn describe(d: &Design, k: &str, inward: bool) -> String {
    let mut it = k.splitn(3, '|');
    let (who, proto, ports) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
    let name = if who == "self" { "itself".to_string() } else if let Some(c) = who.strip_prefix("cidr:") { d.nodes.iter().find(|n| n.kind == "cidr" && n.params.get("cidr").and_then(|v| v.as_str()) == Some(c)).map(|n| n.label.clone()).unwrap_or(c.to_string()) } else { d.node(who).map(|n| n.label.clone()).unwrap_or(who.to_string()) };
    format!("{} {} {}", traffic_label(proto, ports), if inward { "from" } else { "to" }, name)
}

struct Group {
    id: String,
    label: String,
    vpc: String,
    members: Vec<String>,
    inward: BTreeSet<String>,
    /// None = egress "all"
    outward: Option<BTreeSet<String>>,
}

fn groups(d: &Design) -> Vec<Group> {
    d.nodes.iter().filter(|n| n.kind == "sg").map(|g| {
        let members = d.nodes.iter().filter(|n| ((n.kind == "ec2" || n.kind == "asg") && n.params.get("sgs").and_then(|v| v.as_array()).map(|a| a.iter().any(|v| v.as_str() == Some(&g.id))).unwrap_or(false))
            || ((n.kind == "alb" || n.kind == "vpce") && n.params.get("sg").and_then(|v| v.as_str()) == Some(&g.id))).map(|n| n.label.clone()).collect();
        let inward = d.edges.iter().filter(|e| e.kind == "traffic" && e.target == g.id).map(|e| key(d, &e.source, e, &g.id)).collect();
        let outward = if g.params.get("egress").and_then(|v| v.as_str()) == Some("rules") { Some(d.edges.iter().filter(|e| e.kind == "traffic" && e.source == g.id).map(|e| key(d, &e.target, e, &g.id)).collect()) } else { None };
        Group { id: g.id.clone(), label: g.label.clone(), vpc: g.parent.clone().unwrap_or_default(), members, inward, outward }
    }).collect()
}

fn is_wizard(label: &str) -> bool {
    label.starts_with("launch_wizard") || label.starts_with("launch-wizard")
}

fn names(v: &[String]) -> String {
    v.join(", ")
}

/// what the members of `from` would newly let in (and out) when `from` is folded into `into`
fn widening(d: &Design, from: &Group, into: &Group) -> Vec<String> {
    let mut out: Vec<String> = into.inward.difference(&from.inward).map(|k| describe(d, k, true)).collect();
    match (&from.outward, &into.outward) {
        (Some(_), None) => out.push("everything out (egress \"all\")".into()),
        (Some(f), Some(i)) => out.extend(i.difference(f).map(|k| describe(d, k, false))),
        _ => {}
    }
    out
}

/// the recommendations for this design, the biggest cleanup first
pub fn recommend(d: &Design) -> Vec<Recommendation> {
    let gs = groups(d);
    let mut out = vec![];

    // ---- unused groups (AWS keeps a VPC's default group, so that one is left alone)
    let unused: Vec<&Group> = gs.iter().filter(|g| g.members.is_empty() && g.label != "default").collect();
    if !unused.is_empty() {
        let labels: Vec<String> = unused.iter().map(|g| g.label.clone()).collect();
        out.push(Recommendation {
            id: format!("delete:{}", unused.iter().map(|g| g.id.as_str()).collect::<Vec<_>>().join(",")),
            kind: "delete".into(),
            title: format!("Delete the {} security group{} nothing uses", unused.len(), if unused.len() == 1 { "" } else { "s" }),
            detail: format!("No instance, scaling group, balancer or endpoint uses {}. Their rules go with them; nothing that runs changes.{}", names(&labels),
                if gs.iter().any(|g| g.label == "default" && g.members.is_empty()) { " The VPC's default group stays: AWS keeps it." } else { "" }),
            keep: None, drop: unused.iter().map(|g| g.id.clone()).collect(), label: None, widens: vec![],
        });
    }

    // ---- unused address ranges
    let ranges: Vec<&Node> = d.nodes.iter().filter(|n| n.kind == "cidr" && !d.edges.iter().any(|e| e.source == n.id || e.target == n.id)).collect();
    if !ranges.is_empty() {
        out.push(Recommendation {
            id: format!("delete:{}", ranges.iter().map(|n| n.id.as_str()).collect::<Vec<_>>().join(",")),
            kind: "delete".into(),
            title: format!("Delete the {} address range{} no rule uses", ranges.len(), if ranges.len() == 1 { "" } else { "s" }),
            detail: format!("{} appear in no rule.", names(&ranges.iter().map(|n| n.label.clone()).collect::<Vec<_>>())),
            keep: None, drop: ranges.iter().map(|n| n.id.clone()).collect(), label: None, widens: vec![],
        });
    }

    let used: Vec<&Group> = gs.iter().filter(|g| !g.members.is_empty()).collect();
    // ---- the same rules: merge, nothing opens
    let mut same: BTreeMap<String, Vec<&Group>> = BTreeMap::new();
    for g in &used {
        let signature = format!("{}
{}
{}", g.vpc, g.inward.iter().cloned().collect::<Vec<_>>().join(";"), g.outward.as_ref().map(|s| s.iter().cloned().collect::<Vec<_>>().join(";")).unwrap_or("*".into()));
        same.entry(signature).or_default().push(g);
    }
    // the groups a merge above deletes take no further part
    let mut merged: BTreeSet<String> = BTreeSet::new();
    for cluster in same.values().filter(|c| c.len() > 1) {
        let keep = cluster.iter().max_by_key(|g| (g.members.len(), !is_wizard(&g.label), std::cmp::Reverse(g.id.clone()))).unwrap();
        let drops: Vec<&&Group> = cluster.iter().filter(|g| g.id != keep.id).collect();
        merged.extend(drops.iter().map(|g| g.id.clone()));
        let movers: Vec<String> = drops.iter().flat_map(|g| g.members.clone()).collect();
        out.push(Recommendation {
            id: format!("merge:{}:{}", keep.id, drops.iter().map(|g| g.id.as_str()).collect::<Vec<_>>().join(",")),
            kind: "merge".into(),
            title: format!("Merge {} into {}: the same rules", names(&drops.iter().map(|g| g.label.clone()).collect::<Vec<_>>()), keep.label),
            detail: format!("{} let in exactly what {} lets in. {} switch to {}; the other group{} deleted. Nothing opens or closes.",
                names(&drops.iter().map(|g| g.label.clone()).collect::<Vec<_>>()), keep.label, names(&movers), keep.label, if drops.len() == 1 { " is" } else { "s are" }),
            keep: Some(keep.id.clone()), drop: drops.iter().map(|g| g.id.clone()).collect(), label: None, widens: vec![],
        });
    }

    // ---- rules that are part of another group's: fold in, saying what opens
    for g in used.iter().filter(|g| !merged.contains(&g.id)) {
        let supers: Vec<&&Group> = used.iter().filter(|h| h.id != g.id && h.vpc == g.vpc && !merged.contains(&h.id) && g.inward.is_subset(&h.inward)
            && match (&g.outward, &h.outward) { (_, None) => true, (Some(a), Some(b)) => a.is_subset(b), (None, Some(_)) => false })
            .collect();
        let Some(into) = supers.iter().min_by_key(|h| (h.inward.len() - g.inward.len(), h.id.clone())) else { continue };
        let widens = widening(d, g, into);
        out.push(Recommendation {
            id: format!("merge:{}:{}", into.id, g.id),
            kind: "merge".into(),
            title: format!("Fold {} into {}: its rules are part of {}'s", g.label, into.label, into.label),
            detail: format!("{} switch to {} and {} is deleted. They would then also let in: {}.", names(&g.members), into.label, g.label, widens.join("; ")),
            keep: Some(into.id.clone()), drop: vec![g.id.clone()], label: None, widens,
        });
    }

    // ---- the launch-wizard groups: one group with the union of their rules
    let mut by_vpc: BTreeMap<String, Vec<&Group>> = BTreeMap::new();
    for g in used.iter().filter(|g| is_wizard(&g.label)) {
        by_vpc.entry(g.vpc.clone()).or_default().push(g);
    }
    for wizards in by_vpc.values().filter(|w| w.len() > 1) {
        let keep = wizards.iter().max_by_key(|g| (g.inward.len(), g.members.len(), std::cmp::Reverse(g.id.clone()))).unwrap();
        let union_in: BTreeSet<String> = wizards.iter().flat_map(|g| g.inward.iter().cloned()).collect();
        let all_out = wizards.iter().any(|g| g.outward.is_none());
        let union = Group { id: keep.id.clone(), label: keep.label.clone(), vpc: keep.vpc.clone(), members: vec![], inward: union_in.clone(),
            outward: if all_out { None } else { Some(wizards.iter().flat_map(|g| g.outward.clone().unwrap_or_default()).collect()) } };
        let mut widens = vec![];
        for g in wizards {
            let w = widening(d, g, &union);
            if !w.is_empty() {
                widens.push(format!("{} ({}) also: {}", names(&g.members), g.label, w.join("; ")));
            }
        }
        let label = if d.nodes.iter().any(|n| n.label == "servers") { "launch_wizards".to_string() } else { "servers".to_string() };
        let drops: Vec<String> = wizards.iter().filter(|g| g.id != keep.id).map(|g| g.id.clone()).collect();
        out.push(Recommendation {
            id: format!("combine:{}:{}", keep.id, drops.join(",")),
            kind: "combine".into(),
            title: format!("Combine the {} launch-wizard groups into one group \"{}\"", wizards.len(), label),
            detail: format!("The console made one group per launch ({}). One group \"{}\" gets all their rules: {}. {}",
                names(&wizards.iter().map(|g| g.label.clone()).collect::<Vec<_>>()), label,
                union_in.iter().map(|k| describe(d, k, true)).collect::<Vec<_>>().join("; "),
                if widens.is_empty() { "Nothing opens that was not open." } else { "Afterwards delete the arrows that are not needed: every member then lets in the union." }),
            keep: Some(keep.id.clone()), drop: drops, label: Some(label), widens,
        });
    }
    out
}

/// carry out a recommendation (one the design still has; find it again with `recommend` first)
pub fn apply(d: &mut Design, rec: &Recommendation) {
    match rec.kind.as_str() {
        "delete" => d.remove(&rec.drop),
        _ => {
            let Some(keep) = rec.keep.clone() else { return };
            let drop: Vec<String> = rec.drop.iter().filter(|id| **id != keep).cloned().collect();
            // the members switch to the kept group
            for n in d.nodes.iter_mut() {
                if let Some(arr) = n.params.get_mut("sgs").and_then(|v| v.as_array_mut()) {
                    let mut ids: Vec<String> = arr.iter().filter_map(|v| v.as_str()).map(|s| if drop.contains(&s.to_string()) { keep.clone() } else { s.to_string() }).collect();
                    let mut seen = HashMap::new();
                    ids.retain(|x| seen.insert(x.clone(), ()).is_none());
                    *arr = ids.into_iter().map(|s| json!(s)).collect();
                }
                if let Some(sg) = n.params.get("sg").and_then(|v| v.as_str()) {
                    if drop.contains(&sg.to_string()) {
                        n.params.insert("sg".into(), json!(keep));
                    }
                }
            }
            // the kept group lets out everything when any of the folded groups did
            let any_all = rec.kind == "combine" && drop.iter().chain(std::iter::once(&keep)).any(|id| d.node(id).map(|n| n.params.get("egress").and_then(|v| v.as_str()) != Some("rules")).unwrap_or(false));
            // the arrows move over, once each
            for e in d.edges.iter_mut() {
                if drop.contains(&e.source) {
                    e.source = keep.clone();
                }
                if drop.contains(&e.target) {
                    e.target = keep.clone();
                }
            }
            let mut seen: BTreeSet<String> = BTreeSet::new();
            d.edges.retain(|e| seen.insert(format!("{}|{}|{}|{}", e.source, e.target, e.kind, serde_json::to_string(&e.params).unwrap_or_default())));
            d.remove(&drop);
            if let Some(k) = d.node_mut(&keep) {
                if any_all {
                    k.params.insert("egress".into(), json!("all"));
                }
                if let Some(l) = &rec.label {
                    k.label = clean_label(l);
                }
            }
            d.ensure_labels();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;

    const TEXT: &str = "net t\nregion ap-south-1\nazs 1\n\nvpc main 10.0.0.0/16\nsubnet pa public 10.0.0.0/20 az a in main\n\
ec2 one t4g.micro in pa sg launch_wizard_1 key k\nec2 two t4g.micro in pa sg launch_wizard_2 key k\nec2 three t4g.micro in pa sg launch_wizard_3 key k\nec2 four t4g.micro in pa sg web,db key k\n\
sg launch_wizard_1 in main\nsg launch_wizard_2 in main\nsg launch_wizard_3 in main\nsg web in main\nsg db in main\nsg idle in main\nsg default in main\n\
cidr internet 0.0.0.0/0\ncidr office 203.0.113.0/24\ncidr old 198.51.100.0/24\n\n\
rule internet -> launch_wizard_1 tcp 22\nrule internet -> launch_wizard_2 tcp 22\nrule internet -> launch_wizard_3 tcp 22\nrule internet -> launch_wizard_3 tcp 80\n\
rule office -> web tcp 443\nrule web -> db tcp 5432\n";

    #[test]
    fn recommends_and_applies() {
        let d = parse(TEXT, None).unwrap();
        let recs = recommend(&d);
        let titles: Vec<&str> = recs.iter().map(|r| r.title.as_str()).collect();
        assert!(titles[0].starts_with("Delete the 1 security group nothing uses"), "{titles:?}");
        assert!(recs[0].detail.contains("idle") && recs[0].detail.contains("default group stays"), "{}", recs[0].detail);
        assert!(titles.iter().any(|t| t.starts_with("Delete the 1 address range")), "{titles:?}");
        let merge = recs.iter().find(|r| r.kind == "merge" && r.title.contains("the same rules")).expect("wizard 1 and 2 have the same rules");
        assert!(merge.widens.is_empty());
        let fold = recs.iter().find(|r| r.title.starts_with("Fold")).expect("wizard 1/2 ⊂ wizard 3");
        assert!(fold.widens.iter().any(|w| w.contains("tcp 80 (http) from internet")), "{:?}", fold.widens);
        let combine = recs.iter().find(|r| r.kind == "combine").expect("three wizards");
        assert!(combine.title.contains("3 launch-wizard groups") && combine.label.as_deref() == Some("servers"));
        assert!(combine.widens.len() == 2, "one and two gain port 80: {:?}", combine.widens);

        // apply the combine: one group, three members, two rules, nothing dangling
        let mut d2 = d.clone();
        apply(&mut d2, combine);
        let sgs: Vec<&Node> = d2.nodes.iter().filter(|n| n.kind == "sg").collect();
        assert_eq!(sgs.len(), 5, "{:?}", sgs.iter().map(|g| &g.label).collect::<Vec<_>>());
        let servers = d2.by_label("servers").expect("renamed");
        let rules: Vec<_> = d2.edges.iter().filter(|e| e.target == servers.id).collect();
        assert_eq!(rules.len(), 2, "22 and 80 once each");
        let members = d2.nodes.iter().filter(|n| n.kind == "ec2" && n.params["sgs"].as_array().unwrap().iter().any(|v| v == &json!(servers.id))).count();
        assert_eq!(members, 3);
        assert!(d2.edges.iter().all(|e| d2.node(&e.source).is_some() && d2.node(&e.target).is_some()));
        // the text prints without the old groups
        let text = crate::dsl::print(&d2);
        assert!(!text.contains("launch_wizard"), "{text}");
        assert!(text.contains("rule internet -> servers tcp 80"), "{text}");
        // after it, no wizard recommendation is left
        assert!(recommend(&d2).iter().all(|r| r.kind != "combine"));

        // delete the unused group
        let mut d3 = d.clone();
        apply(&mut d3, &recs[0]);
        assert!(d3.by_label("idle").is_none() && d3.by_label("default").is_some());
    }

    #[test]
    fn a_tidy_design_has_nothing_to_say() {
        let d = parse("net t\nregion ap-south-1\nazs 1\n\nvpc main 10.0.0.0/16\nsubnet pa public 10.0.0.0/20 az a in main\nec2 one t4g.micro in pa sg web key k\nsg web in main\n\ncidr office 203.0.113.0/24\n\nrule office -> web tcp 22\n", None).unwrap();
        assert!(recommend(&d).is_empty());
    }
}
