# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Version 0.2.0 (12 November 2026): phase 0 of the roadmap in `IDEA.md` (the full design; read it before changing the model) plus,
pulled forward at the owner's request, credentials, import and costs. The app draws VPCs, subnets, Internet/NAT Gateways and route
tables, checks CIDRs and routing while drawing, keeps the design in step with `.aib` text, shows what the design costs a month and how
to pay less, takes AWS credentials from `~/.aws` profiles or pasted keys (keychain), and imports an existing VPC read-only. AWS is only
ever read (`sts:GetCallerIdentity`, `ec2:Describe*`). Still to come (`IDEA.md` §13): security groups, EC2, HCL generation, plan/apply.

## Layout

```
Cargo.toml                 workspace: crates/core, app/src-tauri (version in [workspace.package], app/src-tauri/tauri.conf.json, web/package.json)
crates/core/src/           aib-core: no UI, no AWS calls
  model.rs                 Design {name, region, azs, nodes, edges, text}; Node {id, kind, label, params, parent, x, y, w, h} (positions relative to
                           the parent); Edge {kind "attach" | "route", params.dest}; KINDS (vpc, subnet, igw, nat, rt) with the kind each lives in;
                           REGIONS with AZ counts; add() (next free CIDR, balanced AZ), connect() (edge_kind decides), remove() (with contents),
                           ensure_labels(), arrange() (AZ columns × tier rows per VPC), sort_parents_first()
  cidr.rs                  Cidr: parse, contains, overlaps, usable (size − 5), is_private, next_free (aligned); random-input test
  checks.rs                analyse(&Design) -> Analysis {nodes: NodeResult{status, messages, info, summary}, edges, problems, ok, counts, az_spread,
                           az_names}. Every rule has a message that says what to change. Tests build designs from .aib text.
  dsl.rs                   the .aib text: parse(text, base) keeps ids and places of blocks whose label stays (new/moved blocks are laid out,
                           containers grow); print(design) in a stable order (per VPC: igw, subnets tier-major, nats, route tables by the tier
                           they serve); sync() = ensure_labels + print into design.text; help() for the Text tab
  templates.rs             TEMPLATES as .aib text (single_public, two_tier, three_tier, broken); build(key). The test checks print(parse(t)) == t,
                           so a template's text must be in print order
  store.rs                 settings folder (AWS_INFRA_BUILDER_HOME, else ~/Library/Application Support/AWS Infra Builder), settings.json,
                           projects (project.json + design.json; an imported project's project.json has "aws": {account, region, vpc_id}),
                           the one unsaved draft (<home>/draft) for templates and imports, save_as, listing (canonical paths)
  pricing.rs               PRICES per region (NAT Gateway USD/h, per GB, idle EIP); cost(&Design) -> CostReport {items, total, advice, assumptions}:
                           only NAT Gateways cost while idle in this version; advice = one shared NAT, unused NAT, endpoints instead of NAT,
                           tear down at night. Analysis.cost carries it; the NAT block shows its region's price.
  creds.rs                 profiles from ~/.aws/config + credentials (names, kind keys|sso|role|process, region, key hint; never a secret;
                           AWS_INFRA_BUILDER_AWS_DIR overrides the folder), Source {none | profile name | keys}, check_keys, read_only_policy()
  import.rs                Inventory {vpc, subnets, igws, nats, route_tables with routes + associations} -> Imported {design, notes}: tiers
                           from the routes (0.0.0.0/0 -> igw public, -> nat private, else isolated), AZ letters from the AZ names, NATs in
                           their subnet, main table drawn when a subnet uses it implicitly, routes to things the canvas lacks noted.
                           Tested with a hand-built inventory; the AWS calls are in the app.
app/src-tauri/src/aws.rs   the AWS SDK for Rust (aws-config, aws-sdk-sts, aws-sdk-ec2): Creds {Profile | Keys}, identity(), list_vpcs(),
                           inventory() with a 60 s timeout and readable errors. Only Describe* and GetCallerIdentity. One real test
                           (`#[ignore]`, AWS_INFRA_BUILDER_REAL=1 with the default profile) prints the account's first VPC as .aib.
