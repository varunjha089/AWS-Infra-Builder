# AWS Infra Builder

Draw AWS infrastructure on a canvas, have every part checked while it is drawn, and (in later versions) turn the finished design into
real AWS resources with one reviewed, reversible step.

Building a VPC by hand in the AWS console takes about fifteen screens, in the right order, and nothing shows the whole picture.
Terraform shows the whole text but not the picture, and mistakes appear only at `plan` or `apply`, or never. This app catches a
wrong CIDR, a private subnet with no way out, or a NAT Gateway in the wrong subnet at design time, on the block itself, in plain
words, with what to change.

**Version 0.5.0: networking, security groups, instances, Spot, VPC endpoints, S3, load balancers and Auto Scaling on the canvas and in
text; OpenTofu/Terraform code; Plan & Apply from the app; costs; a read-only look at your account; neon night mode.** The app itself only reads from AWS; resources are created only when you press
Apply after seeing the plan, and the Costs tab tells you beforehand what they cost a month.

## What it does now

- **Design tab:** a VPC is a box; subnets are boxes inside it, laid out as Availability-Zone columns × tier rows (public, private,
  isolated); NAT Gateways sit inside their subnet; Internet Gateways and route tables are small blocks. Arrows: a subnet → a route
  table (the subnet uses it), a route table → a gateway (a route, with its destination). Drag a block into another box to move it there.
  **Arrange** (⇧⌘A) lays everything out from the innermost block outwards: every block gets the room its text needs, a subnet grows
  around its instances, a VPC around its subnets, gateways, route tables and groups (long lists wrap into rows), so nothing overlaps.
- **Tidy up:** with nothing selected, the inspector lists what would clean the design and does it at a click (one undo step each):
  delete the security groups nothing uses and the address ranges no rule uses; merge groups with exactly the same rules (nothing opens);
  fold a group into one whose rules contain its own, saying what its instances would newly let in; combine the `launch-wizard-N`
  groups the console makes one per launch into one group with the union of their rules, listing per group what that opens. Made for
  an imported account that grew by clicking "Launch instance".
- **Fix buttons:** every problem the app can repair has one, on the block or arrow and in the inspector's list: write the network
  address, use the next free block, attach or complete the public route table, mark the tier the routes say, delete what nothing uses,
  the nearest known instance type, Graviton for the same size, the public IP off (or on, with its price), the scaling group's sizes,
  a lifecycle rule. "Fix all safe ones" runs those that cost nothing and open nothing. "Allow SSH only from my current address" looks
  your public IP up (checkip.amazonaws.com) and puts it in an Address range block, leaving the web ports open as they were.
- **New instance by need:** adding an instance asks for vCPUs, memory, GPU or not, the OS and the hours a month it will run,
  and lists the types that fit, cheapest first, priced at those hours (Spot and Graviton noted). The estimate on the block, in
  the summary and in the Costs tab follows the hours you set (730 = always on; 176 = working hours), not a full month by
  default; "Recommend a type…" on an existing instance opens the same dialog. Every value is yours to type: any vCPU or memory
  number, any instance type name (one the app does not know gets a warning and no price, not a refusal), a key pair name with
  the ones found in `~/.ssh` and in the account suggested. Images: Amazon Linux 2023, Ubuntu 24.04 and 22.04, Debian 12, the
  Deep Learning AMI (PyTorch, CUDA) and Base AMI, OpenVPN Access Server; each says what it needs (x86 only, a 30 GiB root
  volume, a GPU, the VPN ports) and the checks hold you to it.
- **Lambda functions and start/stop:** a Lambda block stops or starts the instances you tick at a time of day in your time
  zone (weekdays, daily or weekends; EventBridge Scheduler, free tier), or runs your own Python; the generated code gets a role
  that may do nothing else. "Schedule stop and start" on an instance adds the evening/morning pair so a box priced for working
  hours really is off at night. The real instance behind an imported or applied block can be stopped, started and refreshed
  from the inspector; Start asks once more and shows the hourly price.
- **The whole picture on import:** load balancers (with their targets as arrows) and Auto Scaling groups come in as blocks;
  anything else that lives in the VPC and has no block in the palette (an EFS mount target, a database, a Lambda, a peering
  connection a route points at) is drawn as a grey, read-only block, so nothing is hidden. The app never writes anything for
  a grey block.
- **Adopt what exists:** a plan of an imported VPC adopts the resources it found (OpenTofu `import` blocks, written for every
  block that carries an AWS id and is not yet in the state) instead of creating them again: "adopt 29, create 0". An adopted
  instance keeps its AMI and root volume. If a plan would still create a second copy of something that exists, the app says
  so and refuses to apply it.
- **Terminal tab:** SSH into your instances from inside the app, like Termius. Every instance of the design is a host with the
  user its OS expects, the key pair's `.pem` file and its address (from the import, or from the outputs after Apply); press
  "Connect (SSH)" on an instance block, or save any other machine in the list. It runs your own `ssh` in a real terminal
  (xterm.js), so host keys, agents and `~/.ssh/config` work as they always do; the app never reads a key.
- **Keys:** the key files in `~/.ssh` are offered by name; a key chosen anywhere else can be moved into `~/.ssh` with mode 600
  in one click (ssh refuses a key others can read); "New key pair in AWS" makes an ed25519 pair (free) and writes its key once
  to `~/.ssh/<name>.pem`; "List the key pairs in AWS" shows which pairs have their file on this computer.
