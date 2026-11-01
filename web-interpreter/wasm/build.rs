use std::{env, path::PathBuf};
fn main() {
    println!("cargo::rustc-check-cfg=cfg(native_runtime)");
    println!("cargo:rerun-if-env-changed=RFORT_COMMIT");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..").canonicalize().unwrap();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let source = root.join("src");
    println!("cargo:rerun-if-changed={}", source.display());
    lalrpop::Configuration::new().set_in_dir(&source).set_out_dir(&out)
        .process_file(source.join("fortran.lalrpop")).unwrap();
    println!("cargo:rustc-env=RFORT_BROWSER_BUILD={}", env::var("RFORT_COMMIT").unwrap_or_else(|_|"development".into()));
}
