use std::fs;
use zed_extension_api::{self as zed, Result};

struct DTSExtension {
    cached_binary_path: Option<String>,
}

#[derive(Clone)]
struct DTSBinary(String);

impl DTSExtension {
    fn language_server_binary(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<DTSBinary> {
        if let Some(path) = worktree.which("dts-lsp") {
            return Ok(DTSBinary(path));
        }

        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).map_or(false, |stat| stat.is_file()) {
                return Ok(DTSBinary(path.clone()));
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = zed::latest_github_release(
            "igor-prusov/dts-lsp",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let (platform, arch) = zed::current_platform();
        let version = release
            .version
            .strip_prefix("v")
            .unwrap_or(&release.version);
        let asset_name = format!(
            "dts-lsp-{version}-{arch}-{os}.tar.gz",
            version = version,
            arch = match arch {
                zed::Architecture::Aarch64 => "aarch64",
                zed::Architecture::X8664 => "x86_64",
                zed::Architecture::X86 => return Err("unsupported architecture".into()),
            },
            os = match platform {
                zed::Os::Mac => "apple-darwin",
                zed::Os::Linux => "unknown-linux-musl",
                zed::Os::Windows => "pc-windows-msvc",
            },
        );

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("no asset found matching {:?}", asset_name))?;

        let version_dir = format!("dts-{}", release.version);
        fs::create_dir_all(&version_dir).map_err(|e| format!("failed to create directory: {e}"))?;

        let binary_path = format!(
            "{version_dir}/{DTS_binary}",
            DTS_binary = match platform {
                zed::Os::Mac => "dts-lsp",
                zed::Os::Linux => "dts-lsp",
                zed::Os::Windows => "dts-lsp.exe",
            }
        );

        if !fs::metadata(&binary_path).map_or(false, |stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &version_dir,
                zed::DownloadedFileType::GzipTar,
            )
            .map_err(|e| format!("failed to download file: {e}"))?;

            zed::make_file_executable(&binary_path)?;
            if let Ok(z3s) = fs::read_dir(format!("{version_dir}")) {
                for file in z3s.flatten() {
                    if let Some(path) = file.path().to_str() {
                        zed::make_file_executable(path)?;
                    }
                }
            }

            let entries =
                fs::read_dir(".").map_err(|e| format!("failed to list working directory {e}"))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("failed to load directory entry {e}"))?;
                if entry.file_name().to_str() != Some(&version_dir) {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }

        self.cached_binary_path = Some(binary_path.clone());
        Ok(DTSBinary(binary_path))
    }
}

impl zed::Extension for DTSExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let DTSBinary(path) = self.language_server_binary(language_server_id, worktree)?;
        Ok(zed::Command {
            command: path,
            args: vec![],
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(DTSExtension);
