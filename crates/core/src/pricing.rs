//! Cost: what the design costs per month before any traffic, per block and in total, and how to pay less. Prices are a bundled
//! table of on-demand list prices (USD per hour; 730 hours a month); every number is an estimate and says so. The principle: the
//! cheapest design that does the job is the default, and anything that costs money while idle is pointed out with the alternative.

use crate::model::{instance_type, Design, INSTANCE_TYPES};
use serde::{Deserialize, Serialize};

pub const HOURS: f64 = 730.0;

/// how many hours a month the block runs: its `hours` (1 to 744), else all month
pub fn hours_of(n: &crate::model::Node) -> f64 {
    n.params.get("hours").and_then(|v| v.as_f64()).filter(|h| (1.0..=744.0).contains(h)).unwrap_or(HOURS)
}

/// an instance type that fits what was asked (at least the vCPUs and memory, GPU or not), cheapest first
#[derive(Debug, Clone, serde::Serialize)]
pub struct Candidate {
    pub r#type: String,
    pub vcpu: u8,
    pub memory: f64,
    pub arch: String,
    pub gpu: bool,
    pub hourly: f64,
    /// at the asked hours a month
    pub monthly: f64,
    pub note: String,
    /// false = smaller than asked (nothing in the table fits; the biggest are listed)
    pub fits: bool,
}

