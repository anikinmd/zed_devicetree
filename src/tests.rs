//! Everything the extension decides on its own, held to the behaviour the
//! README promises. The language server, npm and the editor are out of reach
//! here; the worktree is not, so it is stood in for.

use std::collections::HashMap;

use zed::serde_json::{json, Value};

use super::*;

/// A worktree with nothing behind it: a root, whichever files a test cares to
/// put in it, and an environment.
#[derive(Default)]
struct FakeProject {
    root: String,
    files: HashMap<String, String>,
    env: Vec<(String, String)>,
}

impl FakeProject {
    fn new(root: &str) -> Self {
        Self {
            root: root.to_string(),
            ..Default::default()
        }
    }

    fn with_file(mut self, path: &str, contents: &str) -> Self {
        self.files.insert(path.to_string(), contents.to_string());
        self
    }

    fn with_env(mut self, name: &str, value: &str) -> Self {
        self.env.push((name.to_string(), value.to_string()));
        self
    }
}

impl Project for FakeProject {
    fn root_path(&self) -> String {
        self.root.clone()
    }

    fn read_text_file(&self, path: &str) -> Result<String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no such file: {path}"))
    }

    fn shell_env(&self) -> Vec<(String, String)> {
        self.env.clone()
    }
}

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// The `devicetree` section of a finished configuration.
fn devicetree(config: &Value) -> &Value {
    &config["devicetree"]
}

// ---------------------------------------------------------------------------
// .west/config parsing
// ---------------------------------------------------------------------------

#[test]
fn west_base_is_read_from_the_zephyr_section() {
    let config = "\
[manifest]
path = zephyr
file = west.yml

[zephyr]
base = zephyr
";
    assert_eq!(west_zephyr_base(config).as_deref(), Some("zephyr"));
}

#[test]
fn west_base_ignores_identically_named_keys_in_other_sections() {
    // `manifest.path` is the trap: in most workspaces it holds the same value.
    let config = "\
[manifest]
base = wrong

[foo]
base = also-wrong
";
    assert_eq!(west_zephyr_base(config), None);
}

#[test]
fn west_base_stops_at_the_next_section() {
    let config = "\
[zephyr]
[manifest]
base = wrong
";
    assert_eq!(west_zephyr_base(config), None);
}

#[test]
fn west_base_tolerates_case_and_padding() {
    let config = "\
  [ Zephyr ]
   BASE   =   ../shared/zephyr
";
    assert_eq!(
        west_zephyr_base(config).as_deref(),
        Some("../shared/zephyr")
    );
}

#[test]
fn west_base_takes_the_first_of_several() {
    let config = "\
[zephyr]
base = first
base = second
";
    assert_eq!(west_zephyr_base(config).as_deref(), Some("first"));
}

#[test]
fn west_base_skips_an_empty_value_and_keeps_looking() {
    let config = "\
[zephyr]
base =
base =
base = real
";
    assert_eq!(west_zephyr_base(config).as_deref(), Some("real"));
}

#[test]
fn west_base_ignores_commented_out_lines() {
    let config = "\
[zephyr]
# base = commented
; base = also-commented
base = real
";
    assert_eq!(west_zephyr_base(config).as_deref(), Some("real"));
}

#[test]
fn west_base_keeps_a_value_containing_an_equals_sign() {
    let config = "\
[zephyr]
base = /opt/sdk=v3/zephyr
";
    assert_eq!(
        west_zephyr_base(config).as_deref(),
        Some("/opt/sdk=v3/zephyr")
    );
}

#[test]
fn west_base_handles_an_empty_or_sectionless_file() {
    assert_eq!(west_zephyr_base(""), None);
    assert_eq!(west_zephyr_base("base = zephyr\n"), None);
}

// ---------------------------------------------------------------------------
// Absolute path detection
// ---------------------------------------------------------------------------

#[test]
fn absolute_paths_are_recognised_on_both_platforms() {
    assert!(is_absolute("/opt/zephyr"));
    assert!(is_absolute("C:/ncs/zephyr"));
    assert!(is_absolute("c:\\ncs\\zephyr"));
}

#[test]
fn relative_paths_are_not_mistaken_for_absolute_ones() {
    assert!(!is_absolute("zephyr"));
    assert!(!is_absolute("../shared/zephyr"));
    assert!(!is_absolute("./zephyr"));
    assert!(!is_absolute(""));
    assert!(!is_absolute("z"));
}

