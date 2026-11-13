//! Layout, innermost block first: every block takes the room it needs, a subnet grows around its instances, a VPC grows around its
//! subnets, gateways, route tables and groups, and the region grows around the VPCs. Nothing overlaps: the size of a block is its
//! measured size when the canvas has one, else an estimate from the text it shows.
use std::collections::HashMap;

use crate::checks;
use crate::model::{az_letters, is_container, title, Design, SUB_H, SUB_W, TIERS};

/// block id -> (width, height) in canvas pixels
pub type Sizes = HashMap<String, (f64, f64)>;

/// the canvas draws a block at most this wide (styles.css: .block max-width)
pub const BLOCK_W: f64 = 230.0;
/// a block with one line of text
pub const BLOCK_H: f64 = 54.0;
/// between blocks inside a container
const GAP: f64 = 22.0;
/// a container's inner margin
const PAD: f64 = 24.0;
/// between the blocks inside a subnet
const INNER: f64 = 12.0;
/// a subnet's header and its "usable" line
const SUB_HEAD: f64 = 60.0;
/// a VPC's header line
const VPC_HEAD: f64 = 44.0;
/// a VPC with few subnets still wraps its route tables and groups this many to a row, so a long list grows down, not sideways
const BLOCKS_PER_ROW: usize = 5;
/// between the region's columns and between VPCs
const REGION_GAP: f64 = 40.0;

/// every non-container block's size, estimated from what it shows (the header, then the first problem or the summary) the way
/// the canvas wraps it: about 34 characters a line in a 230 px block
pub fn estimate(d: &Design) -> Sizes {
    let a = checks::analyse(d);
    let mut out = Sizes::new();
    for n in d.nodes.iter().filter(|n| !is_container(&n.kind)) {
        let r = a.nodes.iter().find(|r| r.id == n.id);
        let head = title(&n.kind).len() + n.label.len() + 3;
        let body = match r {
            Some(r) if r.status != "ok" => r.messages.first().map(|m| m.len()).unwrap_or(0),
            Some(r) => r.summary.len(),
            None => 0,
        } + n.params.get("aws_id").and_then(|v| v.as_str()).map(|s| s.len() + 3).unwrap_or(0);
        let head_lines = (head as f64 / 30.0).ceil().max(1.0);
        let body_lines = (body as f64 / 34.0).ceil().max(1.0);
        out.insert(n.id.clone(), (BLOCK_W, 8.0 + head_lines * 18.0 + body_lines * 15.0 + 10.0));
    }
    out
}

/// place blocks left to right, wrapping into a new row when the next would cross `wrap_w` (at least one a row); a row is as tall
/// as its tallest block. Returns (id, x, y) for each block, then the width and height used
fn flow(ids: &[String], size: &dyn Fn(&str) -> (f64, f64), x0: f64, y0: f64, wrap_w: f64, gap: f64) -> (Vec<(String, f64, f64)>, f64, f64) {
    let mut out = vec![];
    let (mut x, mut y, mut row_h, mut used_w) = (x0, y0, 0.0f64, 0.0f64);
    for id in ids {
        let (w, h) = size(id);
        if x > x0 && x + w > x0 + wrap_w {
            x = x0;
            y += row_h + gap;
            row_h = 0.0;
        }
        out.push((id.clone(), x, y));
        used_w = used_w.max(x + w - x0);
        row_h = row_h.max(h);
        x += w + gap;
    }
    let used_h = if ids.is_empty() { 0.0 } else { y + row_h - y0 };
    (out, used_w, used_h)
}

/// lay the whole design out with the blocks' estimated sizes
pub fn arrange(d: &mut Design) {
    arrange_with(d, &Sizes::new());
}