pub fn recommend(vcpu: u8, ram: f64, gpu: bool, region: &str, hours: f64) -> Vec<Candidate> {
    let hours = if hours.is_finite() { hours.clamp(1.0, 744.0) } else { HOURS };
    let mut v: Vec<Candidate> = crate::model::INSTANCE_TYPES.iter().filter(|i| i.1 >= vcpu && i.2 >= ram && crate::model::is_gpu(i.0) == gpu).map(|i| {
        let h = instance_hourly(i.0, region);
        Candidate { r#type: i.0.into(), vcpu: i.1, memory: i.2, arch: i.3.into(), gpu, hourly: h, monthly: h * hours, note: String::new(), fits: true }
    }).collect();
    if v.is_empty() {
        // the biggest the table has, so the user sees where it ends and can type a bigger type by hand
        let mut all: Vec<&(&str, u8, f64, &str, f64, f64)> = crate::model::INSTANCE_TYPES.iter().filter(|i| crate::model::is_gpu(i.0) == gpu).collect();
        all.sort_by(|a, b| (b.1, b.2).partial_cmp(&(a.1, a.2)).unwrap_or(std::cmp::Ordering::Equal));
        return all.into_iter().take(3).map(|i| { let h = instance_hourly(i.0, region); Candidate { r#type: i.0.into(), vcpu: i.1, memory: i.2, arch: i.3.into(), gpu, hourly: h, monthly: h * hours, note: "smaller than asked: the app's table ends here; a bigger type can be typed by hand".into(), fits: false } }).collect();
    }
    v.sort_by(|a, b| a.hourly.partial_cmp(&b.hourly).unwrap_or(std::cmp::Ordering::Equal));
    for (k, c) in v.iter_mut().enumerate() {
        c.note = if k == 0 { "cheapest that fits".into() } else if c.arch == "arm64" { "Graviton (arm64): Linux only, most software has arm builds".into() } else if c.vcpu > vcpu || c.memory > ram { "more than asked".into() } else { "x86_64".into() };
    }
    v.truncate(8);
    v
}

/// (region, NAT Gateway USD/hour, NAT data processing USD/GB, idle Elastic IP USD/hour)
const PRICES: &[(&str, f64, f64, f64)] = &[
    ("us-east-1", 0.045, 0.045, 0.005),
    ("us-east-2", 0.045, 0.045, 0.005),
    ("us-west-1", 0.045, 0.045, 0.005),
    ("us-west-2", 0.045, 0.045, 0.005),
    ("ca-central-1", 0.045, 0.045, 0.005),
    ("eu-west-1", 0.045, 0.045, 0.005),
    ("eu-west-2", 0.048, 0.048, 0.005),
    ("eu-west-3", 0.048, 0.048, 0.005),
    ("eu-central-1", 0.052, 0.052, 0.005),
    ("eu-north-1", 0.044, 0.044, 0.005),
    ("ap-south-1", 0.056, 0.056, 0.005),
    ("ap-south-2", 0.056, 0.056, 0.005),
    ("ap-southeast-1", 0.059, 0.059, 0.005),
    ("ap-southeast-2", 0.059, 0.059, 0.005),
    ("ap-northeast-1", 0.062, 0.062, 0.005),
    ("ap-northeast-2", 0.059, 0.059, 0.005),
    ("sa-east-1", 0.093, 0.093, 0.005),
    ("me-south-1", 0.058, 0.058, 0.005),
    ("af-south-1", 0.065, 0.065, 0.005),
];

pub fn nat_hourly(region: &str) -> f64 {
    PRICES.iter().find(|p| p.0 == region).map(|p| p.1).unwrap_or(0.056)
}
pub fn nat_per_gb(region: &str) -> f64 {
    PRICES.iter().find(|p| p.0 == region).map(|p| p.2).unwrap_or(0.056)
}
pub fn eip_idle_hourly(region: &str) -> f64 {
    PRICES.iter().find(|p| p.0 == region).map(|p| p.3).unwrap_or(0.005)
}
/// a public IPv4 address, in use or not (charged since February 2024)
pub const PUBLIC_IP_HOURLY: f64 = 0.005;
/// the root volume every instance gets (8 GiB gp3)
pub const ROOT_VOLUME_MONTHLY: f64 = 8.0 * 0.08;
/// gp3 storage, USD per GiB-month
pub const GP3_PER_GIB: f64 = 0.08;
/// what a Spot instance costs against on-demand, as a typical share (the real price moves; the Costs tab says so)
pub const SPOT_FACTOR: f64 = 0.3;
/// an interface endpoint's network interface, per hour per AZ (plus USD 0.01 per GB)
pub const INTERFACE_ENDPOINT_HOURLY: f64 = 0.01;

/// an Application Load Balancer per hour, before capacity units
pub fn alb_hourly(region: &str) -> f64 {
    match region { "us-east-1" | "us-east-2" | "us-west-2" => 0.0225, "ap-south-1" => 0.0239, "eu-west-1" => 0.0252, _ => 0.025 }
}

/// a bucket's storage a month from its expected GB and lifecycle (Standard 0.023, Infrequent Access 0.0125, Glacier Instant 0.004 per GB)
pub fn s3_monthly(n: &crate::model::Node) -> f64 {
    let gb = n.params.get("gb").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let per_gb = match n.params.get("lifecycle").and_then(|v| v.as_str()).unwrap_or("none") {
        // most of a growing bucket is old: take two thirds as moved to the cheaper class
        "ia-30" => 0.023 / 3.0 + 0.0125 * 2.0 / 3.0,
        "glacier-90" => 0.023 / 3.0 + 0.004 * 2.0 / 3.0,
        "ia-30-glacier-90" => 0.023 / 3.0 + 0.0125 / 3.0 + 0.004 / 3.0,
        _ => 0.023,
    };
    gb * per_gb
}

/// on-demand price of an instance type: the table has us-east-1 and ap-south-1; other regions are taken as us-east-1 × 1.1
pub fn instance_hourly(t: &str, region: &str) -> f64 {
    let Some(it) = instance_type(t) else { return 0.0 };
    match region {
        "us-east-1" => it.4,
        "ap-south-1" => it.5,
        _ => it.4 * 1.1,
    }
}

/// the Graviton (arm64) type of the same size as an x86 type, if the table has one
pub fn arm_equivalent(t: &str) -> Option<&'static str> {
    let (family, size) = t.split_once('.')?;
    let arm_family = match family { "t2" | "t3" => "t4g", "m5" => "m6g", "c5" => "c6g", "r5" => "r6g", _ => return None };
    let want = format!("{arm_family}.{size}");
    INSTANCE_TYPES.iter().find(|i| i.0 == want).map(|i| i.0)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostItem {
    pub id: String,
    pub label: String,
    pub kind: String,
    /// USD per month while idle
    pub monthly: f64,
    /// the arithmetic and what is not included
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostReport {
    pub items: Vec<CostItem>,
    pub total: f64,
    /// how to pay less, most saving first; each says what it costs in return
    pub advice: Vec<String>,
    pub region: String,
    pub assumptions: Vec<String>,
}

fn money(v: f64) -> String {
    format!("USD {v:.2}")
}

pub fn cost(d: &Design) -> CostReport {
    let region = d.region.clone();
    let nat_h = nat_hourly(&region);
    let mut items = vec![];
    let mut advice = vec![];
    let free = |kind: &str| matches!(kind, "vpc" | "subnet" | "igw" | "rt" | "sg" | "cidr");
    let tier_subnets = |vpc: &Option<String>, tier: &str| d.nodes.iter().filter(|s| s.kind == "subnet" && s.parent == *vpc && s.params.get("tier").and_then(|v| v.as_str()) == Some(tier)).count();
    let mut public_ips = 0;
    let mut x86: Vec<(String, &str, f64)> = vec![];
    for n in &d.nodes {
        if free(&n.kind) {
            continue;
        }
        if n.kind == "nat" {
            let monthly = nat_h * HOURS;
            items.push(CostItem { id: n.id.clone(), label: n.label.clone(), kind: n.kind.clone(), monthly,
                note: format!("{nat_h} USD/h × {HOURS:.0} h = {} a month, plus {} per GB processed", money(monthly), money(nat_per_gb(&region))) });
        }
        if n.kind == "vpce" {
            let svc = n.params.get("service").and_then(|v| v.as_str()).unwrap_or("");
            if crate::model::endpoint_type(svc) == Some("interface") {
                let azs = tier_subnets(&n.parent, n.params.get("tier").and_then(|v| v.as_str()).unwrap_or("private"));
                let monthly = azs as f64 * INTERFACE_ENDPOINT_HOURLY * HOURS;
                items.push(CostItem { id: n.id.clone(), label: n.label.clone(), kind: n.kind.clone(), monthly,
                    note: format!("{svc} interface endpoint: {azs} interface{} × {INTERFACE_ENDPOINT_HOURLY} USD/h × {HOURS:.0} h = {}, plus USD 0.01 per GB", if azs == 1 { "" } else { "s" }, money(monthly)) });
            }
        }
        if n.kind == "alb" {
            let monthly = alb_hourly(&region) * HOURS;
            items.push(CostItem { id: n.id.clone(), label: n.label.clone(), kind: n.kind.clone(), monthly,
                note: format!("{} USD/h × {HOURS:.0} h = {} a month idle, plus capacity units (about USD 6 per LCU-month; a small site uses one)", alb_hourly(&region), money(monthly)) });
            let targets = d.edges.iter().filter(|e| e.kind == "target" && e.source == n.id).count();
            let one_instance = d.edges.iter().filter(|e| e.kind == "target" && e.source == n.id).all(|e| d.node(&e.target).map(|t| t.kind == "ec2").unwrap_or(false));
            if targets <= 1 && one_instance {
                advice.push(format!("{} fronts a single instance: a load balancer costs {} a month idle. For one server, a public address (USD 3.65) and its security group do the job; a balancer pays off with several servers or an Auto Scaling group.", n.label, money(monthly)));
            }
        }
        if n.kind == "asg" {
            let hours = hours_of(n);
            let t = n.params.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let spot = n.params.get("spot").and_then(|v| v.as_bool()).unwrap_or(false);
            let desired = n.params.get("desired").and_then(|v| v.as_f64()).unwrap_or(1.0);
            let disk = n.params.get("disk").and_then(|v| v.as_f64()).unwrap_or(8.0);
            let h = instance_hourly(t, &region) * if spot { SPOT_FACTOR } else { 1.0 };
            let monthly = desired * (h * hours + disk * GP3_PER_GIB);
            items.push(CostItem { id: n.id.clone(), label: n.label.clone(), kind: n.kind.clone(), monthly,
                note: format!("{desired:.0} × {t}{}: {h:.4} USD/h × {hours:.0} h + {disk:.0} GiB each = {} a month at the desired size (more when it scales out)", if spot { " spot" } else { "" }, money(monthly)) });
            if !spot && desired >= 2.0 {
                advice.push(format!("{}: instances in an Auto Scaling group behind a balancer are replaceable, so Spot is safe there and saves about {} a month.", n.label, money(monthly * (1.0 - SPOT_FACTOR))));
            }
            if let Some(arm) = arm_equivalent(t) {
                let save = (instance_hourly(t, &region) - instance_hourly(arm, &region)) * hours * desired * if spot { SPOT_FACTOR } else { 1.0 };
                if save > 0.0 {
                    x86.push((n.label.clone(), arm, save));
                }
            }
        }
        if n.kind == "s3" {
            let monthly = s3_monthly(n);
            items.push(CostItem { id: n.id.clone(), label: n.label.clone(), kind: n.kind.clone(), monthly,
                note: format!("{:.0} GB stored ({}): {} a month; requests and transfer out are extra", n.params.get("gb").and_then(|v| v.as_f64()).unwrap_or(0.0), n.params.get("lifecycle").and_then(|v| v.as_str()).unwrap_or("none"), money(monthly)) });
        }
        if n.kind == "ec2" {
            let hours = hours_of(n);
            let t = n.params.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let spot = n.params.get("spot").and_then(|v| v.as_bool()).unwrap_or(false);
            let h = instance_hourly(t, &region) * if spot { SPOT_FACTOR } else { 1.0 };
            let disk = n.params.get("disk").and_then(|v| v.as_f64()).unwrap_or(8.0);
            let public = n.params.get("public_ip").and_then(|v| v.as_bool()).unwrap_or(false);
            let ip = if public { PUBLIC_IP_HOURLY * hours } else { 0.0 };
            let monthly = h * hours + disk * GP3_PER_GIB + ip;
            items.push(CostItem { id: n.id.clone(), label: n.label.clone(), kind: n.kind.clone(), monthly,
                note: format!("{t}{}: {h:.4} USD/h × {hours:.0} h = {} running {hours:.0} h a month, + {} for the {disk:.0} GiB root volume{}", if spot { " (spot, about 30 % of on-demand)" } else { "" }, money(h * hours), money(disk * GP3_PER_GIB), if public { format!(", + {} for the public IPv4 address", money(ip)) } else { String::new() }) });
            if !spot && instance_type(t).map(|it| it.0.starts_with('g') || it.0.starts_with('p')).unwrap_or(false) {
                advice.push(format!("{} is a GPU instance on demand ({} a month running all the time). Spot saves about 70 % for training that checkpoints and resumes (NN Builder does): tick Spot.", n.label, money(instance_hourly(t, &region) * hours)));
            }
            if public {
                public_ips += 1;
            }
            if let Some(arm) = arm_equivalent(t) {
                let save = (h - instance_hourly(arm, &region)) * hours;
                if save > 0.0 {
                    x86.push((n.label.clone(), arm, save));
                }
            }
        }
    }
    let total: f64 = items.iter().map(|i| i.monthly).sum();
    let instances = d.nodes.iter().filter(|n| n.kind == "ec2").count();
    if instances > 0 {
        let running: f64 = items.iter().filter(|i| i.kind == "ec2").map(|i| i.monthly).sum();
        advice.push(format!("{instances} instance{} cost {} a month running all the time. Stopping them outside working hours (say 10 of 24 hours, 5 days a week) keeps about 70 % of that: only the volumes and public addresses are charged while stopped.", if instances == 1 { "" } else { "s" }, money(running)));
    }
    if !x86.is_empty() {
        let save: f64 = x86.iter().map(|x| x.2).sum();
        advice.push(format!("Graviton (arm64) instead of x86 saves {} a month: {}. Most Linux software runs on arm64 unchanged.", money(save), x86.iter().map(|(l, arm, s)| format!("{l} → {arm} ({})", money(*s))).collect::<Vec<_>>().join(", ")));
    }
    if public_ips > 0 {
        advice.push(format!("{public_ips} public IPv4 address{} cost {} a month. Instances that only serve the VPC need none; for shell access use SSM Session Manager (free) instead of SSH over a public address.", if public_ips == 1 { "" } else { "es" }, money(public_ips as f64 * PUBLIC_IP_HOURLY * HOURS)));
    }

    // advice: NAT Gateways are the only idle cost in this version, and the usual one in a VPC
    let nats: Vec<_> = d.nodes.iter().filter(|n| n.kind == "nat").collect();
    let nat_month = nat_h * HOURS;
    if nats.len() > 1 {
        let saving = nat_month * (nats.len() - 1) as f64;
        advice.push(format!("{} NAT Gateways cost {} a month idle. One shared NAT Gateway saves {} a month; in return an outage of its AZ stops the other AZs' internet access and cross-AZ traffic is charged (USD 0.01/GB each way). Keep one per AZ only for production.", nats.len(), money(nat_month * nats.len() as f64), money(saving)));
    }
    if !nats.is_empty() {
        let unused: Vec<_> = nats.iter().filter(|n| !d.edges.iter().any(|e| e.kind == "route" && e.target == n.id)).collect();
        for n in &unused {
            advice.push(format!("{} carries no route: it costs {} a month for nothing. Delete it, or route a private subnet through it.", n.label, money(nat_month)));
        }
        let private: Vec<_> = d.nodes.iter().filter(|n| n.kind == "subnet" && n.params.get("tier").and_then(|v| v.as_str()) == Some("private")).collect();
        if !private.is_empty() {
            advice.push(format!("Private subnets only need a NAT Gateway for traffic to the internet. If the instances only need AWS services (S3, DynamoDB, SSM, ECR), mark the subnets isolated and use VPC endpoints: the S3 and DynamoDB gateway endpoints are free, and the design saves {} a month per NAT Gateway (endpoints come in a later version).", money(nat_month)));
        }
        advice.push("For a lab that is used by day only, destroy the NAT Gateways at night and recreate them in the morning: the cost is per hour (the \"Tear down for the night\" action comes with plan/apply).".into());
    }
    if nats.is_empty() && instances == 0 && d.nodes.iter().any(|n| n.kind == "subnet") && d.nodes.iter().all(|n| n.kind != "vpce" || crate::model::endpoint_type(n.params.get("service").and_then(|v| v.as_str()).unwrap_or("")) == Some("gateway")) {
        advice.push("No NAT Gateway and no instance: this network costs nothing a month by itself. VPCs, subnets, Internet Gateways, route tables, security groups and gateway endpoints are free; only what runs inside them costs money.".into());
    }
    let interface_eps = d.nodes.iter().filter(|n| n.kind == "vpce" && crate::model::endpoint_type(n.params.get("service").and_then(|v| v.as_str()).unwrap_or("")) == Some("interface")).count();
    if interface_eps >= 3 && nats.is_empty() {
        advice.push(format!("{interface_eps} interface endpoints cost about {} a month together. That is still less than one NAT Gateway ({}), and the traffic stays inside AWS; a NAT only wins when many services are needed.", money(items.iter().filter(|i| i.kind == "vpce").map(|i| i.monthly).sum::<f64>()), money(nat_month)));
    }
    if d.nodes.iter().any(|n| n.kind == "s3" && n.params.get("lifecycle").and_then(|v| v.as_str()) == Some("none") && n.params.get("gb").and_then(|v| v.as_f64()).unwrap_or(0.0) >= 50.0) {
        advice.push("Buckets without a lifecycle rule keep everything in Standard storage. A rule that moves objects older than 30 days to Infrequent Access halves that part of the bill; Glacier Instant Retrieval cuts it to a fifth.".into());
    }
    CostReport {
        items,
        total,
        advice,
        region,
        assumptions: vec![
            format!("On-demand list prices for {} ({} USD/h per NAT Gateway), {HOURS:.0} hours a month (an instance that says how many hours it runs is charged for those), no traffic.", d.region, nat_h),
            "Data transfer is not included: NAT processing and internet egress are charged per GB.".into(),
            "Free: VPC, subnets, Internet Gateway, route tables, security groups. An Elastic IP costs USD 0.005/h; the one a NAT Gateway uses is included above.".into(),
            "Instances: on-demand Linux prices (us-east-1 and ap-south-1 from the table; other regions as us-east-1 × 1.1), the gp3 root volume at USD 0.08 per GiB-month, a public IPv4 address at USD 0.005/h when turned on. Spot is taken as 30 % of on-demand (it moves between about 10 and 40 %, and the instance can be interrupted). Savings Plans come later.".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;

    #[test]
    fn recommends_the_cheapest_type_that_fits_at_the_asked_hours() {
        let r = recommend(2, 4.0, false, "ap-south-1", 176.0);
        assert_eq!(r[0].r#type, "t4g.medium");
        assert!((r[0].monthly - 0.0336 * 176.0).abs() < 0.001);
        assert!(r.iter().all(|c| c.vcpu >= 2 && c.memory >= 4.0 && !c.gpu));
        assert_eq!(recommend(4, 16.0, true, "ap-south-1", 730.0)[0].r#type, "g4dn.xlarge");
        let big = recommend(64, 1.0, false, "us-east-1", 730.0);
        assert!(!big.is_empty() && big.iter().all(|c| !c.fits) && big[0].vcpu == 8, "{big:?}");
        assert_eq!(recommend(1, 0.5, false, "us-east-1", 10000.0)[0].monthly, 0.0042 * 744.0);
        let mut d = crate::dsl::parse("net \"h\"\nregion ap-south-1\nazs 1\n\nvpc main 10.0.0.0/16\nsubnet a public 10.0.0.0/24 az a in main\nec2 box t4g.micro in a hours 176\n", None).unwrap();
        let c = cost(&d);
        let item = c.items.iter().find(|i| i.label == "box").unwrap();
        assert!((item.monthly - (0.0084 * 176.0 + 8.0 * 0.08)).abs() < 0.001, "{}", item.monthly);
        assert!(item.note.contains("× 176 h"));
        d.nodes.iter_mut().find(|n| n.label == "box").unwrap().params.remove("hours");
        assert!(cost(&d).items.iter().find(|i| i.label == "box").unwrap().note.contains("× 730 h"));
    }

    #[test]
    fn two_tier_costs_two_nats_and_advises_one() {
        let d = templates::build("two_tier").unwrap();
        let c = cost(&d);
        assert_eq!(c.items.len(), 4, "two NATs, the balancer, the group");
        let nats: f64 = c.items.iter().filter(|i| i.kind == "nat").map(|i| i.monthly).sum();
        assert!((nats - 2.0 * 0.056 * 730.0).abs() < 0.01, "{nats}");
        assert!(c.advice[0].contains("saves USD 40.88"), "{}", c.advice[0]);
        assert!(c.items.iter().find(|i| i.kind == "nat").unwrap().note.contains("0.056 USD/h × 730 h"));
        let three = cost(&templates::build("three_tier").unwrap());
        assert_eq!(three.items.len(), 1);
        assert!(!three.advice.iter().any(|a| a.contains("One shared NAT")));
        let single = cost(&templates::build("single_public").unwrap());
        assert_eq!(single.total, 0.0);
        assert!(single.advice[0].contains("costs nothing"));
        assert_eq!(instance_hourly("t4g.micro", "ap-south-1"), 0.0084);
        assert!((instance_hourly("t3.micro", "eu-west-1") - 0.0104 * 1.1).abs() < 1e-9);
        assert_eq!(arm_equivalent("t3.small"), Some("t4g.small"));
        assert_eq!(arm_equivalent("g4dn.xlarge"), None);
    }

    #[test]
    fn endpoints_and_buckets() {
        let d = crate::dsl::parse("vpc main 10.0.0.0/16\nsubnet a private 10.0.0.0/24 az a in main\nsubnet b private 10.0.1.0/24 az b in main\nvpce s3_ep s3 in main\nvpce ssm_ep ssm in main tier private\ns3 data gb 100\ns3 logs gb 100 lifecycle glacier-90\n", None).unwrap();
        let c = cost(&d);
        let item = |l: &str| c.items.iter().find(|i| i.label == l).map(|i| i.monthly).unwrap_or(-1.0);
        assert_eq!(item("s3_ep"), -1.0, "gateway endpoints are free and not listed");
        assert!((item("ssm_ep") - 2.0 * 0.01 * 730.0).abs() < 0.01, "{}", item("ssm_ep"));
        assert!((item("data") - 2.3).abs() < 0.01);
        assert!(item("logs") < 1.2, "{}", item("logs"));
        assert!(c.advice.iter().any(|a| a.contains("lifecycle rule")));
    }

    #[test]
    fn an_unused_nat_is_called_out() {
        let d = crate::dsl::parse("vpc main 10.0.0.0/16\nsubnet p public 10.0.0.0/24 az a in main\nnat n in p\n", None).unwrap();
        let c = cost(&d);
        assert!(c.advice.iter().any(|a| a.contains("carries no route")));
        assert_eq!(nat_hourly("nowhere"), 0.056);
    }
}
