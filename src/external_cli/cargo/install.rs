use std::{ffi::OsStr, process::exit, str::FromStr};

use anyhow::Context;
use dialoguer::Confirm;
use semver::Version;

#[cfg(feature = "web")]
use crate::external_cli::wasm_bindgen::{self, wasm_bindgen_cli_version};
use crate::external_cli::CommandExt;

/// Check if the given program is installed on the system.
///
/// This assumes that the program offers a `--version` flag.
pub fn is_installed<P: AsRef<OsStr>>(program: P) -> Option<Vec<u8>> {
    CommandExt::new(program)
        .arg("--version")
        .output()
        .map(|output| output.stdout)
        .ok()
}

/// Checks if the program is installed and installs it if it isn't.
///
/// Returns `true` if the program needed to be installed.
pub(crate) fn if_needed(
    program: &str,
    package: &str,
    package_version: Option<&str>,
    skip_prompts: bool,
) -> anyhow::Result<bool> {
    let mut prompt: Option<String> = None;

    if let Some(stdout) = is_installed(program) {
        let Some(package_version) = package_version else {
            // If no `package_version` is specified and the program is installed,
            // there is nothing to do.
            return Ok(false);
        };

        // Its important that the `wasm-bindgen-cli` and the `wasm-bindgen` version match exactly,
        // therefore compare the desired `package_version` with the installed
        // `wasm-bindgen-cli` version
        #[cfg(feature = "web")]
        if package == wasm_bindgen::PACKAGE {
            let version = wasm_bindgen_cli_version(&stdout)?;
            let desired_version = Version::from_str(package_version)?;
            if version == desired_version {
                return Ok(false);
            }
            prompt = Some(format!(
                "`{program}:{version}` is installed, but \
                version `{desired_version}` is required. Install and replace?"
            ));
        }
    }

    // Abort if the user doesn't want to install it
    if !skip_prompts
        && !Confirm::new()
            .with_prompt(
                prompt.unwrap_or_else(|| {
                    format!("`{program}` is missing, should I install it for you?")
                }),
            )
            .interact()
            .context(
                "failed to show interactive prompt, try using `--yes` to confirm automatically",
            )?
    {
        exit(1);
    }

    let mut cmd = CommandExt::new(super::program());
    cmd.arg("install").arg(package);

    if let Some(version) = package_version {
        cmd.arg("--version").arg(version);
    }

    cmd.ensure_status()?;

    Ok(true)
}
