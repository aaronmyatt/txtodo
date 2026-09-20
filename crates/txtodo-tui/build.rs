//! Hands the release date to the crate as `TXTODO_RELEASE_DATE` (task version-info). The logic is
//! shared by every txtodo binary's build script: `build-support/buildinfo.rs`.
//! https://doc.rust-lang.org/cargo/reference/build-scripts.html

#[path = "../../build-support/buildinfo.rs"]
mod buildinfo;

fn main() {
    buildinfo::emit();
}
