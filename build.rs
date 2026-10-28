fn main() {
    println!("cargo:rerun-if-changed=src/allocation_identity.rs");
    println!("cargo:rerun-if-changed=src/float_model.rs");
    println!("cargo:rerun-if-changed=src/fortran.lalrpop");
    println!("cargo:rerun-if-changed=build.rs");
    lalrpop::process_root().unwrap();
    for file in ["src/native_format.rs", "src/c_descriptor.rs", "src/timing.rs", "src/calendar.rs", "src/environment.rs", "src/format_spec.rs", "src/record_input.rs", "src/list_input.rs", "src/namelist_input.rs", "src/namelist_native.rs", "src/character_input.rs", "src/file_inquiry.rs"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let out =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("rfort_format.lib");
    let status = std::process::Command::new(std::env::var_os("RUSTC").unwrap())
        .args([
            "--crate-name",
            "rfort_format",
            "--crate-type",
            "staticlib",
            "--edition=2021",
            "-C",
            "opt-level=2",
            "-C",
            "panic=abort",
            "-C",
            "debuginfo=0",
            "--target",
        ])
        .arg(std::env::var("TARGET").unwrap())
        .arg("src/native_format.rs")
        .arg("-o")
        .arg(out)
        .status()
        .unwrap();
    assert!(status.success(), "could not build formatting runtime");
}
