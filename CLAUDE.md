# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Version 0.7.0 (13 November 2026): phases 0, 1, 2 and most of 3 of the roadmap in `IDEA.md` (the full design; read it before changing the
model), plus credentials, import and costs pulled forward. Phase 3 so far: Spot instances and root volume size, the NN Builder GPU box
template, VPC endpoints (gateway free, interface per AZ), S3 buckets (public access blocked, encryption, lifecycle), Application Load
Balancer + Auto Scaling group with target arrows. 0.5.1: the layout goes innermost block first with every block's real size
(nothing overlaps; `layout.rs`) and the Tidy-up recommendations (merge groups with the same rules, fold a group into the one
whose rules contain its own, combine the launch-wizard groups, delete unused groups and ranges; `tidy.rs`). 0.5.2: Fix buttons
(`fix.rs`): every problem the checks can repair has one, "Fix all safe ones" runs those that cost and open nothing; "allow SSH only
from my address" looks the owner's address up at checkip.amazonaws.com. 0.6.0: the Terminal tab (`hosts.rs`, `term.rs`,
`TerminalTab.tsx`): SSH into the design's instances from the app, like Termius: the system `ssh` in a pseudo-terminal drawn
with xterm.js, one host per instance (user from the OS, ~/.ssh/<key pair>.pem, the address from the import or the apply outputs),
saved hosts in settings.json, "Connect (SSH)" on an instance block. The app never reads a key; ssh does. 0.6.1: keys: the
key files of ~/.ssh offered (by name only), "Move it into ~/.ssh (mode 600)" for a key chosen elsewhere, a new key pair made in
AWS (ec2:CreateKeyPair, free: the one write outside Apply) with its key written once to ~/.ssh/<name>.pem, the account's key
pairs listed with whether the file is here; a Fix moves a NAT Gateway out of a private subnet (the "Spot the mistakes"
template is now fully repaired by "Fix all safe ones"). 0.7.0: adding an instance asks what it must have (vCPUs, memory,
GPU or not, OS, hours a month) and offers the types that fit, cheapest first (`pricing::recommend`, `NewInstance.tsx`;
"Recommend a type…" on an existing block); every instance and scaling group has `hours` (default 730) and its estimate follows
them instead of a full month; the type table gained the xlarge/2xlarge sizes and the g5 GPU family.
Still to come: RDS, reachability questions, import of balancers/groups. Phase 2 = Plan & Apply with OpenTofu (found on PATH or installed into the app's folder
from the official release, checksum-verified): plan is read-only and drawn on the canvas; apply runs only the shown plan, needs the
project's name typed when anything is destroyed or replaced, backs up the state first, and refuses when the credentials point at
another account (account guard) or the estimate exceeds the project's budget without a tick (budget guard). The app draws VPCs, subnets, Internet/NAT Gateways, route tables, security groups with
traffic arrows, EC2 instances and address ranges; checks CIDRs, routing, exposure and placement while drawing; keeps the design in step
with `.aib` text; generates OpenTofu/Terraform HCL (validated with `tofu validate` for every template, locally and in CI); shows what
the design costs a month and how to pay less; takes AWS credentials from `~/.aws` profiles or pasted keys (keychain); imports an existing
VPC with its groups, rules and instances read-only. Night mode is neon. The app itself only reads AWS (plus the free CreateKeyPair from the Terminal tab); OpenTofu changes it only on the
owner's Apply. **Never run apply or destroy in tests or on your own: it creates resources that cost money.** The opt-in real tests
only read (identity, import, plan). Still to come (`IDEA.md` §13): ALB/ASG/RDS/S3/endpoints, reachability, drift, remote state.

## Layout

