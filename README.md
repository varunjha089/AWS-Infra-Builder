# AWS Infra Builder

Draw AWS infrastructure on a canvas, have every part checked while it is drawn, and (in later versions) turn the finished design into
real AWS resources with one reviewed, reversible step.

Building a VPC by hand in the AWS console takes about fifteen screens, in the right order, and nothing shows the whole picture.
Terraform shows the whole text but not the picture, and mistakes appear only at `plan` or `apply`, or never. This app catches a
wrong CIDR, a private subnet with no way out, or a NAT Gateway in the wrong subnet at design time, on the block itself, in plain
words, with what to change.

**Version 0.1.0 (phase 0): networking on the canvas and in text.** Nothing in this version talks to AWS.

## What it does now

- **Design tab:** a VPC is a box; subnets are boxes inside it, laid out as Availability-Zone columns × tier rows (public, private,
  isolated); NAT Gateways sit inside their subnet; Internet Gateways and route tables are small blocks. Arrows: a subnet → a route
  table (the subnet uses it), a route table → a gateway (a route, with its destination). Drag a block into another box to move it there.
- **Checks on every block:** VPC and subnet CIDRs between /16 and /28, inside the VPC, not overlapping; usable addresses shown as
  `2^(32−24) = 256 − 5 = 251`; a public subnet must route `0.0.0.0/0` to the Internet Gateway, a private one to a NAT Gateway, an
  isolated one nowhere; a NAT Gateway must be in a public subnet; a private subnet whose NAT is in another AZ gets a warning; one
  Internet Gateway per VPC; a region's AZ count; routes inside the VPC's own range; duplicate names. Red means AWS would refuse it
  or the design contradicts itself; yellow means it works but is probably not what you want.
- **Text tab:** the same design as `.aib` text, one line per block, kept in step both ways:

  ```text
  net "Two-tier VPC"
  region ap-south-1
  azs 2

  vpc main 10.0.0.0/16
  igw igw in main
  subnet public_a public 10.0.0.0/20 az a in main rt public
  subnet app_a private 10.0.32.0/20 az a in main rt app_a_rt
  nat nat_a in public_a
  rt public in main: 0.0.0.0/0 -> igw
  rt app_a_rt in main: 0.0.0.0/0 -> nat_a
  ```

- **Templates:** a single public subnet, a two-tier VPC in 2 AZs, a three-tier VPC with a shared NAT, and "Spot the mistakes".
- **Projects:** a folder with `project.json` and `design.json`, wherever you choose. A template opens unsaved until you save it.

## Roadmap

| Phase | Contents |
|---|---|
| 0 (this) | canvas, CIDR and routing checks, `.aib` text, templates |
| 1 | security groups as traffic arrows, EC2, Terraform/OpenTofu HCL generation, `tofu validate` in CI |
| 2 | plan / apply / destroy with a bundled OpenTofu, the plan drawn on the canvas, state backups, account guard |
| 3 | ALB, Auto Scaling, RDS, S3, VPC endpoints; reachability questions; cost estimates |
| 4 | import an existing VPC, drift check, least-privilege IAM policy generator, budget guard |

## Build

Rust (stable), Node 20+, and the Tauri 2 prerequisites for your system.

```sh
cd web && npm ci && npm run build && cd ..
cargo test -p aib-core
cd app && npm ci && npx tauri build          # the bundle lands in target/release/bundle/
```

The settings folder is `~/Library/Application Support/AWS Infra Builder` on macOS (`AWS_INFRA_BUILDER_HOME` overrides it); projects go to
`~/AWS_Infra_Builder_Projects` unless you choose another folder.

## Licence

MIT.