- **Checks on every block:** VPC and subnet CIDRs between /16 and /28, inside the VPC, not overlapping; usable addresses shown as
  `2^(32−24) = 256 − 5 = 251`; a public subnet must route `0.0.0.0/0` to the Internet Gateway, a private one to a NAT Gateway, an
  isolated one nowhere; a NAT Gateway must be in a public subnet; a private subnet whose NAT is in another AZ gets a warning; one
  Internet Gateway per VPC; a region's AZ count; routes inside the VPC's own range; duplicate names. Red means AWS would refuse it
  or the design contradicts itself; yellow means it works but is probably not what you want.
- **Security groups and instances:** a security group is a block; an arrow from a group or an address range (the internet, your office IP)
  to a group is the traffic it lets in, labelled with the ports (`tcp 5432 (postgres)`). An instance lives in a subnet, with its type and
  monthly price, OS, key pair, public address and groups. Checks: SSH, RDP or a database port open to the whole internet, groups nobody
  uses or nothing can reach, a public IP in a private tier, SSH allowed without a key pair, an instance without a group.
- **Code tab:** the design as `main.tf`, `variables.tf` and `outputs.tf` for the AWS provider, one resource per block with a comment naming
  the block and the tags `managed-by = aws-infra-builder`, `project`, `design-block`. Copy or export; every template's code passes
  `tofu validate` (checked in CI).
- **Plan & Apply tab:** OpenTofu is found on your PATH or installed into the app's folder from the official release (checksum-verified).
  *Plan* is read-only and is drawn on the canvas: green = will be created, yellow = changed, orange = replaced, red = destroyed, with the
  estimated change in monthly cost. *Apply* runs only the plan you saw; when anything would be destroyed or replaced you type the project's
  name first. The state file stays in the project's `tofu/` folder and is backed up before every apply. Guards: the credentials must point
  at the account the project was first planned against; an apply above the project's monthly budget needs an extra tick. *Destroy* is a
  separate plan behind the typed project name. Blocks show their AWS ids once created; outputs (IPs, ids) are listed.
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
  ec2 web_1 t4g.micro in public_a sg web key my_key public
  sg web in main

  cidr internet 0.0.0.0/0

  rule internet -> web tcp 443
  ```

- **Costs, with the cheapest option first:** every block that costs money shows its monthly price (a NAT Gateway is about USD 41 a month
  in Mumbai, USD 33 in Virginia; a t4g.micro about USD 7; a public IPv4 address USD 3.65); the cheapest instance type is the default; the
  Costs tab adds everything up and says how to pay less: one shared NAT instead of one per AZ and what that costs in reliability, an unused
  NAT, VPC endpoints instead of a NAT, Graviton instead of x86, stopping instances outside working hours, dropping public addresses. VPCs,
  subnets, gateways, route tables and security groups are free, so a network costs nothing until something that runs is added.
- **Connection:** the credentials come from a profile in `~/.aws/config` / `~/.aws/credentials` (SSO profiles work after `aws sso login`)
  or from keys pasted into the app, which go into the system keychain and are handed to the AWS SDK one call at a time. "Test connection"
  shows the account. The read-only IAM policy to give the key is shown ready to paste.
- **Account → Import:** list the VPCs in a region and bring one onto the canvas: its subnets in their AZs, tiers worked out from the routes,
  NAT Gateways in their subnets, route tables with their routes, security groups with their rules as arrows, instances in their subnets,
  and the monthly cost it already incurs. Read-only; the project remembers which account and region it came from.
- **Spot, endpoints, S3, balancers, scaling:** instances and scaling groups can be Spot (about 70 % cheaper; the group replaces
  interrupted ones); VPC endpoints for S3/DynamoDB are free and let isolated subnets reach them without a NAT, interface endpoints
  (SSM, ECR, …) are priced per AZ; S3 buckets always have public access blocked and encryption on, with lifecycle rules to cheaper
  storage; an Application Load Balancer (two AZs checked, HTTPS with an ACM certificate) sends to an Auto Scaling group or an instance.
- **Templates:** a single public subnet, one web server (the cheapest thing that serves a website), a two-tier web app in 2 AZs (ALB +
  Spot Auto Scaling group), a three-tier VPC with a shared NAT and an S3 endpoint, the NN Builder GPU box (g4dn.xlarge Spot), and "Spot
  the mistakes".
- **Night mode** (⇧⌘N) is neon: dark panels, glowing outlines, each kind of block in its own colour.
- **Projects:** a folder with `project.json` and `design.json`, wherever you choose. A template opens unsaved until you save it.

## Roadmap

| Phase | Contents |
|---|---|
| 0 (done) | canvas, CIDR and routing checks, `.aib` text, templates; plus credentials, costs and VPC import pulled forward |
| 1 (done) | security groups as traffic arrows, EC2, Terraform/OpenTofu HCL generation, `tofu validate` in CI |
| 2 (done) | plan / apply / destroy with OpenTofu, the plan drawn on the canvas, state backups, account and budget guards |
| 3 (mostly done) | ALB, Auto Scaling, S3, VPC endpoints, Spot (done); RDS and reachability questions (next) |
| 4 | drift check, least-privilege deploy policy generated from the design, budget guard |

## Build

Rust (stable), Node 20+, and the Tauri 2 prerequisites for your system.

```sh
cd web && npm ci && npm run build && cd ..
cargo test -p aib-core
cargo test -p aib-core -- --ignored every_template_validates   # tofu init + validate on every template (needs OpenTofu and the internet)
cd app && npm ci && npx tauri build          # the bundle lands in target/release/bundle/
```

The settings folder is `~/Library/Application Support/AWS Infra Builder` on macOS (`AWS_INFRA_BUILDER_HOME` overrides it); projects go to
`~/AWS_Infra_Builder_Projects` unless you choose another folder.

## Licence

MIT.
