//! AWS Infra Builder core: the design (blocks inside blocks, arrows between them), CIDR arithmetic, the checks, cost estimates, the
//! .aib text language, templates, project files, the credential sources and the import mapping. No UI and no AWS calls: the app
//! makes the (read-only) calls and hands the result here. The design computer designs; AWS is changed only on an explicit Apply
//! (later phases).

pub mod checks;
pub mod cidr;
pub mod creds;
pub mod dsl;
pub mod fix;
pub mod hcl;
pub mod hosts;
pub mod import;
pub mod layout;
pub mod model;
pub mod plan;
pub mod pricing;
pub mod store;
pub mod templates;
pub mod tidy;
