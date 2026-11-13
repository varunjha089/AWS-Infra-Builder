//! The design: blocks (nodes) that live inside one another (region ⊃ VPC ⊃ subnet ⊃ NAT / instance) and arrows (edges) between
//! them. Kinds: vpc, subnet, igw (Internet Gateway), nat (NAT Gateway), rt (route table), sg (security group), ec2 (instance), cidr
//! (an address range outside the design: the internet, an office). Arrow kinds: `attach` (subnet → route table: the subnet uses that
//! table), `route` (route table → igw / nat, params.dest) and `traffic` (sg / cidr → sg / cidr, params.proto + params.ports: the
//! target allows that traffic in from the source, and the source lets it out).

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub type Params = Map<String, Value>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: String,
    pub kind: String,
    /// the block's name in the text (unique; letters, digits, underscore)
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub params: Params,
    /// the block this one lives in (None for a VPC: it lives in the region)
    #[serde(default)]
    pub parent: Option<String>,
    /// position relative to the parent's top-left corner
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    /// size: containers (vpc, subnet) are drawn as boxes of this size; other blocks size themselves
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    /// "attach" or "route" (decided by the end points, see `edge_kind`)
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub params: Params,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Design {
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_region")]
    pub region: String,
    /// how many Availability Zones the design uses (a, b, c …)
    #[serde(default = "default_azs")]
    pub azs: u8,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// the design as .aib text, kept in step by dsl::sync
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

fn default_region() -> String {
    "ap-south-1".into()
}
fn default_azs() -> u8 {
    2
}

impl Default for Design {
    fn default() -> Self {
        Design { name: String::new(), region: default_region(), azs: default_azs(), nodes: vec![], edges: vec![], text: None }
    }
}

/// (kind, title, the kind it must live in: "" = the region)
pub const KINDS: &[(&str, &str, &str)] = &[
    ("vpc", "VPC", ""),
    ("subnet", "Subnet", "vpc"),
    ("igw", "Internet Gateway", "vpc"),
    ("nat", "NAT Gateway", "subnet"),
    ("rt", "Route table", "vpc"),
    ("sg", "Security group", "vpc"),
    ("ec2", "EC2 instance", "subnet"),
    ("cidr", "Address range", ""),
];

/// the common ports by name, for the arrow label and the checks
pub const PORTS: &[(&str, u16)] = &[("ssh", 22), ("rdp", 3389), ("http", 80), ("https", 443), ("mysql", 3306), ("postgres", 5432), ("mssql", 1433), ("mongodb", 27017), ("redis", 6379)];

/// (type, vCPU, memory GiB, arch, USD/h in us-east-1, USD/h in ap-south-1); other regions: us-east-1 × 1.1 (said in the assumptions)
pub const INSTANCE_TYPES: &[(&str, u8, f64, &str, f64, f64)] = &[
    ("t4g.nano", 2, 0.5, "arm64", 0.0042, 0.0042),
    ("t4g.micro", 2, 1.0, "arm64", 0.0084, 0.0084),
    ("t4g.small", 2, 2.0, "arm64", 0.0168, 0.0168),
    ("t4g.medium", 2, 4.0, "arm64", 0.0336, 0.0336),
    ("t4g.large", 2, 8.0, "arm64", 0.0672, 0.0672),
    ("t3.micro", 2, 1.0, "x86_64", 0.0104, 0.0112),
    ("t3.small", 2, 2.0, "x86_64", 0.0208, 0.0224),
    ("t3.medium", 2, 4.0, "x86_64", 0.0416, 0.0448),
    ("t3.large", 2, 8.0, "x86_64", 0.0832, 0.0896),
    ("m6g.large", 2, 8.0, "arm64", 0.077, 0.0816),
    ("m5.large", 2, 8.0, "x86_64", 0.096, 0.101),
    ("c6g.large", 2, 4.0, "arm64", 0.068, 0.0712),
    ("c5.large", 2, 4.0, "x86_64", 0.085, 0.089),
    ("r6g.large", 2, 16.0, "arm64", 0.1008, 0.1056),
    ("g4dn.xlarge", 4, 16.0, "x86_64", 0.526, 0.526),
];

pub fn instance_type(t: &str) -> Option<&'static (&'static str, u8, f64, &'static str, f64, f64)> {
    INSTANCE_TYPES.iter().find(|i| i.0 == t)
}