```
Cargo.toml                 workspace: crates/core, app/src-tauri (version in [workspace.package], app/src-tauri/tauri.conf.json, web/package.json)
crates/core/src/           aib-core: no UI, no AWS calls
  model.rs                 Design {name, region, azs, nodes, edges, text}; Node {id, kind, label, params, parent, x, y, w, h} (positions relative to
                           the parent); Edge {kind "attach" | "route" | "traffic" | "target", params.dest | proto + ports | port}; KINDS (vpc,
                           subnet, igw, nat, rt, sg, ec2, cidr, vpce, s3, alb, asg) with the kind each lives in ("" = the region); REGIONS with AZ
                           counts; PORTS by name; ENDPOINT_SERVICES (gateway | interface); ec2/asg params type, os, key, public_ip, sgs, spot, disk
                           (+ asg: tier, min, max, desired), hours (1..=744, 730 = always on; the price follows it); alb params scheme, tier, sg, port, cert, health; s3 params gb, versioning, lifecycle,
                           expire_days; vpce params service, tier, sg;
                           INSTANCE_TYPES (vCPU, GiB, arch, USD/h us-east-1 and ap-south-1; t2 and t3.nano included for imports; xlarge/2xlarge
                           sizes and the g4dn/g5 GPU types for the recommendations), is_gpu(type); OS (AMI owner + name pattern); parse_ports,
                           traffic_label; add() (next free CIDR, balanced AZ; ec2 defaults to t4g.micro, no public IP), connect() (edge_kind
                           decides; a traffic arrow = the target's ingress and the source's egress), remove() (with contents), ensure_labels(),
                           arrange() = layout::arrange
  layout.rs                the layout, innermost block first: estimate() guesses every block's drawn size from the text it shows (the
                           canvas sends the measured sizes with Arrange and Tidy; they win); arrange_with(design, sizes): a subnet's
                           contents in a near-square grid and the subnet grows around them; per VPC the gateways/endpoints/balancers on
                           top, the subnet grid (AZ columns × tier rows, a column as wide as its widest subnet, a row as tall as its
                           tallest), then route tables, then groups, each wrapped at max(grid width, 5 blocks); the VPC grows around
                           it all; address ranges in a column left of the VPCs. no_overlap() is the test's check
  tidy.rs                  Recommendation {id, kind delete|merge|combine, title, detail, keep, drop, label, widens}; recommend(design):
                           unused groups (not "default": AWS keeps it) and unused ranges -> delete; used groups with the same rules
                           (ingress keys "<cidr or group>|proto|ports" + egress) -> merge, nothing opens; a group whose rules are part of
                           another's -> fold in, `widens` lists what its members would newly let in; the launch-wizard groups of a VPC
                           -> combine into one "servers" (the union; widens per group). apply(design, rec): members' sgs/sg switch to
                           the kept group, arrows move over once each, egress "all" if any folded group had it, the rest deleted.
                           Analysis.recommendations carries the list; the app re-finds a recommendation by id before applying
  fix.rs                   Fix {id "<kind>:<target>", kind, target (block or arrow), target_label, title (the button), note, safe, needs_ip};
                           fixes(design) mirrors the conditions of checks.rs: network_cidr, propose_cidr (next free block), route_to_igw
                           (reuse a public table of the VPC or add one, add the route), tier (mark public/private), delete (extra IGW,
                           unused range, unused group; "default" only from the design), egress_rules, my_ip (a risky port open to
                           0.0.0.0/0 -> the arrow's source becomes an Address range "my_ip" <ip>/32), type (nearest_type: same size in
                           a family of the same letter, else t3.micro; GPU families -> g4dn.xlarge), graviton, public_ip_off/on,
                           asg_sizes, lifecycle, nat_move (a NAT in a private subnet -> the VPC's public subnet, same AZ first; else
                           the subnet's tier fix). apply(design, fix, my_ip); apply_all_safe() re-lists after each. Analysis.fixes
  hosts.rs                 Host {id, label, host, user, port, key (path, "~" allowed), block, aws_id, note}; guess_user(os) (al2023 ec2-user,
                           ubuntu ubuntu, debian admin), default_key_path(key pair) = ~/.ssh/<name>.pem, suggest(design) = one Host per
                           ec2 block (address from params.address, else a note why it cannot be reached yet), ssh_args(host, key path),
                           check(host) (plain-words problems before ssh starts), saved()/save() = settings.json "hosts". No key is read.
                           ssh_dir(), list_keys(dir) = KeyFile {path, name, kind pem|openssh|""} told apart by name (*.pem, *.key, a
                           file with its .pub, id_*), key_pair_name_problem(name)
  cidr.rs                  Cidr: parse, contains, overlaps, usable (size − 5), is_private, next_free (aligned); random-input test
  checks.rs                analyse(&Design) -> Analysis {nodes: NodeResult{status, messages, info, summary}, edges, problems, ok, counts, az_spread,
                           az_names, cost}. Every rule has a message that says what to change. Non-network CIDRs are errors (AWS and the provider
                           refuse them). Security: ssh/rdp/database ports open to 0.0.0.0/0 (warn), groups nobody uses or nothing reaches, rules
                           across VPCs (error), egress "all" with arrows to a range. Instances: unknown type (error), public IP in a private tier,
                           no group, SSH in without a key pair, the Graviton type that would save money. Tests build designs from .aib text.
  dsl.rs                   the .aib text: parse(text, base) keeps ids and places of blocks whose label stays (new/moved blocks are laid out,
                           containers grow); lines vpc, subnet, igw, nat, rt, sg [egress rules], ec2 <type> [sg a,b] [key k] [public] [os x],
                           cidr <range>, rule <from> -> <to> <proto> [ports]; print(design) in a stable order (per VPC: igw, subnets tier-major,
                           nats, instances, route tables by the tier they serve, groups; then address ranges; then rules); sync() = ensure_labels
                           + print into design.text; help() for the Text tab. Also: `spot`, `disk n`, `hours n` on ec2/asg (printed when not 730); `vpce <svc> in <vpc> [tier] [sg]`;
                           `s3 <name> [gb] [versioning] [lifecycle] [expire]`; `alb <name> internet|internal in <vpc> [tier] [sg] [port] [cert]
                           [health]`; `asg <name> <type> in <vpc> [tier] [min] [max] [desired] [sg] …`; `target <alb> -> <asg|ec2> <port>`. Only an
                           `rt` line has routes after `:` (ARNs hold colons).
  hcl.rs                   generate(&Design) -> Files {main.tf, variables.tf, outputs.tf} for the AWS provider 5: aws_vpc, aws_subnet,
                           aws_internet_gateway, aws_eip + aws_nat_gateway, aws_route_table/route/association, aws_security_group + one
                           aws_vpc_security_group_ingress_rule per arrow in (and egress rules, or the allow-all written down), data aws_ami per
                           OS × arch, aws_instance (gp3 root, IMDSv2); tags Name, design-block (+ managed-by/project via default_tags); a design
                           with errors gets them as a header. write_to(dir). Test `every_template_validates_with_opentofu` is #[ignore]
                           (needs tofu + the internet): `cargo test -p aib-core -- --ignored`; examples/dump_hcl.rs writes the templates for CI.
  templates.rs             TEMPLATES as .aib text (single_public, web_server, two_tier, three_tier, broken); build(key). The test checks
                           print(parse(t)) == t, so a template's text must be in print order
  plan.rs                  parse_plan(design, `tofu show -json`) -> PlanSummary {creates, updates, replaces, destroys, changes, by_block
                           (block id -> strongest action, by the design-block tag or the resource name), cost_delta (from pricing), summary};
                           parse_apply_line(design, one `-json` line) -> ApplyEvent {kind, message, block_id, aws_id, error}; parse_outputs.
                           Tested with hand-written JSON.
  store.rs                 settings folder (AWS_INFRA_BUILDER_HOME, else ~/Library/Application Support/AWS Infra Builder), settings.json,
                           projects (project.json + design.json; an imported project's project.json has "aws": {account, region, vpc_id}),
                           the one unsaved draft (<home>/draft) for templates and imports, save_as, listing (canonical paths)
  pricing.rs               PRICES per region (NAT Gateway USD/h, per GB, idle EIP), instance_hourly (table for us-east-1 / ap-south-1, others
                           × 1.1), PUBLIC_IP_HOURLY, ROOT_VOLUME_MONTHLY, arm_equivalent, hours_of(node) (its `hours`, else 730: instances and
                           scaling groups are charged for those hours; the NAT and balancers for the month), recommend(vcpu, ram, gpu, region,
                           hours) -> Candidate {type, vcpu, memory, arch, gpu, hourly, monthly at those hours, note} cheapest first (at least the
                           vCPUs and memory asked; GPU types only when asked); cost(&Design) -> CostReport {items, total, advice,
                           assumptions}: NAT Gateways and instances (+ root volume + public IPv4) cost; advice = one shared NAT, unused NAT,
                           endpoints instead of NAT, tear down at night, stop instances outside working hours, Graviton instead of x86, drop
                           public addresses. Analysis.cost carries it; blocks show their price.
  creds.rs                 profiles from ~/.aws/config + credentials (names, kind keys|sso|role|process, region, key hint; never a secret;
                           AWS_INFRA_BUILDER_AWS_DIR overrides the folder), Source {none | profile name | keys}, check_keys, read_only_policy()
  import.rs                Inventory {vpc, subnets, igws, nats, route_tables with routes + associations, sgs with ingress/egress PermInfo,
                           instances} -> Imported {design, notes}: tiers from the routes (0.0.0.0/0 -> igw public, -> nat private, else
                           isolated), AZ letters from the AZ names, NATs in their subnet, main table drawn when a subnet uses it implicitly,
                           groups (egress "all" when AWS's allow-all is the only egress rule), one cidr block per range, a traffic arrow per
                           permission (a mirrored pair once), instances with the OS guessed from the AMI name and their addresses
                           (params address = public IPv4 or DNS, private_ip; for the Terminal tab); what was left out is noted.
                           Tested with a hand-built inventory; the AWS calls are in the app.
app/src-tauri/src/term.rs  terminal sessions: portable-pty runs the system `ssh` (found on PATH, /usr/bin, /opt/homebrew/bin) in a
                           pseudo-terminal; a reader thread emits `term-data` {id, data (base64)} and `term-exit` {id, code} events;
                           Terms {open, write, resize, close, open_ids}. expand_home("~/…"), key_problem(path) (missing, a folder, or
                           readable by others: ssh refuses those), fix_key_permissions (mode 600), ensure_dir (700), move_key_into(path,
                           dir) (rename, else copy + remove; never overwrites; 600), write_new_key(dir, name, material) (create_new,
                           0600). Tests: the key problems, the moves, and `sh -c echo` through a pty (no Tauri handle needed)
app/src-tauri/src/tofu.rs  OpenTofu: locate() (settings tofu_path, <home>/bin/tofu, PATH, /opt/homebrew/bin; terraform too), install()
                           (official release zip for the OS/arch, SHA-256 checked against the published sums, unzipped into <home>/bin),
                           Jobs (one background job: JobSnap {state, kind, log, plan, outputs, marks block->creating/created/failed/
                           destroyed, aws_ids}), plan() (writes the HCL into <project>/tofu, init once with TF_PLUGIN_CACHE_DIR=<home>/
                           plugin-cache, plan -out=plan.tfplan [-destroy], show -json -> PlanSummary), apply() (refuses without a plan,
                           backs up terraform.tfstate to tofu/backups/<timestamp>, apply -json streamed, outputs), state_info().
                           Credentials only in the child's environment (AWS_PROFILE, or the three key variables). One `#[ignore]` real
                           test plans the web_server template read-only (12 to add) with the default profile.