/// lay the whole design out; `measured` (from the canvas) wins over the estimate for the blocks it names
pub fn arrange_with(d: &mut Design, measured: &Sizes) {
    let mut sizes = estimate(d);
    for (k, v) in measured {
        if v.0 > 0.0 && v.1 > 0.0 {
            sizes.insert(k.clone(), *v);
        }
    }
    let size = |id: &str| sizes.get(id).copied().unwrap_or((BLOCK_W, BLOCK_H));

    // ---- 1. subnets: what lives inside, in a near-square grid; the subnet grows around it
    let subnet_ids: Vec<String> = d.nodes.iter().filter(|n| n.kind == "subnet").map(|n| n.id.clone()).collect();
    for sid in &subnet_ids {
        let kids: Vec<String> = d.children(sid).iter().map(|c| c.id.clone()).collect();
        let cols = ((kids.len() as f64).sqrt().ceil() as usize).max(1);
        let wrap = cols as f64 * (BLOCK_W + INNER) - INNER + 1.0;
        let (places, used_w, used_h) = flow(&kids, &size, INNER, SUB_HEAD, wrap, INNER);
        for (id, x, y) in places {
            let n = d.node_mut(&id).unwrap();
            n.x = x;
            n.y = y;
        }
        let s = d.node_mut(sid).unwrap();
        s.w = SUB_W.max(used_w + 2.0 * INNER);
        s.h = if kids.is_empty() { SUB_H } else { SUB_H.max(SUB_HEAD + used_h + INNER) };
    }

    // ---- 2. VPCs: gateways on top, the subnet grid (AZ columns × tier rows), route tables, then groups; each row wraps
    let letters = az_letters(d.azs);
    let vpcs: Vec<String> = d.nodes.iter().filter(|n| n.kind == "vpc").map(|n| n.id.clone()).collect();
    for vid in &vpcs {
        let subs: Vec<(String, String, String, f64, f64)> = d.children(vid).into_iter().filter(|n| n.kind == "subnet")
            .map(|n| (n.id.clone(), n.params.get("tier").and_then(|v| v.as_str()).unwrap_or("").to_string(), n.params.get("az").and_then(|v| v.as_str()).unwrap_or("").to_string(), n.w, n.h))
            .collect();
        let mut tiers: Vec<String> = TIERS.iter().map(|t| t.to_string()).filter(|t| subs.iter().any(|s| &s.1 == t)).collect();
        for s in &subs {
            if !tiers.contains(&s.1) {
                tiers.push(s.1.clone());
            }
        }
        let mut cols: Vec<String> = letters.clone();
        for s in &subs {
            if !cols.contains(&s.2) {
                cols.push(s.2.clone());
            }
        }
        // several subnets in one cell stand side by side in sub-columns; a column is as wide as its widest subnet, a row as tall as its tallest
        let mut slot: HashMap<(usize, usize), usize> = HashMap::new();
        let mut placed: Vec<(String, usize, usize, usize)> = vec![]; // id, row, col, sub-column
        for s in &subs {
            let r = tiers.iter().position(|x| x == &s.1).unwrap_or(0);
            let c = cols.iter().position(|x| x == &s.2).unwrap_or(0);
            let k = slot.entry((r, c)).or_insert(0);
            placed.push((s.0.clone(), r, c, *k));
            *k += 1;
        }
        let sub_w = |id: &str| subs.iter().find(|s| s.0 == id).map(|s| s.3).unwrap_or(SUB_W);
        let sub_h = |id: &str| subs.iter().find(|s| s.0 == id).map(|s| s.4).unwrap_or(SUB_H);
        let mut slot_w: HashMap<(usize, usize), f64> = HashMap::new(); // (col, sub-column) -> width
        let mut row_h: Vec<f64> = vec![0.0; tiers.len()];
        for (id, r, c, k) in &placed {
            let e = slot_w.entry((*c, *k)).or_insert(0.0);
            *e = e.max(sub_w(id));
            row_h[*r] = row_h[*r].max(sub_h(id));
        }
        let mut slot_x: HashMap<(usize, usize), f64> = HashMap::new();
        let mut x = PAD;
        for c in 0..cols.len() {
            let subcols = placed.iter().filter(|p| p.2 == c).map(|p| p.3 + 1).max().unwrap_or(1);
            for k in 0..subcols {
                slot_x.insert((c, k), x);
                x += slot_w.get(&(c, k)).copied().unwrap_or(SUB_W) + GAP;
            }
        }
        let grid_w = if subs.is_empty() { 0.0 } else { x - GAP - PAD };
        let grid_h: f64 = row_h.iter().sum::<f64>() + GAP * row_h.len().saturating_sub(1) as f64;
        let wrap_w = grid_w.max(BLOCKS_PER_ROW as f64 * (BLOCK_W + GAP) - GAP);

        let mut y = VPC_HEAD;
        let mut widest = grid_w;
        let kinds = |d: &Design, f: &dyn Fn(&str) -> bool| -> Vec<String> { d.children(vid).iter().filter(|n| f(&n.kind)).map(|n| n.id.clone()).collect() };
        // top row: Internet Gateways and whatever else lives in the VPC itself (endpoints, balancers, scaling groups)
        let tops = kinds(d, &|k| !matches!(k, "subnet" | "rt" | "sg"));
        let (places, used_w, used_h) = flow(&tops, &size, PAD, y, wrap_w, GAP);
        for (id, px, py) in places {
            let n = d.node_mut(&id).unwrap();
            n.x = px;
            n.y = py;
        }
        if !tops.is_empty() {
            widest = widest.max(used_w);
            y += used_h + GAP;
        }
        // the subnet grid
        let mut row_y = vec![];
        let mut ry = y;
        for h in &row_h {
            row_y.push(ry);
            ry += h + GAP;
        }
        for (id, r, c, k) in &placed {
            let n = d.node_mut(id).unwrap();
            n.x = slot_x[&(*c, *k)];
            n.y = row_y[*r];
        }
        if !subs.is_empty() {
            y += grid_h + GAP;
        }
        // route tables, then security groups, each wrapped into rows
        for kind in ["rt", "sg"] {
            let ids = kinds(d, &|k| k == kind);
            if ids.is_empty() {
                continue;
            }
            let (places, used_w, used_h) = flow(&ids, &size, PAD, y, wrap_w, GAP);
            for (id, px, py) in places {
                let n = d.node_mut(&id).unwrap();
                n.x = px;
                n.y = py;
            }
            widest = widest.max(used_w);
            y += used_h + GAP;
        }
        let v = d.node_mut(vid).unwrap();
        v.w = (widest + 2.0 * PAD).max(420.0);
        v.h = (y - GAP + PAD).max(160.0);
    }

    // ---- 3. the region: address ranges (and buckets) in a column on the left, the VPCs stacked beside them
    let loose: Vec<String> = d.nodes.iter().filter(|n| n.parent.is_none() && n.kind != "vpc").map(|n| n.id.clone()).collect();
    let mut loose_w: f64 = 0.0;
    let mut y = REGION_GAP;
    for id in &loose {
        let (w, h) = if is_container(&d.node(id).map(|n| n.kind.clone()).unwrap_or_default()) { let n = d.node(id).unwrap(); (n.w, n.h) } else { size(id) };
        let n = d.node_mut(id).unwrap();
        n.x = REGION_GAP;
        n.y = y;
        y += h + 16.0;
        loose_w = loose_w.max(w);
    }
    let vx = if loose.is_empty() { REGION_GAP } else { REGION_GAP + loose_w + REGION_GAP };
    let mut vy = REGION_GAP;
    for vid in &vpcs {
        let v = d.node_mut(vid).unwrap();
        v.x = vx;
        v.y = vy;
        vy += v.h + REGION_GAP;
    }
    d.sort_parents_first();
}

