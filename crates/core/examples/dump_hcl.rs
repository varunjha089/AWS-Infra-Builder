//! Write every template's HCL into a folder (one sub-folder per template), for `tofu validate` in CI:
//! `cargo run -p aib-core --example dump_hcl -- <folder>`. Templates whose design has errors are skipped and named.

fn main() {
    let dir = std::env::args().nth(1).map(std::path::PathBuf::from).expect("usage: dump_hcl <folder>");
    for t in aib_core::templates::TEMPLATES {
        let d = aib_core::templates::build(t.key).expect("template parses");
        if !aib_core::checks::analyse(&d).ok {
            println!("skipped {} (broken on purpose)", t.key);
            continue;
        }
        let out = dir.join(t.key);
        aib_core::hcl::write_to(&aib_core::hcl::generate(&d), &out).expect("write");
        println!("wrote {}", out.display());
    }
}