/// (os key, title, the AMI search: owner and name pattern)
pub const OS: &[(&str, &str, &str, &str)] = &[
    ("al2023", "Amazon Linux 2023", "amazon", "al2023-ami-2023.*-{arch}"),
    ("ubuntu", "Ubuntu 24.04 LTS", "099720109477", "ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-{ubuntu_arch}-server-*"),
    ("debian", "Debian 12", "136693071363", "debian-12-{arch}-*"),
];

pub const TIERS: &[&str] = &["public", "private", "isolated"];

/// regions and how many Availability Zones each has (the usable ones of a new account; a quota increase can add more)
pub const REGIONS: &[(&str, &str, u8)] = &[
    ("ap-south-1", "Asia Pacific (Mumbai)", 3),
    ("ap-south-2", "Asia Pacific (Hyderabad)", 3),
    ("ap-southeast-1", "Asia Pacific (Singapore)", 3),
    ("ap-southeast-2", "Asia Pacific (Sydney)", 3),
    ("ap-northeast-1", "Asia Pacific (Tokyo)", 3),
    ("ap-northeast-2", "Asia Pacific (Seoul)", 4),
    ("us-east-1", "US East (N. Virginia)", 6),
    ("us-east-2", "US East (Ohio)", 3),
    ("us-west-1", "US West (N. California)", 2),
    ("us-west-2", "US West (Oregon)", 4),
    ("ca-central-1", "Canada (Central)", 3),
    ("eu-west-1", "Europe (Ireland)", 3),
    ("eu-west-2", "Europe (London)", 3),
    ("eu-west-3", "Europe (Paris)", 3),
    ("eu-central-1", "Europe (Frankfurt)", 3),
    ("eu-north-1", "Europe (Stockholm)", 3),
    ("sa-east-1", "South America (São Paulo)", 3),
    ("me-south-1", "Middle East (Bahrain)", 3),
    ("af-south-1", "Africa (Cape Town)", 3),
];

pub fn region_azs(region: &str) -> Option<u8> {
    REGIONS.iter().find(|r| r.0 == region).map(|r| r.2)
}

pub fn title(kind: &str) -> &'static str {
    KINDS.iter().find(|k| k.0 == kind).map(|k| k.1).unwrap_or("Block")
}

/// the kind a block of this kind lives in ("" = the region; None = unknown kind)
pub fn parent_kind(kind: &str) -> Option<&'static str> {
    KINDS.iter().find(|k| k.0 == kind).map(|k| k.2)
}

pub fn is_container(kind: &str) -> bool {
    matches!(kind, "vpc" | "subnet")
}

/// blocks that stand in the region beside the VPCs
pub fn is_region_level(kind: &str) -> bool {
    parent_kind(kind) == Some("")
}

/// the parameters a new block of this kind starts with
pub fn defaults(kind: &str) -> Params {
    let v = match kind {
        "vpc" => json!({"cidr": "10.0.0.0/16"}),
        "subnet" => json!({"cidr": "", "az": "a", "tier": "public"}),
        // egress "all": AWS's default (everything out); "rules": only what the arrows leaving the group say
        "sg" => json!({"egress": "all", "description": ""}),
        // the cheapest type that runs a small service; no public address unless asked (an IPv4 address costs money)
        // spot: about 70 % cheaper, may be interrupted (for work that can be stopped and resumed); disk: the root volume in GiB
        "ec2" => json!({"type": "t4g.micro", "os": "al2023", "key": "", "public_ip": false, "sgs": [], "spot": false, "disk": 8}),
        "cidr" => json!({"cidr": "0.0.0.0/0"}),
        _ => json!({}),
    };
    v.as_object().cloned().unwrap_or_default()
}