app/src-tauri/src/aws.rs   the AWS SDK for Rust (aws-config, aws-sdk-sts, aws-sdk-ec2): Creds {Profile | Keys}, identity(), list_vpcs(), key_pairs(),
                           create_key_pair() (the one write outside Apply: a key pair is free),
                           inventory() (VPC, subnets, IGWs, NATs, route tables, security groups, instances, AMI names) with a 60 s timeout
                           and readable errors. Only Describe* and GetCallerIdentity. One real test (`#[ignore]`, AWS_INFRA_BUILDER_REAL=1 with
                           the default profile) prints the account's first VPC as .aib.
app/src-tauri/src/main.rs  Tauri 2 commands (app_state, project_*, project_load, save_design, design_from_text, design_arrange(sizes), design_tidy(id, sizes), design_fix(id | all_safe, sizes; my_ip() via ureq when the fix needs it; lays out again when blocks came or went), design_add(kind, parent, params: the dialog's type/os/hours onto the new block), instance_recommend(vcpu, ram, gpu, hours, region),
                           design_connect, design_remove, text_help, code, export_code, pick_folder, open_url; tofu_status, tofu_install,
                           tofu_plan(destroy), tofu_apply(confirm, accept_budget) (guarded_ctx: live identity, account guard against
                           project.json "aws".account set by the first plan, region guard, budget guard), job_status(since),
                           project_set_budget, design_set_aws_ids (the created ids onto the blocks, and from the outputs the instances'
                           public/private addresses); hosts_state(design) {saved, suggested, ssh path}, hosts_save, key_check, key_fix,
                           pick_file, term_open(id, host, cols, rows) (hosts::check + key_problem first; returns the ssh arguments),
                           term_write, term_resize, term_close, term_list, keys_list, key_move, aws_key_pairs (DescribeKeyPairs +
                           whether ~/.ssh/<name>.pem exists), aws_create_key_pair(name) (CreateKeyPair ed25519 pem -> write_new_key);
                           aws_state, aws_set_keys, aws_clear_keys,
                           aws_select, aws_test, aws_list_vpcs, aws_import) and the menu (File/Edit/View/Design -> "menu" and "open-tab"
                           events). Every design command returns {design (synced), analysis}. Pasted keys live in the keychain (service
                           "AWS Infra Builder", items aws_key_id / aws_key_secret / aws_key_token), cached per launch; settings.json
                           keeps only aws_source, aws_region, aws_keys {hint, temporary} and aws_identity (account, arn).
web/src/                   React 19 + React Flow 12 (Vite, TypeScript)
  App.tsx                  top bar, tabs (Design, Text), Projects page (templates, new, list), Save-as modal, undo/redo snapshots, menu events
  Design.tsx               the canvas: "box" nodes (vpc, subnet; NodeResizer) and "block" nodes (igw, nat, rt, sg, ec2, cidr), nested by parentId;
                           drop re-parents by geometry (dropTarget); palette; inspector (label, params, Propose free CIDR, instance type with
                           price, OS, key pair, public IP, group ticks, egress mode, rule protocol/ports, facts, problems). With nothing
                           selected: the design's name/region/AZs, the summary, the cost, "Tidy up" (Analysis.recommendations with a
                           button each -> design_tidy, then the layout again), the problems and "Fixes" (Analysis.fixes, a Fix button
                           each and "Fix all safe ones" -> design_fix); a selected block or arrow shows its own fixes under its problems. Arrange (button, ⇧⌘A through the
                           "aib-arrange" event) sends the measured block sizes (rf.getNodes().measured). An instance block has
                           "Connect (SSH)" -> the "aib-connect" event -> the Terminal tab. Colours: COLORS by day,
                           neon(COLORS) at night through --c (useColor)
  NewInstance.tsx          the "New instance" dialog (and "Recommend a type…" on an instance): vCPUs, memory, GPU, OS, hours a month
                           (presets: always on 730, working hours 176, about 10 h a day 320, a few evenings 60, one day 8, custom) ->
                           instance_recommend -> the table of types that fit, cheapest first, priced at those hours; Use -> design_add with
                           {type, os, hours} (or the selected block's params). Adding an ec2 from the palette/menu opens it (Design.tsx add())
  CodeTab.tsx              main.tf / variables.tf / outputs.tf from `code`, Copy, Export… (pick a folder)
  PlanTab.tsx              OpenTofu status / Install, account guard and budget, Plan / Plan destroy, the change table, Apply or Destroy
                           (typed project name when dangerous, budget tick), the live log, outputs. App.tsx polls job_status (startJob)
                           and passes the job to Design.tsx: plan-<action> / mark-<state> classes and a ribbon on each block, a legend
  TextTab.tsx              the .aib text with a line gutter; Apply (⌘↩) -> design_from_text; errors by line
  CostsTab.tsx             the monthly estimate per block, the advice, the assumptions
  TerminalTab.tsx          the Termius part: hosts from the design (hosts_state.suggested) and saved hosts, a host form (label, address,
                           user, port, key file with Choose…, the ~/.ssh keys as a datalist and buttons, "Move it into ~/.ssh", the key
                           check / Fix permissions, "New key pair in AWS" and "List the key pairs in AWS"), sessions as tabs, each an xterm.js
                           Terminal + FitAddon in its own div (term-data bytes in, onData -> term_write, onResize -> term_resize). Always
                           mounted (display none when another tab shows) so sessions survive a tab switch; the "aib-connect" event
                           (Connect (SSH) on an instance block) opens the block's host, the saved copy first. Night theme = neon palette
  ConnectionTab.tsx        profiles (radio), pasted keys -> keychain, region, Test connection (identity), the read-only IAM policy
  AccountTab.tsx           List VPCs in a region, Import one -> new unsaved project, the import notes as a toast
  cidr.ts                  parse / nextFree for the inspector's proposal (the Rust side is the one that checks)
  api.ts, theme.ts (day/night + neon()), styles.css (day, then the neon night section)
.github/workflows/validate.yml  core tests, clippy, dump_hcl + tofu validate per template, web build
```

## Build, test, install (macOS)

- `source ~/.cargo/env` first (cargo is not on PATH in a fresh shell). Quit the app (Cmd-Q).
- `cd web && npm ci && npm run build`; `cargo test -p aib-core`; `cargo clippy -p aib-core -p aws-infra-builder-app` (no warnings);
  `cd app && npm ci && npx tauri build`; then, only if the bundle exists, replace `/Applications/AWS Infra Builder.app` with
  `target/release/bundle/macos/AWS Infra Builder.app` and delete the bundle output (one copy only).
- Look at the window with a scratch settings folder: `open -n --env AWS_INFRA_BUILDER_HOME=<scratch> "/Applications/AWS Infra Builder.app"`.
- Tests use `AWS_INFRA_BUILDER_HOME` in a temp dir and never touch the real settings or projects. OpenTofu: `brew install opentofu`
  (/opt/homebrew/bin/tofu); `cargo test -p aib-core -- --ignored every_template_validates` runs init + validate on every clean template
  (3–4 minutes: the provider is downloaded).

## Rules for changes

- A new block kind: `model::KINDS` (+ `defaults`, `edge_kind`, `arrange` placement), `checks.rs` (its rules), `pricing.rs` (its idle cost and the
  cheaper alternative), `dsl.rs` (parse the line and print it), `hcl.rs` (its resources), `import.rs` if AWS has it, `Design.tsx` (COLORS,
  palette, handles, inspector fields). A new template: `templates.rs` in print order; the template test checks it, and its HCL must validate.
- Cost rule from the owner (a student): no unnecessary cost, ever. Automatic and opt-in tests only read (identity, import, plan,
  validate); nothing applies. Templates default to the cheapest choice (t4g, Spot where safe, no public IP, gateway endpoints, one NAT).
- Plan & Apply: never make the app apply without the shown plan, the typed name for destroys/replaces and the guards; the state file is the
  owner's (back it up, never delete it); the project's `tofu/` folder is program output and may be regenerated, except `terraform.tfstate`
  and `backups/`.
- The Terminal tab: the app spawns the user's own `ssh` and never reads, copies or stores a private key (the path only, in
  settings.json); host keys are checked by ssh as usual (the first-connection question is answered in the terminal). No test
  connects anywhere; the pty test runs `sh -c echo`. Keys are listed by file name, never opened. A new key pair's material is
  written once to ~/.ssh/<name>.pem (mode 600, never overwriting) and kept nowhere else. ec2:CreateKeyPair is the one call
  outside Apply that changes the account; it costs nothing and the policy in the Connection tab names it.
- Night mode stays neon: a new screen or element gets its `[data-theme="night"]` rules (glow in --c or the cyan/magenta accents); day mode
  keeps its plain look.
- Cost minimisation is a design rule: every block that costs money while idle shows its monthly price on the block, in the Costs tab and in
  the summary, with the cheaper alternative and what it costs in return. Templates default to the cheapest design that does the job.
  An instance's price is for the hours it says it runs (`hours`, 730 by default); a new instance is chosen from what it must have,
  cheapest type first.
- No real AWS calls in the automatic tests; the one `#[ignore]` test runs only with AWS_INFRA_BUILDER_REAL=1 and only reads.
- Keep every check's message in plain words with what to change; red only for what AWS refuses or a contradiction, yellow otherwise.
- The design computer designs: no AWS calls outside an explicit, reviewed Apply (phase 2+); credentials only in the keychain (phase 2+).
- Public repository (MIT): no personal paths, account IDs or keys in tracked files; the owner's notes go in `CLAUDE.local.md` (ignored).
- Every change is committed (what and why), and after the tests pass it is pushed to `main` and installed (workspace `../CLAUDE.md`).
