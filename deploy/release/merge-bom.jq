## merge-bom.jq — combine the per-crate CycloneDX JSON files `cargo cyclonedx --all` produces
## (one per workspace member: https://github.com/CycloneDX/cyclonedx-rust-cargo — the tool has no
## workspace-wide mode) into a single release bom.json: one metadata block from the first input,
## plus every input's own metadata.component (so every listed crate is itself a component, not
## only its dependencies) and a deduplicated union of every input's dependency components.
##
## Usage (see justfile's release recipe / RELEASE_CI.patch.md's release.yml sbom job):
##   jq -s -f deploy/release/merge-bom.jq crates/txtodo-cli/txtodo-cli.cdx.json ... > bom.json
##
## CycloneDX JSON schema: https://cyclonedx.org/docs/1.5/json/ · jq manual: https://jqlang.org/manual/
. as $all
| ([ $all[] | .metadata.component ]) as $selves
| ([ $all[] | .components // [] ] | add) as $deps
| $all[0]
| .components = (
    ($selves + $deps)
    | unique_by((.name // "") + "@" + (.version // "") + (.purl // ""))
    | sort_by(.name, .version)
  )
