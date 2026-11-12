# AWS Infra Builder: the idea

Draw AWS infrastructure on a canvas (a region, a VPC, its subnets, gateways, route tables, security groups, instances, load
balancers, databases), have every part checked while it is drawn, and turn the finished design into real AWS resources with one
reviewed, reversible step.

It is NN Builder's idea applied to cloud networks. NN Builder's checker catches a wrong tensor shape at design time instead of
on the GPU. This app does the same for a wrong CIDR, a private subnet with no way out, or a database open to the internet,
catching them at design time instead of after `terraform apply` or a security incident.

Status: idea only (12 November 2026). Nothing is built yet.

---

## 1. Why

Building a VPC by hand in the AWS console takes about fifteen screens. They have to be filled in the right order, and nothing shows
the whole picture. Writing Terraform shows the whole text but not the picture, and mistakes appear only at `plan` or `apply`, or
never (a working but insecure design). Typical mistakes, all detectable before anything is created:

- subnet CIDRs that overlap, fall outside the VPC, or are too small (AWS keeps 5 addresses of every subnet);
- a "public" subnet without a route to an Internet Gateway, or a "private" one whose instances need the internet but have no NAT;
- a NAT Gateway placed in a private subnet;
- a load balancer or RDS subnet group in only one Availability Zone (AWS refuses it);
- a security group that opens SSH or a database port to `0.0.0.0/0`;
- two VPCs to be peered whose ranges overlap;
- a design that silently costs money: each NAT Gateway costs about USD 32 a month before any traffic, and idle Elastic IPs are
  charged too.

The app shows these on the block itself, in plain words, with what to change. That is how NN Builder works.

## 2. Principles (carried over from NN Builder)

1. **The design computer designs. AWS is touched only on an explicit Apply.** Drawing, checking, costing and code generation
   happen offline in the app's own engine. Reading an AWS account (import, drift) is read-only. Creating, changing or deleting
   anything needs a reviewed plan and a confirmation every time.
2. **Every number is shown and explained.** These are the AWS counterparts of NN Builder's shapes and parameters:
   - each subnet shows its CIDR, its usable addresses and its AZ;
   - each arrow shows what it carries (a route, or allowed ports);
   - each block shows its monthly cost.
   The inspector shows the arithmetic, for example `10.0.1.0/24 → 256 − 5 = 251 usable`.
3. **The canvas and a text language are two views of one design**, kept in step both ways, as with NN Builder's NNB text.
4. **The generated code is ordinary, readable infrastructure code**, which you can take and run without the app: Terraform/OpenTofu
   HCL, just as NN Builder writes a plain `train.py`.
5. **Never lose or destroy by surprise.** Before anything is replaced or deleted, the plan says so in red. Deletions need a typed
   confirmation, and deletion protection is on by default for databases.
6. **Secrets stay in the system keychain** and are never written into projects, code or logs.

## 3. What it builds

**Version 1: networking first, then the common services.**

| Group | Blocks |
|---|---|
| Scope | Account / Region, Availability Zone (lanes) |
| Network | VPC, Subnet (public / private / isolated), Internet Gateway, NAT Gateway, Elastic IP, Route table, Network ACL, VPC endpoint (S3 / DynamoDB gateway; interface endpoints for SSM, ECR, …), Egress-only IGW (IPv6) |
| Security | Security group, IAM role + instance profile (from a short list of policies), Key pair |
| Compute | EC2 instance, Auto Scaling group + launch template, Bastion (or none: SSM Session Manager) |
| Load balancing | Application / Network Load Balancer, target group, listener (HTTP/HTTPS with an ACM certificate) |
| Data | RDS (Postgres / MySQL) with its subnet group, S3 bucket (block public access on), EFS |
| DNS / edge | Route 53 record, CloudFront (later) |
| Joins | VPC peering, Transit Gateway (later), Site-to-Site VPN (later) |

Later: EKS (cluster + node groups in the private subnets), ECS/Fargate, Lambda in a VPC, ElastiCache, multi-account (Organizations).

**Not a goal:** the full AWS catalogue (hundreds of services), or replacing the console for day-to-day operations.

## 4. The canvas model

NN Builder's canvas is a graph of blocks and arrows. AWS needs **containment** as well. A subnet lives in one VPC and one AZ, and an
instance lives in a subnet. So the canvas has two kinds of relation:

