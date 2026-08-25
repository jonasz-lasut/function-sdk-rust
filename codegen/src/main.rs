//! Regenerates the checked-in gRPC and serde code in `sdk/src/generated`.
//!
//! Maintainer tool: requires `protoc` on PATH. SDK users never run this
//! because the generated code is committed.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

const PROTO: &str = "sdk/proto/v1/run_function.proto";
const INCLUDE: &str = "sdk/proto";
const OUT: &str = "sdk/src/generated";

fn main() -> Result<(), Box<dyn Error>> {
    let root = workspace_root();
    let tmp = tempfile::tempdir()?;
    let descriptor = tmp.path().join("fileset.bin");

    let mut config = prost_build::Config::new();
    config
        .file_descriptor_set_path(&descriptor)
        .compile_well_known_types()
        .extern_path(".google.protobuf", "::pbjson_types");

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .out_dir(tmp.path())
        .compile_with_config(config, &[root.join(PROTO)], &[root.join(INCLUDE)])?;

    pbjson_build::Builder::new()
        .register_descriptors(&fs::read(&descriptor)?)?
        .out_dir(tmp.path())
        .build(&[".apiextensions"])?;

    let out = root.join(OUT);
    fs::create_dir_all(&out)?;
    for entry in fs::read_dir(tmp.path())? {
        let path = entry?.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let target = if name.ends_with(".serde.rs") {
            "v1.serde.rs"
        } else if name.ends_with(".rs") {
            "v1.rs"
        } else {
            continue;
        };
        fs::copy(&path, out.join(target))?;
        println!("wrote {}", out.join(target).display());
    }
    fs::copy(&descriptor, out.join("fileset.bin"))?;
    println!("wrote {}", out.join("fileset.bin").display());
    Ok(())
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("codegen crate lives one level below the workspace root")
        .to_path_buf()
}
