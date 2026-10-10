fn main() {
    println!("cargo:rerun-if-changed=src/fortran.lalrpop");
    println!("cargo:rerun-if-changed=build.rs");
    lalrpop::process_root().unwrap();
}
