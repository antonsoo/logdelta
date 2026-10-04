use std::{env, fs, path::PathBuf};

fn main() {
    // Cargo has already resolved environment, target/cfg and build-config
    // precedence. Export its effective extra flags instead of reimplementing it.
    let flags = env::var("CARGO_ENCODED_RUSTFLAGS")
        .expect("Cargo must provide the effective encoded rustc flags");
    let directory = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"));
    fs::write(directory.join("logdelta-rustflags"), flags)
        .expect("could not record effective rustc flags");
    for key in ["RUSTC", "RUSTC_WRAPPER"] {
        fs::write(
            directory.join(format!("logdelta-{key}")),
            env::var(key).unwrap_or_default(),
        )
        .expect("could not record Cargo's compiler context");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=LOGDELTA_CAPTURE_TOKEN");
}
