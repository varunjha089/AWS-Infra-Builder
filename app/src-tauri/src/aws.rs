//! The read-only AWS calls (AWS SDK for Rust): who am I, which VPCs are there, and everything in one VPC. Credentials come from a
//! profile in ~/.aws (the SDK reads the files and handles SSO) or from the keys pasted into the app (handed to the SDK for one
//! call at a time; they live in the keychain, see main.rs). Only Describe* and GetCallerIdentity are ever called here.

use aib_core::import::{EndpointInfo, InstanceInfo, Inventory, NatInfo, PermInfo, RouteInfo, RouteTableInfo, SgInfo, SubnetInfo, VpcInfo};
use aws_config::{BehaviorVersion, Region};
use aws_sdk_ec2::types::Filter;
use serde::Serialize;
use std::time::Duration;

/// the credentials for one call
pub enum Creds {
    Profile(String),
    Keys { id: String, secret: String, token: Option<String> },
}

#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    pub account: String,
    pub arn: String,
    pub user_id: String,
}

const TIMEOUT: Duration = Duration::from_secs(60);

async fn config(creds: Creds, region: &str) -> aws_config::SdkConfig {
    let mut loader = aws_config::defaults(BehaviorVersion::latest()).region(Region::new(region.to_string()));
    loader = match creds {
        Creds::Profile(name) => loader.profile_name(name),
        Creds::Keys { id, secret, token } => loader.credentials_provider(aws_credential_types::Credentials::new(id, secret, token, None, "aws-infra-builder-pasted")),
    };
    loader.load().await
}

fn readable<E: std::error::Error + 'static>(e: E) -> String {
    let s = format!("{}", aws_sdk_ec2::error::DisplayErrorContext(&e));
    // the SDK's context is long; keep the first line and the service message
    let first = s.lines().next().unwrap_or("").to_string();
    let msg = s.split("message: \"").nth(1).and_then(|m| m.split('"').next()).map(|m| format!(" ({m})")).unwrap_or_default();
    let mut out = format!("{first}{msg}");
    if out.contains("NoCredentialProviders") || out.contains("no credentials") || out.contains("failed to load credentials") {
        out.push_str(". Choose a profile from ~/.aws or paste keys in the Connection tab; for an SSO profile run `aws sso login` first.");
    }
    if out.contains("UnauthorizedOperation") {
        out.push_str(". The key needs the read-only policy shown in the Connection tab (ec2:Describe*).");
    }
    if out.contains("ExpiredToken") || out.contains("expired") {
        out.push_str(". The session has expired: run `aws sso login` again or paste fresh keys.");
    }
    out
}

async fn with_timeout<T, F: std::future::Future<Output = Result<T, String>>>(f: F) -> Result<T, String> {
    tokio::time::timeout(TIMEOUT, f).await.map_err(|_| "AWS did not answer within 60 seconds. Check the internet connection and the region.".to_string())?
}

pub async fn identity(creds: Creds, region: &str) -> Result<Identity, String> {
    with_timeout(async {
        let conf = config(creds, region).await;
        let out = aws_sdk_sts::Client::new(&conf).get_caller_identity().send().await.map_err(readable)?;
        Ok(Identity { account: out.account().unwrap_or("").into(), arn: out.arn().unwrap_or("").into(), user_id: out.user_id().unwrap_or("").into() })
    }).await
}

fn name_tag(tags: &[aws_sdk_ec2::types::Tag]) -> Option<String> {
    tags.iter().find(|t| t.key() == Some("Name")).and_then(|t| t.value()).map(String::from).filter(|s| !s.trim().is_empty())
}

pub async fn list_vpcs(creds: Creds, region: &str) -> Result<Vec<VpcInfo>, String> {
    with_timeout(async {
        let conf = config(creds, region).await;
        let ec2 = aws_sdk_ec2::Client::new(&conf);
        let vpcs = ec2.describe_vpcs().send().await.map_err(readable)?;
        let subnets = ec2.describe_subnets().send().await.map_err(readable)?;
        Ok(vpcs.vpcs().iter().map(|v| {
            let id = v.vpc_id().unwrap_or("").to_string();
            VpcInfo {
                name: name_tag(v.tags()),
                cidr: v.cidr_block().unwrap_or("").into(),
                is_default: v.is_default().unwrap_or(false),
                subnets: subnets.subnets().iter().filter(|s| s.vpc_id() == Some(id.as_str())).count(),
                id,
            }
        }).collect())
    }).await
}