/// the AZ letters of the design: a, b, c …
pub fn az_letters(azs: u8) -> Vec<String> {
    (0..azs.clamp(1, 6)).map(|i| ((b'a' + i) as char).to_string()).collect()
}

/// the arrow kind the two blocks allow: subnet → rt = attach, rt → igw / nat = route, otherwise none
pub fn edge_kind(source: &str, target: &str) -> Option<&'static str> {
    match (source, target) {
        ("subnet", "rt") => Some("attach"),
        ("rt", "igw") | ("rt", "nat") => Some("route"),
        ("sg", "sg") | ("sg", "cidr") | ("cidr", "sg") => Some("traffic"),
        _ => None,
    }
}

/// "tcp 443", "tcp 8080-8090", "udp 53", "icmp", "all"; port names (ssh, https, postgres …) are accepted
pub fn parse_ports(proto: &str, ports: &str) -> Result<(u16, u16), String> {
    let proto = proto.trim().to_lowercase();
    let ports = ports.trim().to_lowercase();
    if proto == "all" || proto == "-1" {
        return Ok((0, 65535));
    }
    if proto == "icmp" {
        return Ok((0, 0));
    }
    if proto != "tcp" && proto != "udp" {
        return Err(format!("protocol \"{proto}\": write tcp, udp, icmp or all"));
    }
    if ports.is_empty() || ports == "all" {
        return Ok((0, 65535));
    }
    if let Some(p) = PORTS.iter().find(|p| p.0 == ports) {
        return Ok((p.1, p.1));
    }
    let (a, b) = match ports.split_once('-') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (ports.as_str(), ports.as_str()),
    };
    match (a.parse::<u16>(), b.parse::<u16>()) {
        (Ok(a), Ok(b)) if a <= b => Ok((a, b)),
        _ => Err(format!("ports \"{ports}\": write a port (443), a range (8080-8090), a name (ssh, https, postgres) or all")),
    }
}

/// the arrow's label: "tcp 5432 (postgres)", "all traffic"
pub fn traffic_label(proto: &str, ports: &str) -> String {
    let proto = proto.trim().to_lowercase();
    if proto == "all" || proto == "-1" {
        return "all traffic".into();
    }
    if proto == "icmp" {
        return "icmp".into();
    }
    match parse_ports(&proto, ports) {
        Ok((a, b)) if a == 0 && b == 65535 => format!("{proto} all ports"),
        Ok((a, b)) if a == b => match PORTS.iter().find(|p| p.1 == a) {
            Some(p) => format!("{proto} {a} ({})", p.0),
            None => format!("{proto} {a}"),
        },
        Ok((a, b)) => format!("{proto} {a}-{b}"),
        Err(_) => format!("{proto} {}?", ports.trim()),
    }
}

/// the default size of a container of this kind
pub fn default_size(kind: &str) -> (f64, f64) {
    match kind {
        "vpc" => (640.0, 420.0),
        "subnet" => (SUB_W, SUB_H),
        _ => (0.0, 0.0),
    }
}