// ---------------------------------------------------------------------------
// The zephyrBase setting: consumed, never forwarded
// ---------------------------------------------------------------------------

#[test]
fn the_override_is_taken_out_of_the_configuration() {
    let mut config = json!({"devicetree": {"zephyrBase": "/opt/zephyr", "cwd": "/work"}});

    assert_eq!(
        take_zephyr_base_override(&mut config).as_deref(),
        Some("/opt/zephyr")
    );
    assert_eq!(config, json!({"devicetree": {"cwd": "/work"}}));
}

#[test]
fn the_override_is_trimmed() {
    let mut config = json!({"devicetree": {"zephyrBase": "  /opt/zephyr\t"}});

    assert_eq!(
        take_zephyr_base_override(&mut config).as_deref(),
        Some("/opt/zephyr")
    );
}

#[test]
fn a_blank_or_non_string_override_is_dropped_rather_than_forwarded() {
    for unusable in [json!(""), json!("   "), json!(42), json!(null), json!([])] {
        let mut config = json!({"devicetree": {"zephyrBase": unusable}});

        assert_eq!(take_zephyr_base_override(&mut config), None);
        // Whatever it was, the server must not see it.
        assert_eq!(config, json!({"devicetree": {}}));
    }
}

#[test]
fn a_missing_override_leaves_the_configuration_alone() {
    let mut config = json!({"devicetree": {"cwd": "/work"}});
    assert_eq!(take_zephyr_base_override(&mut config), None);
    assert_eq!(config, json!({"devicetree": {"cwd": "/work"}}));

    let mut config = json!({"other": {"zephyrBase": "/opt/zephyr"}});
    assert_eq!(take_zephyr_base_override(&mut config), None);

    let mut config = json!({"devicetree": "not an object"});
    assert_eq!(take_zephyr_base_override(&mut config), None);
}

// ---------------------------------------------------------------------------
// Where ${zephyrBase} comes from, and in which order
// ---------------------------------------------------------------------------

#[test]
fn the_environment_wins_over_everything_else() {
    let project = FakeProject::new("/work")
        .with_file(".west/config", "[zephyr]\nbase = zephyr\n")
        .with_file("zephyr/VERSION", "VERSION_MAJOR = 3\n");

    let base = zephyr_base(&project, &env(&[("ZEPHYR_BASE", "/opt/nordic/zephyr")]));
    assert_eq!(base.as_deref(), Some("/opt/nordic/zephyr"));
}

#[test]
fn an_empty_zephyr_base_variable_is_ignored() {
    let project = FakeProject::new("/work").with_file(".west/config", "[zephyr]\nbase = zephyr\n");

    let base = zephyr_base(&project, &env(&[("ZEPHYR_BASE", "")]));
    assert_eq!(base.as_deref(), Some("/work/zephyr"));
}

#[test]
fn west_beats_a_vendored_tree() {
    let project = FakeProject::new("/work")
        .with_file(".west/config", "[zephyr]\nbase = ../shared/zephyr\n")
        .with_file("zephyr/VERSION", "VERSION_MAJOR = 3\n");

    assert_eq!(
        zephyr_base(&project, &env(&[])).as_deref(),
        Some("/work/../shared/zephyr")
    );
}

#[test]
fn an_absolute_west_base_is_used_as_it_stands() {
    let project =
        FakeProject::new("/work").with_file(".west/config", "[zephyr]\nbase = /opt/zephyr\n");

    assert_eq!(
        zephyr_base(&project, &env(&[])).as_deref(),
        Some("/opt/zephyr")
    );
}

#[test]
fn a_west_config_without_a_base_falls_through_to_the_vendored_tree() {
    let project = FakeProject::new("/work")
        .with_file(".west/config", "[manifest]\npath = zephyr\n")
        .with_file("zephyr/VERSION", "VERSION_MAJOR = 3\n");

    assert_eq!(
        zephyr_base(&project, &env(&[])).as_deref(),
        Some("/work/zephyr")
    );
}

