//! Ready-made designs (the counterpart of NN Builder's demos), written in the text language so that they also test it.

use crate::dsl;
use crate::model::Design;
use anyhow::{anyhow, Result};

pub struct Template {
    pub key: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub text: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        key: "single_public",
        title: "Single public subnet",
        about: "The smallest VPC that can reach the internet: one public subnet in one AZ, an Internet Gateway and a route table. For learning, or one instance.",
        text: "net \"Single public subnet\"\nregion ap-south-1\nazs 1\n\nvpc main 10.0.0.0/16\nigw igw in main\nsubnet public_a public 10.0.0.0/24 az a in main rt public\nrt public in main: 0.0.0.0/0 -> igw\n",
    },
    Template {
        key: "web_server",
        title: "One web server (cheapest)",
        about: "A public subnet, one t4g.micro (about USD 7 a month in Mumbai) with a public address, HTTPS open to the internet and SSH only from your own address range. Change `office` to your IP. The cheapest thing that serves a website.",
        text: "net \"One web server\"\nregion ap-south-1\nazs 1\n\nvpc main 10.0.0.0/16\nigw igw in main\nsubnet public_a public 10.0.0.0/24 az a in main rt public\n\
ec2 web_1 t4g.micro in public_a sg web key my_key public\nrt public in main: 0.0.0.0/0 -> igw\nsg web in main\n\ncidr internet 0.0.0.0/0\ncidr office 203.0.113.0/24\n\n\
rule internet -> web tcp 443\nrule internet -> web tcp 80\nrule office -> web tcp ssh\n",
    },
    Template {
        key: "two_tier",
        title: "Two-tier VPC (2 AZs)",
        about: "Public and private subnets in two Availability Zones, one NAT Gateway per AZ (no single point of failure; about USD 64 a month idle). The network of a load balancer in front of application servers.",
        text: "net \"Two-tier VPC\"\nregion ap-south-1\nazs 2\n\nvpc main 10.0.0.0/16\nigw igw in main\n\
subnet public_a public 10.0.0.0/20 az a in main rt public\nsubnet public_b public 10.0.16.0/20 az b in main rt public\n\
subnet app_a private 10.0.32.0/20 az a in main rt app_a_rt\nsubnet app_b private 10.0.48.0/20 az b in main rt app_b_rt\n\
nat nat_a in public_a\nnat nat_b in public_b\n\
rt public in main: 0.0.0.0/0 -> igw\nrt app_a_rt in main: 0.0.0.0/0 -> nat_a\nrt app_b_rt in main: 0.0.0.0/0 -> nat_b\nsg app in main\nsg web in main\n\ncidr internet 0.0.0.0/0\n\nrule internet -> web tcp 443\nrule web -> app tcp 8080\n",
    },
    Template {
        key: "three_tier",
        title: "Three-tier VPC (2 AZs, shared NAT)",
        about: "Public, private and isolated tiers in two AZs. One NAT Gateway shared by both AZs saves about USD 32 a month; the app warns that AZ b depends on AZ a. The isolated tier (databases) has no route to the internet at all.",
        text: "net \"Three-tier VPC\"\nregion ap-south-1\nazs 2\n\nvpc main 10.0.0.0/16\nigw igw in main\n\
subnet public_a public 10.0.0.0/20 az a in main rt public\nsubnet public_b public 10.0.16.0/20 az b in main rt public\n\
subnet app_a private 10.0.32.0/20 az a in main rt app\nsubnet app_b private 10.0.48.0/20 az b in main rt app\n\
subnet db_a isolated 10.0.64.0/24 az a in main rt db\nsubnet db_b isolated 10.0.65.0/24 az b in main rt db\n\
nat nat_a in public_a\n\
rt public in main: 0.0.0.0/0 -> igw\nrt app in main: 0.0.0.0/0 -> nat_a\nrt db in main\n",
    },
    Template {
        key: "broken",
        title: "Spot the mistakes",
        about: "A design with the usual mistakes, every one caught: a subnet outside the VPC, two that overlap, a private subnet routed to the Internet Gateway, a NAT in a private subnet, and an isolated subnet with a default route.",
        text: "net \"Spot the mistakes\"\nregion ap-south-1\nazs 2\n\nvpc main 10.0.0.0/16\nigw igw in main\n\
subnet public_a public 10.0.0.0/20 az a in main rt public\nsubnet public_b public 10.1.0.0/20 az b in main rt public\n\
subnet app_a private 10.0.8.0/20 az a in main rt public\nsubnet app_b private 10.0.48.0/20 az b in main rt app\n\
subnet db_a isolated 10.0.64.0/24 az a in main rt public\n\
nat nat_b in app_b\n\
rt public in main: 0.0.0.0/0 -> igw\nrt app in main: 0.0.0.0/0 -> nat_b\n",
    },
];

pub fn build(key: &str) -> Result<Design> {
    let t = TEMPLATES.iter().find(|t| t.key == key).ok_or_else(|| anyhow!("no template {key}"))?;
    let mut d = dsl::parse(t.text, None).map_err(|e| anyhow!("template {key}: {:?}", e))?;
    dsl::sync(&mut d);
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::analyse;

    #[test]
    fn every_template_parses_and_prints_back() {
        for t in TEMPLATES {
            let d = build(t.key).unwrap();
            assert_eq!(dsl::print(&d), t.text, "{}: the template text is in the printed form", t.key);
            assert!(d.nodes.iter().all(|n| n.kind != "vpc" || n.w > 400.0), "{}: arranged", t.key);
            let a = analyse(&d);
            if t.key == "broken" {
                assert!(a.errors >= 5, "{:?}", a.problems);
            } else {
                assert!(a.ok, "{}: {:?}", t.key, a.problems);
            }
            if t.key == "three_tier" {
                assert_eq!(a.warnings, 1, "{:?}", a.problems);
                assert!(a.problems[0].contains("is in AZ a"));
            } else if t.key == "two_tier" {
                assert!(a.problems.iter().all(|p| p.contains("No instance uses")), "{:?}", a.problems);
            } else if t.key != "broken" {
                assert_eq!(a.warnings, 0, "{:?}", a.problems);
            }
        }
    }
}