impl Design {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    pub fn by_label(&self, label: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.label == label)
    }
    pub fn children(&self, id: &str) -> Vec<&Node> {
        self.nodes.iter().filter(|n| n.parent.as_deref() == Some(id)).collect()
    }
    /// the block and everything inside it, inside that, …
    pub fn descendants(&self, id: &str) -> Vec<String> {
        let mut out = vec![id.to_string()];
        let mut i = 0;
        while i < out.len() {
            let cur = out[i].clone();
            out.extend(self.children(&cur).into_iter().map(|c| c.id.clone()));
            i += 1;
        }
        out
    }
    /// the nearest enclosing block of this kind
    pub fn ancestor(&self, id: &str, kind: &str) -> Option<&Node> {
        let mut cur = self.node(id)?.parent.clone();
        while let Some(p) = cur {
            let n = self.node(&p)?;
            if n.kind == kind {
                return Some(n);
            }
            cur = n.parent.clone();
        }
        None
    }
    pub fn param_str(&self, id: &str, key: &str) -> String {
        self.node(id).and_then(|n| n.params.get(key)).and_then(|v| v.as_str()).unwrap_or("").to_string()
    }

    /// ids are never reused (the window keys its drawing by them): one more than the highest so far
    pub fn next_id(&self) -> String {
        let max = self.nodes.iter().map(|n| n.id.as_str()).chain(self.edges.iter().map(|e| e.id.as_str())).filter_map(|id| id.get(1..)?.parse::<u64>().ok()).max().unwrap_or(0);
        format!("n{}", max + 1)
    }
    pub fn next_edge_id(&self) -> String {
        format!("e{}", &self.next_id()[1..])
    }

    /// every block gets a unique label (kind + number for those without)
    pub fn ensure_labels(&mut self) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        let ids: Vec<String> = self.nodes.iter().map(|n| n.id.clone()).collect();
        for id in ids {
            let n = self.node(&id).unwrap();
            let mut label = clean_label(&n.label);
            if label.is_empty() {
                label = n.kind.clone();
            }
            let base = label.clone();
            let mut k = 0;
            while seen.contains_key(&label) {
                k += 1;
                label = format!("{base}{k}");
            }
            // a generated plain "vpc" becomes "vpc1" so that the numbering is visible from the start
            if n.label.is_empty() && k == 0 {
                let mut k2 = 1;
                while seen.contains_key(&format!("{base}{k2}")) {
                    k2 += 1;
                }
                label = format!("{base}{k2}");
            }
            seen.insert(label.clone(), 1);
            self.node_mut(&id).unwrap().label = label;
        }
    }

    /// a new block of `kind` inside `parent` (None = the region); subnets get the next free CIDR and a balanced AZ
    pub fn add(&mut self, kind: &str, parent: Option<&str>) -> Option<String> {
        let id = self.next_id();
        let mut params = defaults(kind);
        if kind == "subnet" {
            if let Some(vpc) = parent.and_then(|p| self.node(p)) {
                if let Ok(vc) = crate::cidr::Cidr::parse(vpc.params.get("cidr").and_then(|v| v.as_str()).unwrap_or("")) {
                    let taken: Vec<_> = self.children(&vpc.id).iter().filter(|c| c.kind == "subnet").filter_map(|c| crate::cidr::Cidr::parse(c.params.get("cidr")?.as_str()?).ok()).collect();
                    let prefix = (vc.prefix + 4).min(28);
                    if let Some(c) = crate::cidr::next_free(&vc, &taken, prefix) {
                        params.insert("cidr".into(), json!(c.to_string()));
                    }
                }
                // the AZ with the fewest subnets so far
                let letters = az_letters(self.azs);
                let count = |l: &str| self.children(&vpc.id).iter().filter(|c| c.kind == "subnet" && c.params.get("az").and_then(|v| v.as_str()) == Some(l)).count();
                if let Some(l) = letters.iter().min_by_key(|l| count(l)) {
                    params.insert("az".into(), json!(l));
                }
            }
        }
        let (w, h) = default_size(kind);
        let siblings = parent.map(|p| self.children(p).len()).unwrap_or_else(|| self.nodes.iter().filter(|n| n.parent.is_none()).count()) as f64;
        let (x, y) = if is_container(kind) { (24.0 + 30.0 * siblings, 80.0 + 30.0 * siblings) } else { (24.0 + 40.0 * siblings, 44.0 + 36.0 * siblings) };
        self.nodes.push(Node { id: id.clone(), kind: kind.into(), label: String::new(), params, parent: parent.map(String::from), x, y, w, h });
        self.ensure_labels();
        Some(id)
    }

    /// remove blocks (with everything inside them) and the arrows that touched them
    pub fn remove(&mut self, ids: &[String]) {
        let mut gone: Vec<String> = vec![];
        for id in ids {
            gone.extend(self.descendants(id));
        }
        self.nodes.retain(|n| !gone.contains(&n.id));
        self.edges.retain(|e| !gone.contains(&e.source) && !gone.contains(&e.target));
    }

    /// join two blocks; refused (None) when the kinds do not allow an arrow
    pub fn connect(&mut self, source: &str, target: &str) -> Option<String> {
        let kind = edge_kind(&self.node(source)?.kind, &self.node(target)?.kind)?;
        if kind == "attach" {
            // a subnet uses one route table
            self.edges.retain(|e| !(e.kind == "attach" && e.source == source));
        }
        if self.edges.iter().any(|e| e.source == source && e.target == target) {
            return None;
        }
        let id = self.next_edge_id();
        let params = match kind {
            "route" => json!({"dest": "0.0.0.0/0"}).as_object().cloned().unwrap_or_default(),
            "traffic" => json!({"proto": "tcp", "ports": "443"}).as_object().cloned().unwrap_or_default(),
            _ => Params::new(),
        };
        self.edges.push(Edge { id: id.clone(), source: source.into(), target: target.into(), kind: kind.into(), params });
        Some(id)
    }

    /// the absolute position of a block (its own plus every enclosing block's)
    pub fn abs_pos(&self, id: &str) -> (f64, f64) {
        let mut x = 0.0;
        let mut y = 0.0;
        let mut cur = Some(id.to_string());
        let mut guard = 0;
        while let Some(c) = cur {
            let Some(n) = self.node(&c) else { break };
            x += n.x;
            y += n.y;
            cur = n.parent.clone();
            guard += 1;
            if guard > 100 {
                break;
            }
        }
        (x, y)
    }

    /// containers before what lives in them (React Flow wants parents first)
    pub fn sort_parents_first(&mut self) {
        let mut out: Vec<Node> = vec![];
        let mut rest: Vec<Node> = std::mem::take(&mut self.nodes);
        let mut guard = 0;
        while !rest.is_empty() && guard < 1000 {
            guard += 1;
            let (ready, later): (Vec<Node>, Vec<Node>) = rest.into_iter().partition(|n| n.parent.as_ref().map(|p| out.iter().any(|o| &o.id == p)).unwrap_or(true));
            if ready.is_empty() {
                // orphans (parent missing): put them in the region
                out.extend(later.into_iter().map(|mut n| { n.parent = None; n }));
                rest = vec![];
            } else {
                out.extend(ready);
                rest = later;
            }
        }
        self.nodes = out;
    }
}

