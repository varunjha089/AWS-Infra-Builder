//! AWS Infra Builder core: the design (blocks inside blocks, arrows between them), CIDR arithmetic, the checks, the .aib text
//! language, templates and project files. No UI and no AWS calls: the design computer designs; AWS is touched only on an explicit
//! Apply (later phases).

pub mod checks;
pub mod cidr;
pub mod dsl;
pub mod model;
pub mod store;
pub mod templates;