#[test]
fn a_vendored_tree_is_recognised_by_its_version_file() {
    let project = FakeProject::new("/work").with_file("zephyr/VERSION", "VERSION_MAJOR = 3\n");

    assert_eq!(
        zephyr_base(&project, &env(&[])).as_deref(),
        Some("/work/zephyr")
    );
}

#[test]
fn a_plain_kernel_tree_has_no_zephyr_base() {
    let project = FakeProject::new("/work").with_file("Makefile", "VERSION = 6\n");

    assert_eq!(zephyr_base(&project, &env(&[])), None);
}

// ---------------------------------------------------------------------------
// Variable expansion
// ---------------------------------------------------------------------------

fn lookup(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    let table: HashMap<String, String> = pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    move |name: &str| table.get(name).cloned()
}

#[test]
fn text_without_a_variable_is_returned_unchanged() {
    let expand_with = lookup(&[("zephyrBase", "/opt/zephyr")]);
    assert_eq!(expand("dts/bindings", &expand_with), "dts/bindings");
    assert_eq!(expand("", &expand_with), "");
    assert_eq!(expand("costs $5 {today}", &expand_with), "costs $5 {today}");
}

#[test]
fn variables_are_substituted_wherever_they_appear() {
    let expand_with = lookup(&[("zephyrBase", "/opt/zephyr"), ("env:ARCH", "arm")]);

    assert_eq!(expand("${zephyrBase}", &expand_with), "/opt/zephyr");
    assert_eq!(
        expand("${zephyrBase}/dts/${env:ARCH}", &expand_with),
        "/opt/zephyr/dts/arm"
    );
    assert_eq!(
        expand("${zephyrBase}${zephyrBase}", &expand_with),
        "/opt/zephyr/opt/zephyr"
    );
}

#[test]
fn an_unknown_variable_is_left_visible() {
    // A typo should read as a wrong path, not a plausible one.
    let expand_with = lookup(&[("zephyrBase", "/opt/zephyr")]);

    assert_eq!(
        expand("${zephyrbase}/dts", &expand_with),
        "${zephyrbase}/dts"
    );
    assert_eq!(expand("${env:NOPE}", &expand_with), "${env:NOPE}");
    assert_eq!(expand("${}", &expand_with), "${}");
}

#[test]
fn an_unterminated_variable_is_left_alone() {
    let expand_with = lookup(&[("zephyrBase", "/opt/zephyr")]);

    assert_eq!(expand("${zephyrBase", &expand_with), "${zephyrBase");
    assert_eq!(
        expand("${zephyrBase}/dts/${arch", &expand_with),
        "/opt/zephyr/dts/${arch"
    );
}

#[test]
fn expansion_does_not_recurse_into_its_own_result() {
    let expand_with = lookup(&[("a", "${b}"), ("b", "boom")]);
    assert_eq!(expand("${a}", &expand_with), "${b}");
}

#[test]
fn expansion_reaches_every_string_in_the_tree() {
    let expand_with = lookup(&[("zephyrBase", "/opt/zephyr")]);
    let mut config = json!({
        "devicetree": {
            "defaultIncludePaths": ["${zephyrBase}/dts", "include"],
            "contexts": [{"dtsFile": "${zephyrBase}/boards/a.dts"}],
            "preferredContext": 0,
            "autoChangeContext": true,
            "nothing": null,
        }
    });

    expand_value(&mut config, &expand_with);

    assert_eq!(
        config,
        json!({
            "devicetree": {
                "defaultIncludePaths": ["/opt/zephyr/dts", "include"],
                "contexts": [{"dtsFile": "/opt/zephyr/boards/a.dts"}],
                "preferredContext": 0,
                "autoChangeContext": true,
                "nothing": null,
            }
        })
    );
}

#[test]
fn expansion_leaves_object_keys_untouched() {
    let expand_with = lookup(&[("zephyrBase", "/opt/zephyr")]);
    let mut config = json!({"${zephyrBase}": "${zephyrBase}"});

    expand_value(&mut config, &expand_with);

    assert_eq!(config, json!({"${zephyrBase}": "/opt/zephyr"}));
}

// ---------------------------------------------------------------------------
// Layering user settings over the defaults
// ---------------------------------------------------------------------------

#[test]
fn without_user_settings_the_defaults_stand() {
    let defaults = json!({"devicetree": {"cwd": "/work", "allowAdhocContexts": true}});
    assert_eq!(merge(defaults.clone(), None), defaults);
}