pub async fn inventory(creds: Creds, region: &str, vpc_id: &str, account: &str) -> Result<Inventory, String> {
    with_timeout(async {
        let conf = config(creds, region).await;
        let ec2 = aws_sdk_ec2::Client::new(&conf);
        let by_vpc = |name: &str| Filter::builder().name(name).values(vpc_id).build();
        let vpcs = ec2.describe_vpcs().vpc_ids(vpc_id).send().await.map_err(readable)?;
        let v = vpcs.vpcs().first().ok_or_else(|| format!("There is no VPC {vpc_id} in {region}."))?;
        let vpc = VpcInfo { id: vpc_id.into(), name: name_tag(v.tags()), cidr: v.cidr_block().unwrap_or("").into(), is_default: v.is_default().unwrap_or(false), subnets: 0 };
        let subnets = ec2.describe_subnets().filters(by_vpc("vpc-id")).send().await.map_err(readable)?;
        let subnets: Vec<SubnetInfo> = subnets.subnets().iter().map(|s| SubnetInfo {
            id: s.subnet_id().unwrap_or("").into(), name: name_tag(s.tags()), cidr: s.cidr_block().unwrap_or("").into(),
            az: s.availability_zone().unwrap_or("").into(), map_public_ip: s.map_public_ip_on_launch().unwrap_or(false),
        }).collect();
        let igws = ec2.describe_internet_gateways().filters(by_vpc("attachment.vpc-id")).send().await.map_err(readable)?;
        let igws = igws.internet_gateways().iter().map(|g| (g.internet_gateway_id().unwrap_or("").to_string(), name_tag(g.tags()))).collect();
        let nats = ec2.describe_nat_gateways().filter(by_vpc("vpc-id")).send().await.map_err(readable)?;
        let nats = nats.nat_gateways().iter().map(|n| NatInfo {
            id: n.nat_gateway_id().unwrap_or("").into(), name: name_tag(n.tags()), subnet_id: n.subnet_id().unwrap_or("").into(),
            state: n.state().map(|s| s.as_str().to_string()).unwrap_or_default(),
        }).collect();
        let rts = ec2.describe_route_tables().filters(by_vpc("vpc-id")).send().await.map_err(readable)?;
        let route_tables = rts.route_tables().iter().map(|r| RouteTableInfo {
            id: r.route_table_id().unwrap_or("").into(),
            name: name_tag(r.tags()),
            main: r.associations().iter().any(|a| a.main().unwrap_or(false)),
            subnet_ids: r.associations().iter().filter_map(|a| a.subnet_id()).map(String::from).collect(),
            routes: r.routes().iter().filter_map(|ro| {
                let dest = ro.destination_cidr_block().or(ro.destination_prefix_list_id()).or(ro.destination_ipv6_cidr_block())?.to_string();
                let target = ro.gateway_id().or(ro.nat_gateway_id()).or(ro.vpc_peering_connection_id()).or(ro.transit_gateway_id())
                    .or(ro.network_interface_id()).or(ro.instance_id()).or(ro.egress_only_internet_gateway_id()).unwrap_or("?").to_string();
                Some(RouteInfo { dest, target, state: ro.state().map(|s| s.as_str().to_string()).unwrap_or_default() })
            }).collect(),
        }).collect();
        let perm = |p: &aws_sdk_ec2::types::IpPermission| PermInfo {
            proto: p.ip_protocol().unwrap_or("-1").to_string(),
            from: p.from_port().unwrap_or(0),
            to: p.to_port().unwrap_or(0),
            cidrs: p.ip_ranges().iter().filter_map(|r| r.cidr_ip()).map(String::from).collect(),
            groups: p.user_id_group_pairs().iter().filter_map(|g| g.group_id()).map(String::from).collect(),
        };
        let sgs = ec2.describe_security_groups().filters(by_vpc("vpc-id")).send().await.map_err(readable)?;
        let sgs = sgs.security_groups().iter().map(|g| SgInfo {
            id: g.group_id().unwrap_or("").into(), name: name_tag(g.tags()), group_name: g.group_name().unwrap_or("").into(), description: g.description().unwrap_or("").into(),
            ingress: g.ip_permissions().iter().map(perm).collect(), egress: g.ip_permissions_egress().iter().map(perm).collect(),
        }).collect();
        let res = ec2.describe_instances().filters(by_vpc("vpc-id")).send().await.map_err(readable)?;
        let mut instances: Vec<InstanceInfo> = res.reservations().iter().flat_map(|r| r.instances().iter()).map(|i| InstanceInfo {
            id: i.instance_id().unwrap_or("").into(), name: name_tag(i.tags()), instance_type: i.instance_type().map(|t| t.as_str().to_string()).unwrap_or_default(),
            subnet_id: i.subnet_id().unwrap_or("").into(), sgs: i.security_groups().iter().filter_map(|g| g.group_id()).map(String::from).collect(),
            public_ip: i.public_ip_address().is_some(), key: i.key_name().map(String::from), state: i.state().and_then(|s| s.name()).map(|n| n.as_str().to_string()).unwrap_or_default(),
            image_name: i.image_id().map(String::from),
            spot: i.instance_lifecycle().map(|l| l.as_str() == "spot").unwrap_or(false),
            disk: None,
            address: i.public_ip_address().map(String::from).or_else(|| i.public_dns_name().filter(|s| !s.is_empty()).map(String::from)),
            private_ip: i.private_ip_address().map(String::from),
        }).collect();
        // the AMI names, to guess each instance's OS (one call for all)
        let image_ids: Vec<String> = instances.iter().filter_map(|i| i.image_name.clone()).collect();
        if !image_ids.is_empty() {
            if let Ok(imgs) = ec2.describe_images().set_image_ids(Some(image_ids)).send().await {
                for inst in instances.iter_mut() {
                    let name = imgs.images().iter().find(|im| im.image_id() == inst.image_name.as_deref()).and_then(|im| im.name()).map(String::from);
                    inst.image_name = name;
                }
            }
        }
        let eps = ec2.describe_vpc_endpoints().filters(by_vpc("vpc-id")).send().await.map_err(readable)?;
        let endpoints = eps.vpc_endpoints().iter().map(|e| EndpointInfo {
            id: e.vpc_endpoint_id().unwrap_or("").into(), name: name_tag(e.tags()), service_name: e.service_name().unwrap_or("").into(),
            endpoint_type: e.vpc_endpoint_type().map(|t| t.as_str().to_string()).unwrap_or_default(),
            route_table_ids: e.route_table_ids().to_vec(), subnet_ids: e.subnet_ids().to_vec(), sg_ids: e.groups().iter().filter_map(|g| g.group_id()).map(String::from).collect(),
        }).collect();
        Ok(Inventory { region: region.into(), account: account.into(), vpc, subnets, igws, nats, route_tables, sgs, instances, endpoints })
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a real, read-only call with the owner's default profile: only when asked for (AWS_INFRA_BUILDER_REAL=1), never in CI
    #[test]
    #[ignore]
    fn real_identity_and_vpcs() {
        if std::env::var("AWS_INFRA_BUILDER_REAL").is_err() {
            return;
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let region = std::env::var("AWS_INFRA_BUILDER_REGION").unwrap_or_else(|_| "ap-south-1".into());
        let id = rt.block_on(identity(Creds::Profile("default".into()), &region)).unwrap();
        assert_eq!(id.account.len(), 12, "{id:?}");
        let vpcs = rt.block_on(list_vpcs(Creds::Profile("default".into()), &region)).unwrap();
        println!("account {} · {} VPC(s) in {region}", id.account, vpcs.len());
        if let Some(v) = vpcs.first() {
            let inv = rt.block_on(inventory(Creds::Profile("default".into()), &region, &v.id, &id.account)).unwrap();
            let im = aib_core::import::to_design(&inv);
            println!("{}\n{}", im.notes.join("\n"), im.design.text.unwrap_or_default());
            assert_eq!(im.design.nodes.iter().filter(|n| n.kind == "subnet").count(), inv.subnets.len());
        }
    }
}