- **Nesting (where it lives):** Region ⊃ VPC ⊃ AZ lane × subnet ⊃ instance / ENI / NAT / load-balancer node. A VPC is drawn as a
  large box. AZs are vertical lanes in it, and subnets are boxes where a tier row crosses an AZ lane, the way AWS architecture diagrams
  are usually drawn. Dropping an instance into a subnet places it there.
- **Arrows (what talks to what):**
  - **Route arrows:** subnet/route table → IGW, NAT, endpoint, peering, labelled with the destination (`0.0.0.0/0`, `pl-s3`,
    `10.1.0.0/16`).
  - **Traffic arrows:** security group → security group (or CIDR), labelled with the ports (`tcp 5432`). Drawing "web → db : 5432"
    creates the SG rule pair (egress on web, ingress on db referencing web's group). This is easier to read than rule tables, which
    remain available in the inspector.
  - **Attachment arrows:** target group → instances / ASG, ALB → target group.

**Tiers and AZs.** The layout is a grid: rows are tiers (public, app, data) and columns are AZs. Adding an AZ copies the row
pattern into the new column and gives each new subnet the next free CIDR. **Spread across AZs** turns a single-AZ design into a
2- or 3-AZ one: one NAT per AZ, or a single shared NAT to save money, as you choose.

## 5. The checks (the counterpart of NN Builder's shape checks)

Red means it cannot be created (AWS would refuse it, or the design contradicts itself). Yellow means it works but is probably not
what you want. Each message says what to change.

**Address arithmetic (like NN Builder's output-size formula):**
- the VPC CIDR is between /16 and /28, from RFC 1918 (or a warning when it is public space);
- a subnet lies inside its VPC (or inside an added secondary CIDR), does not overlap another subnet, and is /16–/28;
- usable addresses = 2^(32−prefix) − 5, shown; a warning when an ASG's maximum size, EKS pods or ENIs would not fit;
- overlapping ranges between peered or VPN-joined networks (a red error on the peering arrow);
- **Next free CIDR**: the inspector proposes ranges that fit (a /16 VPC as /20 blocks per tier and AZ, for example).

**Routing and reachability:**
- a subnet marked public has a `0.0.0.0/0 → IGW` route; a private subnet whose instances need the internet has `0.0.0.0/0 → NAT`
  (or a warning that only VPC endpoints are reachable);
- a NAT Gateway sits in a public subnet and has an Elastic IP; one NAT per AZ or an "AZ failure stops egress" warning;
- **Reachability paths**, answered in the inspector by following routes, NACLs and SGs: "Can the internet reach this database?",
  "Can the app reach S3 without the NAT?", "Can I SSH to this instance, and how?" The answer is shown as a highlighted path on
  the canvas. (AWS Reachability Analyzer does this after creation; here it runs before.)

**Service rules AWS enforces:**
- ALB: at least two subnets in two AZs, public for internet-facing; RDS subnet group: two AZs; Multi-AZ RDS needs both;
- the instance type exists in that region and AZ (from a bundled offering table, refreshed when online);
- quotas that bite early: 5 VPCs, 5 Elastic IPs, 200 route tables per region by default, shown against the design and, when
  connected, against the account's real quota.

**Security warnings:**
- SSH/RDP/database ports open to `0.0.0.0/0`;
- a public IP on an instance in a private tier;
- S3 without Block Public Access;
- RDS publicly accessible, unencrypted, or without deletion protection;
- IAM roles with `*` actions;
- missing flow logs (optional, off by default).

## 6. Numbers in the inspector

- **Per subnet:** CIDR, usable IPs, AZ, route table, NACL, what lives in it.
- **Per block: monthly cost**, from the AWS Price List API, cached offline. Each NAT Gateway is about USD 32 a month plus data. The
  footer gives the total per month, as NN Builder's top bar gives parameters. Every cost is labelled an *estimate*, with its
  assumptions (hours per month, GB transferred).
- **Per arrow:** the routes or ports it stands for.
- **The whole design:** resources by type, total estimated cost, AZ spread, and the problems list.

## 7. The text language

The same idea as NNB: one line per resource, labels, `<-` for "lives in / attached to", `let`, `for`, functions. A sketch:

```
net "Two-tier web app"
region ap-south-1
let azs = 2

1 vpc 10.0.0.0/16
2 for az in 1..azs {
    pub subnet public  /20 az az          # 10.0.0.0/20, 10.0.16.0/20 (next free)
    app subnet private /20 az az
    db  subnet isolated /24 az az
    nat nat_gateway in pub                # one NAT per AZ
  }
3 igw <- 1
4 sg web: allow tcp 443 from 0.0.0.0/0
5 sg app: allow tcp 8080 from web
6 sg db:  allow tcp 5432 from app
7 alb internet in 2.*.pub sg web -> tg app_tg port 8080
8 asg app_tg launch t4g.small min 2 max 4 in 2.*.app sg app
9 rds postgres db.t4g.micro multi_az in 2.*.db sg db
```

Typing changes the canvas once the text is valid, and moving blocks keeps the text, exactly as in NN Builder's Text tab. A
design saves as `design.json`, and as `.aib` text for sharing in a pull request or a document.

## 8. From design to real AWS

**Decision: generate Terraform/OpenTofu HCL and run `plan` / `apply` with the bundled OpenTofu binary**, rather than calling the
AWS SDK directly or using CloudFormation/CDK.

| Option | For | Against |
|---|---|---|
| **OpenTofu / Terraform HCL** (chosen) | readable, standard, works without the app, mature AWS provider, `plan` gives an exact diff, state handles updates and deletes | needs a state file (kept per project, optionally in S3 with locking) |
| CloudFormation / CDK | AWS-native, rollback built in, state kept by AWS | slower feedback, YAML/TypeScript less readable to most, stack drift handling is clumsy |
| Direct SDK calls | no extra tool | the app would have to do dependency order, updates, deletes and drift itself: re-implementing Terraform, with more risk |

**The flow:**

1. **Code tab:** the generated `main.tf`, `variables.tf` and `outputs.tf`, one module per tier, with comments that point back to the
   blocks. Copy or export them.
2. **Plan:** `tofu plan` runs with the chosen AWS profile, read-only. The plan is **drawn on the canvas**: green = will be created,
   yellow = changed in place, orange = replaced, red = destroyed. A list shows each change with the cost difference per month.
3. **Apply:** only after the plan is shown, with a summary ("create 23, change 0, destroy 0, +USD 71/month"). It needs a click,
   and also a typed confirmation when anything is destroyed or replaced. The log streams in, and each block turns solid when AWS
   has created it.
4. **Outputs:** the ALB's DNS name, the instance IDs, the RDS endpoint, shown on the blocks.
5. **Destroy:** a separate action, behind a typed project name. Protected resources (RDS with deletion protection, non-empty S3) are
   listed and kept unless you explicitly unprotect them.

**State:** `terraform.tfstate` lives in the project folder by default, backed up before every apply. Optionally it is moved to an S3
bucket with DynamoDB locking, which the app can create with one plan of its own. Every resource gets the tags
`managed-by = aws-infra-builder`, `project = <name>` and `design-block = <label>`, so the account shows what came from where.

## 9. Import and drift

- **Import (read-only):** choose a region and a VPC. The app calls only `Describe*` and `List*` APIs and draws what is there:
  subnets in their AZ lanes, routes as arrows, SG rules as traffic arrows. It then writes the matching HCL with `import` blocks, so
  an existing hand-made VPC can be brought under the app without recreating anything.
- **Drift:** **Check against AWS** runs `tofu plan -refresh-only` and marks blocks that someone changed in the console (an
  SG rule added by hand, for example), with the difference and two choices: take AWS's version into the design, or put the
  design's version back on the next apply.

## 10. Credentials and safety

- **AWS access:** an SSO profile (`aws sso login`) or an access key, kept in the keychain and handed only to the OpenTofu process
  through its environment. They are never written into the project, the HCL or the logs.
- **Least privilege:** the Connection tab shows two IAM policies to create. A **read-only** one covers import, plan, drift and
  pricing. A **deploy** one covers only the services the design uses, generated from the design. A deploy policy for a design
  without RDS has no `rds:*`.
- **Account guard:** each project records the account ID and region it belongs to. Plan and apply refuse to run when the
  credentials point at a different account, which stops a dev design from being applied to a production account.
- **Budget guard:** an optional monthly limit per project. An apply whose estimate exceeds it needs an extra confirmation, and
  the app can create an AWS Budget alarm with the same number.
- **No surprise costs:** idle-cost items (NAT, Elastic IPs, interface endpoints, load balancers) are labelled on the canvas, and a
  **Tear down for the night** action can destroy only those (stop instances, remove NAT) and recreate them later.
- **Never in the app:** root credentials, typing passwords, or creating IAM users with console access.

## 11. Screens

The same layout as NN Builder: a top bar with a project button, tabs, a summary, and Connection on the right.

| Tab | What it does |
|---|---|
| **Design** | palette (Network, Security, Compute, Load balancing, Data, Joins) · canvas with the VPC/AZ/tier grid · inspector (settings, CIDR arithmetic, cost, reachability questions, problems) |
| **Text** | the design as `.aib` text, in step with the canvas |
| **Code** | the generated HCL, Copy / Export |
| **Plan & Apply** | plan drawn on the canvas, change list with costs, Apply / Destroy, live log, outputs |
| **Costs** | monthly estimate per block, per tier, per AZ; what-if (single NAT vs one per AZ, on-demand vs Savings Plan) |
| **Account** | import a VPC, drift check, tags, state location |
| **Connection** | AWS profiles / keys, the two IAM policies to paste, OpenTofu version |

**Templates (like NN Builder's demos):**
- a single public subnet with one instance (learning);
- a two-tier web app (ALB + ASG + RDS, 2 AZs);
- a three-tier app with private app subnets and VPC endpoints;
- an EKS-ready VPC (subnet tags, /19 private subnets for pods);
- a bastion-less private fleet (SSM endpoints, no SSH);
- **an NN Builder GPU box**: a g4dn spot instance in a public subnet with an SG allowing SSH only from your IP, ready to add as a
  machine in NN Builder's Connection tab, with an export of its host, user and key file.

## 12. How it would be built

The same stack and habits as NN Builder, so the code and the build steps look familiar:

- `crates/core` (Rust):
  - the design model (blocks, nesting, arrows);
  - `cidr` (parse, contains, overlap, next free; property-tested);
  - `checks` (every rule in §5, with messages);
  - `reach` (route, NACL and SG path finding);
  - `pricing` (cached price list);
  - `hcl` (code generation);
  - `dsl` (the text language, sync both ways);
  - `import` (describe output → design).
- `app/` (Tauri 2): runs the bundled `tofu` binary and the AWS read calls, keychain, menus, file dialogs.
- `web/` (React + React Flow, with nested group nodes for VPC / AZ / subnet).

**Tests:**
- CIDR and reachability unit and property tests;
- every template and random designs generate HCL that passes `tofu validate`;
- `tofu plan` against **LocalStack** in CI (no AWS account needed);
- an opt-in **real-account smoke test** in a sandbox account with a USD 5 budget, which creates the single-subnet template,
  checks reachability with a real `curl`, then destroys everything.

**Rules shared with the workspace:**
- public repository (MIT);
- no personal data, account IDs or keys in tracked files;
- `*.local.md` for the owner's notes;
- program and data separate (designs and state in project folders the user chooses);
- update CLAUDE.md, commit and push, regenerate CHANGELOG, and build and install after every change.

## 13. Roadmap

| Phase | Contents | Done when |
|---|---|---|
| 0 | Project skeleton (copy NN Builder's Tauri/React/Rust layout), design model, VPC + subnets + IGW + route tables on the canvas, CIDR checks, `.aib` text | a 2-AZ VPC can be drawn and typed, with every CIDR error caught |
| 1 | NAT, SGs as traffic arrows, EC2, HCL generation, `tofu validate` in CI | generated HCL for all network templates validates |
| 2 | Plan/apply/destroy with the bundled OpenTofu, plan drawn on the canvas, state backups, account guard | the single-subnet template is created and destroyed in a sandbox account |
| 3 | ALB, ASG, RDS, S3, endpoints; reachability questions; cost estimates | the two-tier template is deployed and reachable through the ALB |
| 4 | Import a VPC, drift check, least-privilege policy generator, budget guard | an existing hand-made VPC is imported and managed without recreation |
| 5 | EKS, peering / Transit Gateway, multi-region, NN Builder GPU-box export | — |

## 14. Open questions

- **OpenTofu or Terraform** as the bundled binary (licence: OpenTofu is MPL, Terraform BSL). OpenTofu is the default, and HCL from
  either runs on both.
- **How far the containment layout should be automatic**: free placement, or always the strict AZ × tier grid. The proposal is
  the grid, with free placement inside a subnet.
- **Pricing accuracy:** data-transfer costs depend on traffic the design cannot know. Show them as "+ data" with the per-GB price,
  or let the user enter expected GB per month.
- **Multi-account / Organizations and SCPs:** out of scope until phase 5.
- **Whether import should handle resources the palette does not have:** draw them as grey "unmanaged" blocks, read-only.
