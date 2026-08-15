# Zed DeviceTree Support

[DeviceTree](https://github.com/devicetree-org/devicetree-specification) support
for the [Zed](https://zed.dev) editor: syntax highlighting via
[tree-sitter-devicetree](https://github.com/joelspadin/tree-sitter-devicetree)
for `.dts`, `.dtsi`, `.dtso`, `.its` and `.overlay` files, plus the
[dts-lsp][lsp] language server.

The server gives you diagnostics, completions, hover with node consolidation
across includes, go-to-definition and declarations, find-references, document
and workspace symbols, rename and formatting.

## Default configuration

The configuration below is used by default, which is suitable for the Linux Kernel tree.

```json
{
  "devicetree": {
    "cwd": "<the folder you opened>",
    "defaultBindingType": "DevicetreeOrg",
    "defaultDeviceOrgTreeBindings": [],
    "defaultDeviceOrgBindingsMetaSchema": [],
    "defaultIncludePaths": ["include"],
    "allowAdhocContexts": true,
    "autoChangeContext": true,
    "defaultShowFormattingErrorAsDiagnostics": false
  }
}
```

Formatting diagnostics are off because the server checks against the
[Zephyr style guide][style], which is not kernel style. Binding validation is
off because it has to be pointed at a schema tree first.

## Your own configuration

Run `zed: open project settings` to create `.zed/settings.json`. Settings go
under `lsp.dts-language-server.settings.devicetree`, and are layered over the
defaults above **one key at a time** — adding `contexts` keeps the rest. To
switch a default off, name it:

```json
{
  "lsp": {
    "dts-language-server": {
      "settings": {
        "devicetree": { "allowAdhocContexts": false }
      }
    }
  }
}
```

Two templates to start from:

- [`templates/linux-kernel.jsonc`](templates/linux-kernel.jsonc) — Linux kernel checkout
- [`templates/zephyr.jsonc`](templates/zephyr.jsonc) — Zephyr west workspace

For the settings themselves, see the [dts-lsp documentation][lsp].

### Variables

Any string in the configuration may contain these, so nothing has to hard-code
where an SDK lives.

| Variable | Expands to | Resolved by |
| --- | --- | --- |
| `${zephyrBase}` | The Zephyr tree — see below. | the extension |
| `${env:NAME}` | An environment variable from your shell. | the extension |
| `${workspaceFolder}` | The folder holding the file being parsed. | the server |
| `${workspaceFolder:name}` | A named workspace folder. | the server |

A name that cannot be resolved is left as it is, so a typo shows up as a
visibly wrong path rather than a plausible one.

`${zephyrBase}` is looked up in this order, first hit wins:

1. A `zephyrBase` setting, described below.
2. `$ZEPHYR_BASE`, which both `west` and `zephyr-env.sh` export.
3. `zephyr.base` from `.west/config`, checked in the folder you opened and one
   level up, since opening the application directory is common.
4. A vendored `zephyr/` directory, detected by `zephyr/VERSION`.

When that picks the wrong tree, or there is more than one to pick from, pin it:

```json
{
  "devicetree": {
    "zephyrBase": "/opt/nordic/ncs/v3.0.0/zephyr"
  }
}
```

Absolute, relative to the folder you opened, or `"${env:NCS_BASE}/zephyr"`.
Empty and non-string values fall back to the lookup above.

`zephyrBase` belongs to the extension, not the language server, so it is
consumed during expansion and never forwarded. It is the only key under
`devicetree` that is not passed straight through.

### Overriding the binary

A `binary` block skips the automatic install:

```json
{
  "lsp": {
    "dts-language-server": {
      "binary": {
        "path": "/usr/local/bin/devicetree-language-server",
        "arguments": ["--stdio"]
      }
    }
  }
}
```