/// a label as the text language accepts it: letters, digits and underscore, not starting with a digit
pub fn clean_label(s: &str) -> String {
    let mut out: String = s.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        out.insert(0, '_');
    }
    out
}

// ---------------------------------------------------------------- layout
pub const SUB_W: f64 = 250.0;
pub const SUB_H: f64 = 150.0;
const GAP: f64 = 22.0;
const PAD: f64 = 24.0;
const VPC_HEAD: f64 = 108.0;

/// the AZ × tier grid: in each VPC, Availability Zones are columns and tiers (public, private, isolated) are rows; the Internet
/// Gateway sits on top, the route tables in a row under the subnets; NAT Gateways sit inside their subnet. VPCs are stacked.
pub fn arrange(d: &mut Design) {
    let letters = az_letters(d.azs);
    let vpcs: Vec<String> = d.nodes.iter().filter(|n| n.kind == "vpc").map(|n| n.id.clone()).collect();
    let mut vy = 40.0;
    for vid in &vpcs {
        let subs: Vec<Node> = d.children(vid).into_iter().filter(|n| n.kind == "subnet").cloned().collect();
        // rows: the tiers that occur, in the fixed order; columns: the AZ letters that occur (design AZs first)
        let mut tiers: Vec<String> = TIERS.iter().map(|t| t.to_string()).filter(|t| subs.iter().any(|s| s.params.get("tier").and_then(|v| v.as_str()) == Some(t))).collect();
        for s in &subs {
            let t = s.params.get("tier").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !tiers.contains(&t) {
                tiers.push(t);
            }
        }
        let mut cols: Vec<String> = letters.clone();
        for s in &subs {
            let a = s.params.get("az").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !cols.contains(&a) {
                cols.push(a);
            }
        }
        // several subnets in the same cell go side by side in sub-columns
        let mut used: HashMap<(usize, usize), usize> = HashMap::new();
        let mut cell_width: Vec<usize> = vec![1; cols.len()];
        for s in &subs {
            let t = s.params.get("tier").and_then(|v| v.as_str()).unwrap_or("");
            let a = s.params.get("az").and_then(|v| v.as_str()).unwrap_or("");
            let r = tiers.iter().position(|x| x == t).unwrap_or(0);
            let c = cols.iter().position(|x| x == a).unwrap_or(0);
            let k = used.entry((r, c)).or_insert(0);
            *k += 1;
            cell_width[c] = cell_width[c].max(*k);
        }
        let col_x: Vec<f64> = {
            let mut xs = vec![];
            let mut x = PAD;
            for w in &cell_width {
                xs.push(x);
                x += *w as f64 * (SUB_W + GAP);
            }
            xs
        };
        let mut placed: HashMap<(usize, usize), usize> = HashMap::new();
        for s in &subs {
            let t = s.params.get("tier").and_then(|v| v.as_str()).unwrap_or("");
            let a = s.params.get("az").and_then(|v| v.as_str()).unwrap_or("");
            let r = tiers.iter().position(|x| x == t).unwrap_or(0);
            let c = cols.iter().position(|x| x == a).unwrap_or(0);
            let k = placed.entry((r, c)).or_insert(0);
            let n = d.node_mut(&s.id).unwrap();
            n.x = col_x[c] + *k as f64 * (SUB_W + GAP);
            n.y = VPC_HEAD + r as f64 * (SUB_H + GAP);
            n.w = SUB_W;
            n.h = SUB_H;
            *k += 1;
            // what lives in the subnet: a row of small blocks
            let kids: Vec<String> = d.children(&s.id).iter().map(|c| c.id.clone()).collect();
            for (i, kid) in kids.iter().enumerate() {
                let k = d.node_mut(kid).unwrap();
                k.x = 16.0 + i as f64 * 120.0;
                k.y = 62.0;
            }
        }
        let grid_w = col_x.last().copied().unwrap_or(PAD) + cell_width.last().copied().unwrap_or(1) as f64 * (SUB_W + GAP) - GAP;
        let rows = tiers.len().max(1) as f64;
        let rt_y = VPC_HEAD + rows * (SUB_H + GAP) + 8.0;
        // top row: Internet Gateways; bottom row: route tables
        let tops: Vec<String> = d.children(vid).iter().filter(|n| n.kind == "igw").map(|n| n.id.clone()).collect();
        for (i, id) in tops.iter().enumerate() {
            let n = d.node_mut(id).unwrap();
            n.x = PAD + i as f64 * 190.0;
            n.y = 44.0;
        }
        let rts: Vec<String> = d.children(vid).iter().filter(|n| n.kind == "rt").map(|n| n.id.clone()).collect();
        for (i, id) in rts.iter().enumerate() {
            let n = d.node_mut(id).unwrap();
            n.x = PAD + i as f64 * 180.0;
            n.y = rt_y;
        }
        let sgs: Vec<String> = d.children(vid).iter().filter(|n| n.kind == "sg").map(|n| n.id.clone()).collect();
        let sg_y = rt_y + if rts.is_empty() { 0.0 } else { 74.0 };
        for (i, id) in sgs.iter().enumerate() {
            let n = d.node_mut(id).unwrap();
            n.x = PAD + i as f64 * 180.0;
            n.y = sg_y;
        }
        let others: Vec<String> = d.children(vid).iter().filter(|n| !matches!(n.kind.as_str(), "igw" | "rt" | "subnet" | "sg")).map(|n| n.id.clone()).collect();
        for (i, id) in others.iter().enumerate() {
            let n = d.node_mut(id).unwrap();
            n.x = PAD + tops.len() as f64 * 190.0 + i as f64 * 190.0;
            n.y = 44.0;
        }
        let w = (grid_w + PAD).max(PAD + (tops.len() + others.len()).max(rts.len()).max(sgs.len()) as f64 * 190.0).max(420.0);
        let h = sg_y + 60.0 + PAD;
        let v = d.node_mut(vid).unwrap();
        v.x = 40.0;
        v.y = vy;
        v.w = w;
        v.h = h;
        vy += h + 40.0;
    }
    // blocks in the region that are not VPCs (address ranges such as the internet): a column to the left of the VPCs
    let loose: Vec<String> = d.nodes.iter().filter(|n| n.parent.is_none() && n.kind != "vpc").map(|n| n.id.clone()).collect();
    if !loose.is_empty() {
        for id in d.nodes.iter().filter(|n| n.kind == "vpc").map(|n| n.id.clone()).collect::<Vec<_>>() {
            d.node_mut(&id).unwrap().x = 260.0;
        }
        for (i, id) in loose.iter().enumerate() {
            let n = d.node_mut(id).unwrap();
            n.x = 40.0;
            n.y = 40.0 + i as f64 * 80.0;
        }
    }
    d.sort_parents_first();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_connect_remove() {
        let mut d = Design::default();
        let v = d.add("vpc", None).unwrap();
        let s1 = d.add("subnet", Some(&v)).unwrap();
        let s2 = d.add("subnet", Some(&v)).unwrap();
        assert_eq!(d.param_str(&s1, "cidr"), "10.0.0.0/20");
        assert_eq!(d.param_str(&s2, "cidr"), "10.0.16.0/20");
        assert_eq!(d.param_str(&s1, "az"), "a");
        assert_eq!(d.param_str(&s2, "az"), "b");
        let rt = d.add("rt", Some(&v)).unwrap();
        let igw = d.add("igw", Some(&v)).unwrap();
        let nat = d.add("nat", Some(&s1)).unwrap();
        assert!(d.connect(&s1, &rt).is_some());
        assert!(d.connect(&rt, &igw).is_some());
        assert!(d.connect(&igw, &rt).is_none(), "no arrow the wrong way");
        assert!(d.connect(&s1, &igw).is_none(), "subnets route through a table");
        assert_eq!(d.edges.len(), 2);
        assert_eq!(d.edges[1].params["dest"], "0.0.0.0/0");
        // a second attach replaces the first
        let rt2 = d.add("rt", Some(&v)).unwrap();
        d.connect(&s1, &rt2);
        assert_eq!(d.edges.iter().filter(|e| e.kind == "attach").count(), 1);
        assert_eq!(d.ancestor(&nat, "vpc").unwrap().id, v);
        let labels: Vec<_> = d.nodes.iter().map(|n| n.label.clone()).collect();
        assert_eq!(labels, ["vpc1", "subnet1", "subnet2", "rt1", "igw1", "nat1", "rt2"]);
        d.remove(&[s1.clone()]);
        assert!(d.node(&nat).is_none(), "the NAT inside the subnet goes too");
        assert!(d.edges.iter().all(|e| e.source != s1));
        d.remove(&[v]);
        assert!(d.nodes.is_empty() && d.edges.is_empty());
    }

    #[test]
    fn arrange_grid() {
        let mut d = Design::default();
        let v = d.add("vpc", None).unwrap();
        for (az, tier) in [("a", "public"), ("b", "public"), ("a", "private"), ("b", "private")] {
            let s = d.add("subnet", Some(&v)).unwrap();
            let n = d.node_mut(&s).unwrap();
            n.params.insert("az".into(), json!(az));
            n.params.insert("tier".into(), json!(tier));
        }
        d.add("igw", Some(&v));
        d.add("rt", Some(&v));
        arrange(&mut d);
        let at = |label: &str| { let n = d.by_label(label).unwrap(); (n.x, n.y) };
        assert_eq!(at("subnet1").1, at("subnet2").1, "same tier, same row");
        assert!(at("subnet2").0 > at("subnet1").0, "AZ b is right of AZ a");
        assert_eq!(at("subnet1").0, at("subnet3").0, "same AZ, same column");
        assert!(at("subnet3").1 > at("subnet1").1, "private below public");
        assert!(at("rt1").1 > at("subnet3").1 + SUB_H);
        assert!(at("igw1").1 < at("subnet1").1);
        let vpc = d.by_label("vpc1").unwrap();
        assert!(vpc.w >= at("subnet2").0 + SUB_W && vpc.h >= at("rt1").1 + 40.0);
        assert_eq!(d.nodes[0].kind, "vpc");
    }

    #[test]
    fn ports_and_labels_of_traffic() {
        assert_eq!(parse_ports("tcp", "443"), Ok((443, 443)));
        assert_eq!(parse_ports("tcp", "postgres"), Ok((5432, 5432)));
        assert_eq!(parse_ports("TCP", "8080-8090"), Ok((8080, 8090)));
        assert_eq!(parse_ports("udp", "all"), Ok((0, 65535)));
        assert_eq!(parse_ports("all", ""), Ok((0, 65535)));
        assert_eq!(parse_ports("icmp", ""), Ok((0, 0)));
        assert!(parse_ports("tcp", "99999").is_err());
        assert!(parse_ports("tcp", "90-80").is_err());
        assert!(parse_ports("gre", "1").is_err());
        assert_eq!(traffic_label("tcp", "5432"), "tcp 5432 (postgres)");
        assert_eq!(traffic_label("tcp", "8080"), "tcp 8080");
        assert_eq!(traffic_label("all", ""), "all traffic");
        assert_eq!(traffic_label("tcp", "x"), "tcp x?");
        let mut d = Design::default();
        let v = d.add("vpc", None).unwrap();
        let a = d.add("sg", Some(&v)).unwrap();
        let b = d.add("sg", Some(&v)).unwrap();
        let net = d.add("cidr", None).unwrap();
        assert!(d.connect(&net, &a).is_some());
        assert!(d.connect(&a, &b).is_some());
        assert!(d.connect(&b, &v).is_none());
        assert_eq!(d.edges[0].kind, "traffic");
        assert_eq!(d.edges[0].params["ports"], "443");
        assert_eq!(d.param_str(&net, "cidr"), "0.0.0.0/0");
        arrange(&mut d);
        assert!(d.node(&net).unwrap().x < d.node(&v).unwrap().x, "address ranges stand left of the VPCs");
    }

    #[test]
    fn labels_are_cleaned_and_unique() {
        let mut d = Design::default();
        d.add("vpc", None);
        d.add("vpc", None);
        d.nodes[1].label = "my vpc!".into();
        d.nodes.push(Node { id: "n9".into(), kind: "igw".into(), label: "vpc1".into(), params: Params::new(), parent: Some("n1".into()), x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        d.ensure_labels();
        let labels: Vec<_> = d.nodes.iter().map(|n| n.label.clone()).collect();
        assert_eq!(labels, ["vpc1", "my_vpc_", "vpc11"]);
        assert_eq!(clean_label("1abc"), "_1abc");
    }
}