#[test]
fn user_settings_are_layered_one_key_at_a_time() {
    let merged = merge(
        json!({"devicetree": {"cwd": "/work", "allowAdhocContexts": true}}),
        Some(json!({"devicetree": {"contexts": [], "allowAdhocContexts": false}})),
    );

    assert_eq!(
        merged,
        json!({
            "devicetree": {
                "cwd": "/work",
                "allowAdhocContexts": false,
                "contexts": [],
            }
        })
    );
}

#[test]
fn a_section_the_defaults_do_not_have_is_added_whole() {
    let merged = merge(
        json!({"devicetree": {"cwd": "/work"}}),
        Some(json!({"other": {"key": "value"}})),
    );

    assert_eq!(
        merged,
        json!({"devicetree": {"cwd": "/work"}, "other": {"key": "value"}})
    );
}

#[test]
fn a_non_object_section_replaces_the_defaults_for_that_section() {
    let merged = merge(
        json!({"devicetree": {"cwd": "/work"}}),
        Some(json!({"devicetree": "nonsense"})),
    );

    assert_eq!(merged, json!({"devicetree": "nonsense"}));
}

#[test]
fn non_object_settings_replace_the_defaults_outright() {
    let merged = merge(json!({"devicetree": {}}), Some(json!("nonsense")));
    assert_eq!(merged, json!("nonsense"));
}

// ---------------------------------------------------------------------------
// The whole thing, as the server receives it
// ---------------------------------------------------------------------------

#[test]
fn a_kernel_tree_gets_the_documented_defaults() {
    let config = workspace_configuration(&FakeProject::new("/home/me/linux"), None);

    assert_eq!(
        config,
        json!({
            "devicetree": {
                "cwd": "/home/me/linux",
                "defaultBindingType": "DevicetreeOrg",
                "defaultDeviceOrgTreeBindings": [],
                "defaultDeviceOrgBindingsMetaSchema": [],
                "defaultIncludePaths": ["include"],
                "allowAdhocContexts": true,
                "autoChangeContext": true,
                "defaultShowFormattingErrorAsDiagnostics": false,
            }
        })
    );
}

#[test]
fn the_setting_outranks_the_environment() {
    let project = FakeProject::new("/work")
        .with_env("ZEPHYR_BASE", "/opt/from-env")
        .with_file(".west/config", "[zephyr]\nbase = zephyr\n");
    let user = json!({"devicetree": {
        "zephyrBase": "/opt/nordic/ncs/v3.0.0/zephyr",
        "defaultZephyrBindings": ["${zephyrBase}/dts/bindings", "dts/bindings"],
    }});

    let config = workspace_configuration(&project, Some(user));

    assert_eq!(
        devicetree(&config)["defaultZephyrBindings"],
        json!(["/opt/nordic/ncs/v3.0.0/zephyr/dts/bindings", "dts/bindings"])
    );
    assert!(devicetree(&config).get("zephyrBase").is_none());
}

#[test]
fn a_relative_setting_is_anchored_to_the_folder_that_was_opened() {
    let user = json!({"devicetree": {
        "zephyrBase": "../zephyr",
        "defaultIncludePaths": ["${zephyrBase}/dts"],
    }});

    let config = workspace_configuration(&FakeProject::new("/work/app"), Some(user));

    assert_eq!(
        devicetree(&config)["defaultIncludePaths"],
        json!(["/work/app/../zephyr/dts"])
    );
}

#[test]
fn a_setting_may_lean_on_the_environment_itself() {
    let project = FakeProject::new("/work").with_env("NCS_BASE", "/opt/nordic/ncs/v3.0.0");
    let user = json!({"devicetree": {
        "zephyrBase": "${env:NCS_BASE}/zephyr",
        "defaultIncludePaths": ["${zephyrBase}/dts"],
    }});

    let config = workspace_configuration(&project, Some(user));

    assert_eq!(
        devicetree(&config)["defaultIncludePaths"],
        json!(["/opt/nordic/ncs/v3.0.0/zephyr/dts"])
    );
}

