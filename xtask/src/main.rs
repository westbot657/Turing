use serde::Deserialize;
use std::path::Path;
use std::process::Command;
use std::{env, fs};

#[derive(Deserialize)]
struct CargoToml {
    package: Package,
    lib: Lib,
}

#[derive(Deserialize)]
struct Package {
    version: String,
}

#[derive(Deserialize)]
struct Lib {
    name: String,
}

fn main() {
    let mut args = env::args().skip(1);
    let task = args.next().unwrap_or_else(|| {
        eprintln!("No task provided, Available tasks: win-build, test-run");
        std::process::exit(1);
    });

    match task.as_str() {
        "win-build" | "w" => build_windows(),
        "test-run" | "t" => test_run(),
        unknown => {
            eprintln!("Unknown task: {}", unknown);
            std::process::exit(1);
        }
    }
}

fn compile_package(target: &str, crate_name: &str, mode: &str) {
    let cargo_bin = env::var("CARGO").unwrap_or("cargo".to_string());

    let mut status = Command::new(cargo_bin);
    if mode == "--debug" {
        status.args(["build", "--target", target, "-p", crate_name]);
    } else {
        status.args(["build", mode, "--target", target, "-p", crate_name]);
    }

    let status = status.status().expect("Failed to build Turing");

    if !status.success() {
        eprintln!("Failed to compile {} crate", crate_name);
        std::process::exit(1);
    }
}

fn build_windows() {
    let target = "x86_64-pc-windows-gnu";
    let crate_name = "turing";
    compile_package(target, crate_name, "--release");
    let raw_cargo = fs::read_to_string(format!("{}/Cargo.toml", crate_name))
        .expect("Failed to read Cargo.toml");
    let cargo: CargoToml = toml::from_str(&raw_cargo).expect("Failed to parse Cargo.toml");

    let version = cargo.package.version;
    let lib_name = cargo.lib.name;

    let built = format!("target/{}/release/{}.dll", target, lib_name);
    let output = Path::new("dist").join(format!("{}-{}.dll", lib_name, version));

    fs::create_dir_all("dist").expect("Failed to create dist directory");
    fs::copy(&built, &output)
        .unwrap_or_else(|e| panic!("Failed to copy DLL: {}.dll {}", lib_name, e));

    // copy to BEAT_SABER_DIR/Libs/Native/turing_rs.dll if BEAT_SABER_DIR is set
    if let Ok(beat_saber_dir) = env::var("BEAT_SABER_DIR") {
        let dest_dir = Path::new(&beat_saber_dir).join("Libs").join("Native");
        fs::create_dir_all(&dest_dir).expect("Failed to create Beat Saber Libs/Native directory");
        let dest_path = dest_dir.join(format!("{lib_name}.dll"));


        fs::copy(&built, &dest_path)
            .unwrap_or_else(|e| panic!("Failed to copy DLL to Beat Saber directory: {} {}", dest_path.display(), e));
        println!("Copied dll to Beat Saber directory: {}", dest_path.display());
    }

    println!("Windows dll generated in dist");
}

fn test_run() {
    compile_package("wasm32-wasip1", "wasm_tests", "--debug");

    let _ = fs::remove_file("tests/wasm/wasm_tests.wasm");
    fs::copy(
        "target/wasm32-wasip1/debug/wasm_tests.wasm",
        "tests/wasm/wasm_tests.wasm",
    )
    .unwrap_or_else(|e| panic!("Failed to copy wasm file for testing: {}", e));

    println!("Copied wasm test script to tests/wasm, running tests...");

    let cargo_bin = env::var("CARGO").unwrap_or("cargo".to_string());

    let status = Command::new(cargo_bin)
        .args(["test", "-p", "turing", "--", "--nocapture"])
        .status()
        .expect("Failed to run tests");

    if !status.success() {
        println!("Turing tests failed to run")
    }
}