/// true when no two blocks with the same container overlap (used by the tests and the checks)
pub fn no_overlap(d: &Design, sizes: &Sizes) -> Result<(), String> {
    let rect = |n: &crate::model::Node| -> (f64, f64, f64, f64) {
        let (w, h) = if is_container(&n.kind) { (n.w, n.h) } else { sizes.get(&n.id).copied().unwrap_or((BLOCK_W, BLOCK_H)) };
        (n.x, n.y, n.x + w, n.y + h)
    };
    for (i, a) in d.nodes.iter().enumerate() {
        for b in d.nodes.iter().skip(i + 1) {
            if a.parent != b.parent {
                continue;
            }
            let (ax0, ay0, ax1, ay1) = rect(a);
            let (bx0, by0, bx1, by1) = rect(b);
            if ax0 < bx1 && bx0 < ax1 && ay0 < by1 && by0 < ay1 {
                return Err(format!("{} overlaps {}", a.label, b.label));
            }
        }
        // a block fits inside its container
        if let Some(p) = a.parent.as_ref().and_then(|p| d.node(p)) {
            let (_, _, ax1, ay1) = rect(a);
            if a.x < 0.0 || a.y < 0.0 || ax1 > p.w + 0.5 || ay1 > p.h + 0.5 {
                return Err(format!("{} sticks out of {} ({ax1} x {ay1} in {} x {})", a.label, p.label, p.w, p.h));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;

    /// the owner's imported VPC in miniature: three public subnets with instances, a long row of groups with warnings, five ranges
    const MESSY: &str = "net messy\nregion ap-south-1\nazs 3\n\nvpc main 172.31.0.0/16\nigw gw in main\nsubnet pa public 172.31.0.0/20 az a in main\nsubnet pb public 172.31.16.0/20 az b in main\nsubnet pc public 172.31.32.0/20 az c in main\n\
ec2 one t3.medium in pa sg launch_wizard_1 key k os ubuntu public\nec2 two t3.micro in pb sg launch_wizard_2,dhimahi_me key k\nec2 three t3.micro in pb sg gee_runner key k os ubuntu\nec2 four t3.micro in pc sg launch_wizard_7 key k\n\
rt pub in main: 0.0.0.0/0 -> gw\nsg launch_wizard_1 in main\nsg launch_wizard_2 in main\nsg launch_wizard_3 in main\nsg launch_wizard_4 in main\nsg launch_wizard_7 in main\nsg launch_wizard_8 in main\nsg launch_wizard_10 in main\n\
sg dhimahi_me in main\nsg gee_runner in main\nsg openvpn_access_server_2_8_5_autogenbyawsmp_ in main\nsg dl_notebook_vscode_security_group in main\nsg default in main\n\
cidr internet 0.0.0.0/0\ncidr net_a 117.96.150.138/32\ncidr net_b 223.191.66.7/32\ncidr net_c 106.192.105.207/32\ncidr net_d 223.238.153.117/32\n\n\
rule internet -> launch_wizard_1 tcp 3389\nrule internet -> launch_wizard_2 tcp 22\nrule internet -> launch_wizard_2 tcp 80\nrule internet -> launch_wizard_3 tcp 22\nrule internet -> launch_wizard_7 tcp 22\n\
rule internet -> launch_wizard_7 tcp 8000-8010\nrule internet -> launch_wizard_10 tcp 22\nrule internet -> launch_wizard_4 tcp 22\nrule internet -> dhimahi_me tcp 80\nrule internet -> dhimahi_me tcp 22\n\
rule net_d -> gee_runner tcp 22\nrule internet -> openvpn_access_server_2_8_5_autogenbyawsmp_ udp 1194\nrule internet -> dl_notebook_vscode_security_group tcp 22\n";

    #[test]
    fn nothing_overlaps_and_containers_fit() {
        let mut d = parse(MESSY, None).unwrap();
        arrange(&mut d);
        let sizes = estimate(&d);
        no_overlap(&d, &sizes).unwrap();
        // the groups wrap into rows instead of one endless line
        let sgs: Vec<_> = d.nodes.iter().filter(|n| n.kind == "sg").collect();
        let rows: std::collections::BTreeSet<i64> = sgs.iter().map(|n| n.y as i64).collect();
        assert!(rows.len() >= 2, "{} groups in {} row(s)", sgs.len(), rows.len());
        let vpc = d.by_label("main").unwrap();
        assert!(vpc.w < 1500.0, "the VPC stays about a screen wide: {}", vpc.w);
        // the subnet with two instances grew around them
        let pb = d.by_label("pb").unwrap();
        assert!(pb.w > SUB_W && pb.h >= SUB_H, "{} x {}", pb.w, pb.h);
        // the address ranges stand left of the VPC, one under the other
        let ranges: Vec<_> = d.nodes.iter().filter(|n| n.kind == "cidr").collect();
        assert!(ranges.iter().all(|r| r.x < vpc.x));
        assert!(ranges.windows(2).all(|p| p[1].y > p[0].y));
    }

    #[test]
    fn measured_sizes_win() {
        let mut d = parse(MESSY, None).unwrap();
        let big = d.by_label("launch_wizard_1").unwrap().id.clone();
        let mut m = Sizes::new();
        m.insert(big.clone(), (230.0, 400.0));
        arrange_with(&mut d, &m);
        let mut sizes = estimate(&d);
        sizes.insert(big, (230.0, 400.0));
        no_overlap(&d, &sizes).unwrap();
    }

    #[test]
    fn a_block_in_the_region_gets_a_place() {
        let mut d = Design::default();
        let v = d.add("vpc", None).unwrap();
        d.add("s3", None);
        d.add("cidr", None);
        let s = d.add("subnet", Some(&v)).unwrap();
        d.add("ec2", Some(&s));
        d.add("ec2", Some(&s));
        d.add("ec2", Some(&s));
        arrange(&mut d);
        no_overlap(&d, &estimate(&d)).unwrap();
        let sub = d.node(&s).unwrap();
        assert!(sub.w >= 2.0 * BLOCK_W, "three instances: two columns, {}", sub.w);
    }
}