app/src-tauri/src/main.rs  Tauri 2 commands (app_state, project_*, project_load, save_design, design_from_text, design_arrange, design_add,
                           design_connect, design_remove, text_help, pick_folder, open_url; aws_state, aws_set_keys, aws_clear_keys,
                           aws_select, aws_test, aws_list_vpcs, aws_import) and the menu (File/Edit/View/Design -> "menu" and "open-tab"
                           events). Every design command returns {design (synced), analysis}. Pasted keys live in the keychain (service
                           "AWS Infra Builder", items aws_key_id / aws_key_secret / aws_key_token), cached per launch; settings.json
                           keeps only aws_source, aws_region, aws_keys {hint, temporary} and aws_identity (account, arn).
web/src/                   React 19 + React Flow 12 (Vite, TypeScript)
  App.tsx                  top bar, tabs (Design, Text), Projects page (templates, new, list), Save-as modal, undo/redo snapshots, menu events
  Design.tsx               the canvas: "box" nodes (vpc, subnet; NodeResizer) and "block" nodes (igw, nat, rt), nested by parentId; drop
                           re-parents by geometry (dropTarget); palette; inspector (label, params, Propose free CIDR, facts, problems)
  TextTab.tsx              the .aib text with a line gutter; Apply (⌘↩) -> design_from_text; errors by line
  CostsTab.tsx             the monthly estimate per block, the advice, the assumptions
  ConnectionTab.tsx        profiles (radio), pasted keys -> keychain, region, Test connection (identity), the read-only IAM policy
  AccountTab.tsx           List VPCs in a region, Import one -> new unsaved project, the import notes as a toast
  cidr.ts                  parse / nextFree for the inspector's proposal (the Rust side is the one that checks)
  api.ts, theme.ts, styles.css
```

## Build, test, install (macOS)

- `source ~/.cargo/env` first (cargo is not on PATH in a fresh shell). Quit the app (Cmd-Q).
- `cd web && npm ci && npm run build`; `cargo test -p aib-core`; `cargo clippy -p aib-core -p aws-infra-builder-app` (no warnings);
  `cd app && npm ci && npx tauri build`; then, only if the bundle exists, replace `/Applications/AWS Infra Builder.app` with
  `target/release/bundle/macos/AWS Infra Builder.app` and delete the bundle output (one copy only).
- Look at the window with a scratch settings folder: `open -n --env AWS_INFRA_BUILDER_HOME=<scratch> "/Applications/AWS Infra Builder.app"`.
- Tests use `AWS_INFRA_BUILDER_HOME` in a temp dir and never touch the real settings or projects.

## Rules for changes

- A new block kind: `model::KINDS` (+ `defaults`, `edge_kind`, `arrange` placement), `checks.rs` (its rules), `pricing.rs` (its idle cost and the
  cheaper alternative), `dsl.rs` (parse the line and print it), `import.rs` if AWS has it, `Design.tsx` (COLORS, palette, inspector fields).
  A new template: `templates.rs` in print order; the template test checks it.
- Cost minimisation is a design rule: every block that costs money while idle shows its monthly price on the block, in the Costs tab and in
  the summary, with the cheaper alternative and what it costs in return. Templates default to the cheapest design that does the job.
- No real AWS calls in the automatic tests; the one `#[ignore]` test runs only with AWS_INFRA_BUILDER_REAL=1 and only reads.
- Keep every check's message in plain words with what to change; red only for what AWS refuses or a contradiction, yellow otherwise.
- The design computer designs: no AWS calls outside an explicit, reviewed Apply (phase 2+); credentials only in the keychain (phase 2+).
- Public repository (MIT): no personal paths, account IDs or keys in tracked files; the owner's notes go in `CLAUDE.local.md` (ignored).
- Every change is committed (what and why), and after the tests pass it is pushed to `main` and installed (workspace `../CLAUDE.md`).
