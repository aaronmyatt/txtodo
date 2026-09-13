# deploy/nix/packages.nix — the three release binaries (plan M10): `txtodo` (crates/txtodo-cli),
# `txtodod` (crates/txtodo-daemon), `txtodo-tui` (crates/txtodo-tui). Package name follows each
# crate's [[bin]] name in Cargo.toml, not the crate directory name.
#
# Each call to `craneLib.buildPackage` below scopes cargo to one workspace member via
# `cargoExtraArgs = "-p <crate>"`; cargo then only builds that member's dependency subgraph, so
# the heavier workspace members (apps/desktop/src-tauri's webkit2gtk/libayatana stack, relay)
# never enter these three derivations even though they share one Cargo.lock. crane computes a
# separate dep-only pass per call automatically when no `cargoArtifacts` is supplied, so the three
# packages stay correctly isolated from one another too.
#
# crane workspace pattern: https://crane.dev/examples/quick-start.html
# cargo -p / workspace selection: https://doc.rust-lang.org/cargo/reference/workspaces.html
{ pkgs, craneLib }:
let
  # crane's own cleanCargoSource keeps only .rs/.toml/Cargo.lock (https://crane.dev/API.html#
  # cranelibcleancargosource) — too aggressive here: several crates `include_str!`/`include_bytes!`
  # a non-Rust asset at build time (not test-only — these are outside `#[cfg(test)]`), and
  # cleanCargoSource strips every one of them, breaking the build (confirmed locally — see the
  # worktree report):
  #   - txtodo-cli/src/commands/service.rs -> deploy/launchd/*.plist, deploy/systemd/*.service
  #   - txtodo-store/src/lib.rs           -> txtodo-store/migrations/*.sql
  #   - txtodo-store/src/registry.rs      -> txtodo-store/registry_migrations/*.sql
  #   - txtodo-sync/src/eff_wordlist.rs   -> txtodo-sync/src/wordlists/*.txt
  # Keep crane's own filter and additionally allow exactly those directories through.
  # lib.cleanSourceWith / lib.hasInfix: https://nixos.org/manual/nixpkgs/stable/#sec-functions-library-filesystem
  src = pkgs.lib.cleanSourceWith {
    src = pkgs.lib.cleanSource ../..;
    filter = path: type:
      craneLib.filterCargoSources path type
      || pkgs.lib.hasInfix "/deploy/launchd/" path
      || pkgs.lib.hasInfix "/deploy/systemd/" path
      || pkgs.lib.hasInfix "/migrations/" path
      || pkgs.lib.hasInfix "/registry_migrations/" path
      || pkgs.lib.hasInfix "/wordlists/" path;
    name = "txtodo-workspace-source";
  };

  commonArgs = {
    inherit src;
    strictDeps = true;
    doCheck = false; # `cargo test --workspace` is CI's job (ci.yml); this builds binaries only
  };

  mkBin = { pname, crate }:
    craneLib.buildPackage (commonArgs // {
      inherit pname;
      version = "0.0.0"; # workspace.package.version (Cargo.toml); a release tag overrides this externally
      cargoExtraArgs = "--locked -p ${crate}";
    });
in
{
  txtodo = mkBin { pname = "txtodo"; crate = "txtodo-cli"; };
  txtodod = mkBin { pname = "txtodod"; crate = "txtodo-daemon"; };
  txtodo-tui = mkBin { pname = "txtodo-tui"; crate = "txtodo-tui"; };
}