#[test]
fn a_setting_that_expands_to_a_relative_path_is_still_anchored() {
    let project = FakeProject::new("/work").with_env("SDK", "vendor");
    let user = json!({"devicetree": {
        "zephyrBase": "${env:SDK}/zephyr",
        "defaultIncludePaths": ["${zephyrBase}/dts"],
    }});

    let config = workspace_configuration(&project, Some(user));

    assert_eq!(
        devicetree(&config)["defaultIncludePaths"],
        json!(["/work/vendor/zephyr/dts"])
    );
}

#[test]
fn an_unusable_setting_falls_back_to_the_lookup() {
    let project = FakeProject::new("/work").with_file("zephyr/VERSION", "VERSION_MAJOR = 3\n");
    let user = json!({"devicetree": {
        "zephyrBase": "   ",
        "defaultIncludePaths": ["${zephyrBase}/dts"],
    }});

    let config = workspace_configuration(&project, Some(user));

    assert_eq!(
        devicetree(&config)["defaultIncludePaths"],
        json!(["/work/zephyr/dts"])
    );
    assert!(devicetree(&config).get("zephyrBase").is_none());
}

#[test]
fn without_a_setting_the_environment_is_used() {
    let project = FakeProject::new("/work").with_env("ZEPHYR_BASE", "/opt/zephyr");
    let user = json!({"devicetree": {"defaultZephyrBindings": ["${zephyrBase}/dts/bindings"]}});

    let config = workspace_configuration(&project, Some(user));

    assert_eq!(
        devicetree(&config)["defaultZephyrBindings"],
        json!(["/opt/zephyr/dts/bindings"])
    );
}

#[test]
fn without_a_zephyr_tree_the_variable_stays_visible() {
    let user = json!({"devicetree": {
        "defaultIncludePaths": ["${zephyrBase}/dts", "${env:NOPE}/include"],
    }});

    let config = workspace_configuration(&FakeProject::new("/work"), Some(user));

    assert_eq!(
        devicetree(&config)["defaultIncludePaths"],
        json!(["${zephyrBase}/dts", "${env:NOPE}/include"])
    );
}

#[test]
fn the_zephyr_template_resolves_end_to_end() {
    let project = FakeProject::new("/work/app").with_file(
        ".west/config",
        "[manifest]\npath = zephyr\n\n[zephyr]\nbase = zephyr\n",
    );
    let user = json!({"devicetree": {
        "defaultBindingType": "Zephyr",
        "defaultZephyrBindings": ["${zephyrBase}/dts/bindings", "dts/bindings"],
        "defaultIncludePaths": ["${zephyrBase}/dts", "${zephyrBase}/include", "dts", "include"],
        "contexts": [{
            "ctxName": "nrf52840dk",
            "dtsFile": "${zephyrBase}/boards/nordic/nrf52840dk/nrf52840dk_nrf52840.dts",
            "overlays": ["boards/nrf52840dk_nrf52840.overlay"],
        }],
        "preferredContext": 0,
        "defaultShowFormattingErrorAsDiagnostics": true,
        "defaultLockRenameEdits": ["${zephyrBase}"],
    }});

    let config = workspace_configuration(&project, Some(user));
    let devicetree = devicetree(&config);

    assert_eq!(
        config,
        json!({
            "devicetree": {
                // Defaults that survive the layering.
                "cwd": "/work/app",
                "defaultDeviceOrgTreeBindings": [],
                "defaultDeviceOrgBindingsMetaSchema": [],
                "allowAdhocContexts": true,
                "autoChangeContext": true,
                // Overridden, with every ${zephyrBase} resolved.
                "defaultBindingType": "Zephyr",
                "defaultZephyrBindings": ["/work/app/zephyr/dts/bindings", "dts/bindings"],
                "defaultIncludePaths": [
                    "/work/app/zephyr/dts",
                    "/work/app/zephyr/include",
                    "dts",
                    "include",
                ],
                "contexts": [{
                    "ctxName": "nrf52840dk",
                    "dtsFile": "/work/app/zephyr/boards/nordic/nrf52840dk/nrf52840dk_nrf52840.dts",
                    "overlays": ["boards/nrf52840dk_nrf52840.overlay"],
                }],
                "preferredContext": 0,
                "defaultShowFormattingErrorAsDiagnostics": true,
                "defaultLockRenameEdits": ["/work/app/zephyr"],
            }
        })
    );
    assert!(devicetree.get("zephyrBase").is_none());
}
