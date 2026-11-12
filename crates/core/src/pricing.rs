//! Cost: what the design costs per month before any traffic, per block and in total, and how to pay less. Prices are a bundled
//! table of on-demand list prices (USD per hour; 730 hours a month); every number is an estimate and says so. The principle: the
//! cheapest design that does the job is the default, and anything that costs money while idle is pointed out with the alternative.

use crate::model::Design;
use serde::{Deserialize, Serialize};

pub const HOURS: f64 = 730.0;

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
    let free = |kind: &str| matches!(kind, "vpc" | "subnet" | "igw" | "rt");
    for n in &d.nodes {
        if free(&n.kind) {
            continue;
        }
        if n.kind == "nat" {
            let monthly = nat_h * HOURS;
            items.push(CostItem { id: n.id.clone(), label: n.label.clone(), kind: n.kind.clone(), monthly,
                note: format!("{nat_h} USD/h × {HOURS:.0} h = {} a month, plus {} per GB processed", money(monthly), money(nat_per_gb(&region))) });
        }
    }
    let total: f64 = items.iter().map(|i| i.monthly).sum();

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
    if nats.is_empty() && d.nodes.iter().any(|n| n.kind == "subnet") {
        advice.push("No NAT Gateway: this network costs nothing a month by itself. VPCs, subnets, Internet Gateways and route tables are free; only what runs inside them costs money.".into());
    }
    CostReport {
        items,
        total,
        advice,
        region,
        assumptions: vec![
            format!("On-demand list prices for {} ({} USD/h per NAT Gateway), {HOURS:.0} hours a month, no traffic.", d.region, nat_h),
            "Data transfer is not included: NAT processing and internet egress are charged per GB.".into(),
            "Free: VPC, subnets, Internet Gateway, route tables, security groups. An Elastic IP costs USD 0.005/h; the one a NAT Gateway uses is included above.".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;

    #[test]
    fn two_tier_costs_two_nats_and_advises_one() {
        let d = templates::build("two_tier").unwrap();
        let c = cost(&d);
        assert_eq!(c.items.len(), 2);
        assert!((c.total - 2.0 * 0.056 * 730.0).abs() < 0.01, "{}", c.total);
        assert!(c.advice[0].contains("saves USD 40.88"), "{}", c.advice[0]);
        assert!(c.items[0].note.contains("0.056 USD/h × 730 h"));
        let three = cost(&templates::build("three_tier").unwrap());
        assert_eq!(three.items.len(), 1);
        assert!(!three.advice.iter().any(|a| a.contains("One shared NAT")));
        let single = cost(&templates::build("single_public").unwrap());
        assert_eq!(single.total, 0.0);
        assert!(single.advice[0].contains("costs nothing"));
    }

    #[test]
    fn an_unused_nat_is_called_out() {
        let d = crate::dsl::parse("vpc main 10.0.0.0/16\nsubnet p public 10.0.0.0/24 az a in main\nnat n in p\n", None).unwrap();
        let c = cost(&d);
        assert!(c.advice.iter().any(|a| a.contains("carries no route")));
        assert_eq!(nat_hourly("nowhere"), 0.056);
    }
}
