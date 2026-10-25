//! `PruneConfig` sources (task `peer-prune`): the environment beats the file, the file beats the
//! default, `0` is off, and an unreadable value falls through instead of switching a rule off.

use std::collections::BTreeMap;

use super::{config_path, resolve};
use crate::prune_stale::{DAY_MS, PruneConfig};

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |name| map.get(name).cloned()
}

#[test]
fn nothing_set_is_the_default() {
    assert_eq!(resolve(&env_of(&[]), None), PruneConfig::default());
}

#[test]
fn the_file_sets_either_rule_with_a_duration_or_integer_seconds() {
    let file = "prune_parked_after = \"12h\"\nprune_quiet_after = 0\nrelay_url = \"x\"\n";
    let cfg = resolve(&env_of(&[]), Some(file));
    assert_eq!(cfg.parked_after_ms, 12 * 3_600_000);
    assert_eq!(cfg.quiet_after_ms, 0, "the integer 0 is off");
}

#[test]
fn the_environment_beats_the_file() {
    let file = "prune_quiet_after = \"3d\"\n";
    let env = env_of(&[("TXTODO_PRUNE_QUIET_AFTER", "30d")]);
    assert_eq!(resolve(&env, Some(file)).quiet_after_ms, 30 * DAY_MS);
}

#[test]
fn an_unreadable_value_falls_through_to_the_next_source() {
    let file = "prune_quiet_after = \"3d\"\nprune_parked_after = \"soon\"\n";
    let env = env_of(&[("TXTODO_PRUNE_QUIET_AFTER", "whenever")]);
    let cfg = resolve(&env, Some(file));
    assert_eq!(cfg.quiet_after_ms, 3 * DAY_MS, "bad env, so the file's");
    assert_eq!(
        cfg.parked_after_ms,
        PruneConfig::default().parked_after_ms,
        "bad file value, so the default"
    );
    let negative = resolve(&env_of(&[]), Some("prune_quiet_after = -5\n"));
    assert_eq!(
        negative.quiet_after_ms,
        PruneConfig::default().quiet_after_ms
    );
}

#[test]
fn a_file_that_is_not_toml_is_ignored() {
    assert_eq!(resolve(&env_of(&[]), Some("= = =")), PruneConfig::default());
}

#[test]
fn the_config_path_follows_the_cli() {
    let path = |pairs: &[(&str, &str)]| config_path(&env_of(pairs));
    assert_eq!(
        path(&[("TXTODO_CONFIG", "/a/c.toml"), ("HOME", "/h")]),
        Some("/a/c.toml".into())
    );
    assert_eq!(
        path(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")]),
        Some("/x/txtodo/config.toml".into())
    );
    assert_eq!(
        path(&[("HOME", "/h")]),
        Some("/h/.config/txtodo/config.toml".into())
    );
    assert_eq!(path(&[]), None);
}
