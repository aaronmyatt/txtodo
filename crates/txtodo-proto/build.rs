//! Regenerates `src/generated/txtodo.v1.rs` from `proto/` when built with `--features regen`.
//! Otherwise a no-op: the generated file is committed (constitution §6, generated artifact) so
//! ordinary builds and CI need no protoc. Ref: https://docs.rs/tonic-prost-build

fn main() {
    println!("cargo:rerun-if-changed=proto/txtodo/v1/txtodo.proto");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "regen")]
    regen();
}

#[cfg(feature = "regen")]
fn regen() {
    let out = std::path::Path::new("src/generated");
    assert!(out.is_dir(), "src/generated must exist");
    tonic_prost_build::configure()
        .out_dir(out)
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/txtodo/v1/txtodo.proto"], &["proto"])
        .unwrap_or_else(|e| panic!("protoc failed: {e}"));
    assert!(
        out.join("txtodo.v1.rs").is_file(),
        "codegen wrote the expected file"
    );
}
