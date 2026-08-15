use std::collections::HashMap;
use std::fs;

use zed::serde_json::{json, Value};
use zed::settings::LspSettings;
use zed_extension_api::{self as zed, Result};

const LANGUAGE_SERVER_ID: &str = "dts-language-server";
const NPM_PACKAGE: &str = "devicetree-language-server";
const NPM_SERVER_PATH: &str = "node_modules/devicetree-language-server/dist/server.js";

#[derive(Default)]
struct DeviceTreeExtension {
    installed: bool,
}

/// Enough to be useful in a kernel tree
fn default_configuration(worktree: &zed::Worktree) -> Value {
    json!({
        "devicetree": {
            "cwd": worktree.root_path(),
            "defaultBindingType": "DevicetreeOrg",
            "defaultDeviceOrgTreeBindings": [],
            "defaultDeviceOrgBindingsMetaSchema": [],
            "defaultIncludePaths": ["include"],
            "allowAdhocContexts": true,
            "autoChangeContext": true,
            "defaultShowFormattingErrorAsDiagnostics": false,
        }
    })
}

fn west_zephyr_base(config: &str) -> Option<String> {
    let mut in_zephyr_section = false;
    for line in config.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_zephyr_section = section.trim().eq_ignore_ascii_case("zephyr");
            continue;
        }
        if !in_zephyr_section {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim().eq_ignore_ascii_case("base") && !value.trim().is_empty() {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || path.chars().nth(1) == Some(':')
}

fn take_zephyr_base_override(config: &mut Value) -> Option<String> {
    let base = config
        .get_mut("devicetree")?
        .as_object_mut()?
        .remove("zephyrBase")?;
    let base = base.as_str()?.trim();
    (!base.is_empty()).then(|| base.to_string())
}

fn zephyr_base(worktree: &zed::Worktree, env: &HashMap<String, String>) -> Option<String> {
    // What the user chose, if they sourced zephyr-env.sh or ran through west.
    if let Some(base) = env.get("ZEPHYR_BASE").filter(|base| !base.is_empty()) {
        return Some(base.clone());
    }

    // Check if west config is available
    let root = worktree.root_path();
    if let Ok(config) = worktree.read_text_file(".west/config") {
        if let Some(base) = west_zephyr_base(&config) {
            return Some(if is_absolute(&base) {
                base
            } else {
                format!("{root}/{base}")
            });
        }
    }

    // Zephyr vendored in, checked out without west.
    worktree
        .read_text_file("zephyr/VERSION")
        .ok()
        .map(|_| format!("{root}/zephyr"))
}

fn expand(text: &str, lookup: &impl Fn(&str) -> Option<String>) -> String {
    if !text.contains("${") {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str("${");
            rest = after;
            continue;
        };
        let name = &after[..end];
        match lookup(name) {
            Some(value) => out.push_str(&value),
            None => {
                out.push_str("${");
                out.push_str(name);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn expand_value(value: &mut Value, lookup: &impl Fn(&str) -> Option<String>) {
    match value {
        Value::String(text) => *text = expand(text, lookup),
        Value::Array(items) => items.iter_mut().for_each(|item| expand_value(item, lookup)),
        Value::Object(entries) => entries
            .values_mut()
            .for_each(|entry| expand_value(entry, lookup)),
        _ => {}
    }
}

fn workspace_configuration(worktree: &zed::Worktree, user: Option<Value>) -> Value {
    let mut config = merge(default_configuration(worktree), user);

    let env: HashMap<String, String> = worktree.shell_env().into_iter().collect();
    let from_env = |name: &str| {
        name.strip_prefix("env:")
            .and_then(|var| env.get(var).cloned())
    };

    let zephyr = match take_zephyr_base_override(&mut config) {
        // An explicit setting may lean on the environment itself, and may be
        // written relative to the folder that was opened.
        Some(base) => {
            let base = expand(&base, &from_env);
            Some(if is_absolute(&base) {
                base
            } else {
                format!("{}/{base}", worktree.root_path())
            })
        }
        None => zephyr_base(worktree, &env),
    };

    expand_value(&mut config, &|name: &str| match name {
        "zephyrBase" => zephyr.clone(),
        _ => from_env(name),
    });

    config
}

/// Key by key, so that setting `contexts` alone does not quietly drop the rest
/// of the defaults.
fn merge(mut config: Value, user: Option<Value>) -> Value {
    let Some(user) = user else {
        return config;
    };

    let (Some(sections), Some(user_sections)) = (config.as_object_mut(), user.as_object()) else {
        return user;
    };

    for (name, user_section) in user_sections {
        match (
            sections.get_mut(name).and_then(Value::as_object_mut),
            user_section.as_object(),
        ) {
            (Some(defaults), Some(overrides)) => {
                for (key, value) in overrides {
                    defaults.insert(key.clone(), value.clone());
                }
            }
            _ => {
                sections.insert(name.clone(), user_section.clone());
            }
        }
    }

    config
}

impl DeviceTreeExtension {
    fn server_script_path(&mut self, language_server_id: &zed::LanguageServerId) -> Result<String> {
        let installed = |path: &str| fs::metadata(path).is_ok_and(|stat| stat.is_file());

        if self.installed && installed(NPM_SERVER_PATH) {
            return Ok(absolute_server_path());
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let latest = zed::npm_package_latest_version(NPM_PACKAGE)?;

        if !installed(NPM_SERVER_PATH)
            || zed::npm_package_installed_version(NPM_PACKAGE)?.as_ref() != Some(&latest)
        {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            if let Err(error) = zed::npm_install_package(NPM_PACKAGE, &latest) {
                if !installed(NPM_SERVER_PATH) {
                    return Err(error);
                }
            } else if !installed(NPM_SERVER_PATH) {
                return Err(format!(
                    "installed package {NPM_PACKAGE:?} did not contain expected path {NPM_SERVER_PATH:?}"
                ));
            }
        }

        self.installed = true;
        Ok(absolute_server_path())
    }
}

fn absolute_server_path() -> String {
    std::env::current_dir()
        .map(|dir| dir.join(NPM_SERVER_PATH).to_string_lossy().into_owned())
        .unwrap_or_else(|_| NPM_SERVER_PATH.to_string())
}

impl zed::Extension for DeviceTreeExtension {
    fn new() -> Self {
        Self::default()
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary = LspSettings::for_worktree(LANGUAGE_SERVER_ID, worktree)
            .ok()
            .and_then(|settings| settings.binary);

        let mut args = binary
            .as_ref()
            .and_then(|binary| binary.arguments.clone())
            .unwrap_or_else(|| vec!["--stdio".into()]);
        let mut env = worktree.shell_env();
        if let Some(extra) = binary.as_ref().and_then(|binary| binary.env.clone()) {
            env.extend(extra);
        }

        let path = binary
            .and_then(|binary| binary.path)
            .or_else(|| worktree.which(NPM_PACKAGE));

        let command = match path {
            Some(command) => command,
            None => {
                args.insert(0, self.server_script_path(language_server_id)?);
                zed::node_binary_path()?
            }
        };

        Ok(zed::Command { command, args, env })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<Value>> {
        Ok(LspSettings::for_worktree(LANGUAGE_SERVER_ID, worktree)
            .ok()
            .and_then(|settings| settings.initialization_options))
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<Value>> {
        let user = LspSettings::for_worktree(LANGUAGE_SERVER_ID, worktree)
            .ok()
            .and_then(|settings| settings.settings);

        Ok(Some(workspace_configuration(worktree, user)))
    }
}

zed::register_extension!(DeviceTreeExtension);
