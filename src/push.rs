// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use indicatif::ProgressBar;
use log::{debug, info, warn};
use std::path::Path;
use std::process::Stdio;
use thiserror::Error;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::LinesStream;

#[derive(Error, Debug)]
pub enum PushProfileError {
    #[error("Failed to run Nix eval command: {0}")]
    EvalStore(std::io::Error),
    #[error("Nixeval command ouput contained an invalid UTF-8 sequence: {0}")]
    EvalStoreUtf8(std::str::Utf8Error),
    #[error("Failed to run Nix show-derivation command: {0}")]
    ShowDerivation(std::io::Error),
    #[error("Nix show-derivation command resulted in a bad exit code: {0:?}")]
    ShowDerivationExit(Option<i32>),
    #[error("Nix show-derivation command output contained an invalid UTF-8 sequence: {0}")]
    ShowDerivationUtf8(std::str::Utf8Error),
    #[error("Failed to parse the output of nix show-derivation: {0}")]
    ShowDerivationParse(serde_json::Error),
    #[error("Nix show derivation output is not an object")]
    ShowDerivationInvalid,
    #[error("Nix show-derivation output is empty")]
    ShowDerivationEmpty,
    #[error("Failed to run Nix build command: {0}")]
    Build(std::io::Error),
    #[error("Nix build command resulted in a bad exit code: {0:?}")]
    BuildExit(Option<i32>),
    #[error(
        "Activation script deploy-rs-activate does not exist in profile.\n\
             Did you forget to use deploy-rs#lib.<...>.activate.<...> on your profile path?"
    )]
    DeployRsActivateDoesntExist,
    #[error(
        "Activation script activate-rs does not exist in profile.\n\
             Is there a mismatch in deploy-rs used in the flake you're deploying and deploy-rs command you're running?"
    )]
    ActivateRsDoesntExist,
    #[error("Failed to run Nix sign command: {0}")]
    Sign(std::io::Error),
    #[error("Nix sign command resulted in a bad exit code: {0:?}")]
    SignExit(Option<i32>),
    #[error("Failed to run Nix copy command: {0}")]
    Copy(std::io::Error),
    #[error("Nix copy command resulted in a bad exit code: {0:?}")]
    CopyExit(Option<i32>),

    #[error("Failed to run Nix path-info command: {0}")]
    PathInfo(std::io::Error),
}

#[derive(Clone)]
pub struct PushProfileData {
    pub supports_flakes: bool,
    pub check_sigs: bool,
    pub repo: String,
    pub deploy_data: super::DeployData,
    pub deploy_defs: super::DeployDefs,
    pub keep_result: bool,
    pub result_path: Option<String>,
    pub extra_build_args: Vec<String>,
}

pub async fn build_profile_locally(
    data: &PushProfileData,
    derivation_name: &str,
) -> Result<(), PushProfileError> {
    info!(
        "Building profile `{}` for node `{}`",
        data.deploy_data.profile_name, data.deploy_data.node_name
    );

    let mut build_command = if data.supports_flakes {
        Command::new("nix")
    } else {
        Command::new("nix-build")
    };

    if data.supports_flakes {
        build_command.arg("build").arg(derivation_name)
    } else {
        build_command.arg(derivation_name)
    };

    if let Ok(build_dir) = std::env::var("TMPDIR") {
        info!("Detected TMPDIR is set for build to {build_dir}");
        build_command.env("TMPDIR", build_dir);
    }
    match (data.keep_result, data.supports_flakes) {
        (true, _) => {
            let result_path = data
                .result_path
                .clone()
                .unwrap_or("./.deploy-gc".to_string());

            build_command.arg("--out-link").arg(format!(
                "{}/{}/{}",
                result_path, data.deploy_data.node_name, data.deploy_data.profile_name
            ))
        }
        (false, false) => build_command.arg("--no-out-link"),
        (false, true) => build_command.arg("--no-link"),
    };

    build_command.args(data.extra_build_args.clone());

    let build_exit_status = build_command
        // Logging should be in stderr, this just stops the store path from printing for no reason
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(PushProfileError::Build)?;

    match build_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExit(a)),
    };

    if !Path::new(
        format!(
            "{}/deploy-rs-activate",
            data.deploy_data.profile.profile_settings.path
        )
        .as_str(),
    )
    .exists()
    {
        return Err(PushProfileError::DeployRsActivateDoesntExist);
    }

    if !Path::new(
        format!(
            "{}/activate-rs",
            data.deploy_data.profile.profile_settings.path
        )
        .as_str(),
    )
    .exists()
    {
        return Err(PushProfileError::ActivateRsDoesntExist);
    }

    if let Ok(local_key) = std::env::var("LOCAL_KEY") {
        info!(
            "Signing key present! Signing profile `{}` for node `{}`",
            data.deploy_data.profile_name, data.deploy_data.node_name
        );

        let sign_exit_status = Command::new("nix")
            .arg("sign-paths")
            .arg("-r")
            .arg("-k")
            .arg(local_key)
            .arg(&data.deploy_data.profile.profile_settings.path)
            .status()
            .await
            .map_err(PushProfileError::Sign)?;

        match sign_exit_status.code() {
            Some(0) => (),
            a => return Err(PushProfileError::SignExit(a)),
        };
    }
    Ok(())
}

async fn update_pb_with_child_output(pb: &ProgressBar, child: &mut Child) {
    let stdout = child
        .stdout
        .take()
        .expect("child did not have a stdout handle");
    let stderr = child
        .stderr
        .take()
        .expect("child did not have a stderr handle");

    let stdout = LinesStream::new(BufReader::new(stdout).lines());
    let stderr = LinesStream::new(BufReader::new(stderr).lines());
    let mut merged = StreamExt::merge(stdout, stderr);

    while let Some(line) = merged.next().await {
        pb.set_message(line.expect("expected a valid line"));
    }
}

pub async fn build_profile_remotely(
    data: &PushProfileData,
    derivation_name: &str,
) -> Result<(), PushProfileError> {
    info!(
        "Building profile `{}` for node `{}` on remote host",
        data.deploy_data.profile_name, data.deploy_data.node_name
    );

    // TODO: this should probably be handled more nicely during 'data' construction
    let hostname = match data.deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &data.deploy_data.node.node_settings.hostname,
    };
    let store_address = format!("ssh-ng://{}@{}", data.deploy_defs.ssh_user, hostname);

    let ssh_opts_str = shlex::try_join(
        data.deploy_data
            .merged_settings
            .ssh_opts
            .iter()
            .map(String::as_str)
            .collect::<Vec<&str>>(),
    )
    .unwrap_or(data.deploy_data.merged_settings.ssh_opts.join(" "));

    // copy the derivation to remote host so it can be built there
    let copy_command_status = {
        let mut copy_command = Command::new("nix");
        copy_command
            .arg("--experimental-features")
            .arg("nix-command");
        copy_command
            .arg("copy")
            .arg("-s") // fetch dependencies from substitutors, not localhost
            .arg("--to")
            .arg(&store_address)
            .arg("--derivation")
            .arg(derivation_name)
            .env("NIX_SSHOPTS", ssh_opts_str.clone());

        debug!("copy command: {:?}", copy_command);

        let mut child = copy_command
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("failed to spawn nix copy command");

        if let Some(pb) = &data.deploy_data.progressbar {
            update_pb_with_child_output(pb, &mut child).await;
        }

        child.wait().await.map_err(PushProfileError::Copy)?
    };

    match copy_command_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::CopyExit(a)),
    };

    let build_exit_status = {
        let mut build_command = Command::new("nix");
        build_command
            .arg("--experimental-features")
            .arg("nix-command")
            .arg("build")
            .arg(derivation_name)
            .arg("--eval-store")
            .arg("auto")
            .arg("--store")
            .arg(&store_address)
            .args(data.extra_build_args.clone())
            .env("NIX_SSHOPTS", ssh_opts_str.clone());

        debug!("build command: {:?}", build_command);

        let mut child = build_command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn nix build command");

        if let Some(pb) = &data.deploy_data.progressbar {
            update_pb_with_child_output(pb, &mut child).await;
        }

        child.wait().await.map_err(PushProfileError::Build)?
    };

    match build_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExit(a)),
    };

    Ok(())
}

pub async fn build_profile(data: &PushProfileData) -> Result<(), PushProfileError> {
    debug!(
        "Finding the deriver of store path for {}",
        &data.deploy_data.profile.profile_settings.path
    );

    // `nix-store --query --deriver` doesn't work on invalid paths, so we parse output of show-derivation :(
    let show_derivation_output = Command::new("nix")
        .arg("--experimental-features")
        .arg("nix-command")
        .arg("show-derivation")
        .arg(&data.deploy_data.profile.profile_settings.path)
        .output()
        .await
        .map_err(PushProfileError::ShowDerivation)?;

    match show_derivation_output.status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::ShowDerivationExit(a)),
    };

    let show_derivation_json: serde_json::value::Value = serde_json::from_str(
        std::str::from_utf8(&show_derivation_output.stdout)
            .map_err(PushProfileError::ShowDerivationUtf8)?,
    )
    .map_err(PushProfileError::ShowDerivationParse)?;

    // Nix 2.33+ nests derivations under a "derivations" key, so try to get that first
    let derivation_info = show_derivation_json
        .get("derivations")
        .unwrap_or(&show_derivation_json)
        .as_object()
        .ok_or(PushProfileError::ShowDerivationInvalid)?;

    let deriver_key = derivation_info
        .keys()
        .next()
        .ok_or(PushProfileError::ShowDerivationEmpty)?;

    // Nix 2.32+ returns relative paths (without /nix/store/ prefix) in show-derivation output
    // Normalize to always use full store paths
    let nix_store_output = Command::new("nix")
        .arg("eval")
        .arg("--raw")
        .arg("--expr")
        .arg("builtins.storeDir")
        .output()
        .await
        .map_err(PushProfileError::EvalStore)?;
    let nix_store =
        std::str::from_utf8(&nix_store_output.stdout).map_err(PushProfileError::EvalStoreUtf8)?;

    let deriver = if deriver_key.starts_with(nix_store) {
        deriver_key.to_string()
    } else {
        format!("{}/{}", nix_store, deriver_key)
    };

    let new_deriver = if data.supports_flakes
        || data
            .deploy_data
            .merged_settings
            .remote_build
            .unwrap_or(false)
    {
        // Since nix 2.15.0 'nix build <path>.drv' will build only the .drv file itself, not the
        // derivation outputs, '^out' is used to refer to outputs explicitly
        deriver.clone() + "^out"
    } else {
        deriver.clone()
    };

    let path_info_output = Command::new("nix")
        .arg("--experimental-features")
        .arg("nix-command")
        .arg("path-info")
        .arg(&deriver)
        .output()
        .await
        .map_err(PushProfileError::PathInfo)?;

    let deriver = if std::str::from_utf8(&path_info_output.stdout).map(|s| s.trim())
        == Ok(deriver.as_str())
    {
        // In this case we're on 2.15.0 or newer, because 'nix path-info <...>.drv'
        // returns the same '<...>.drv' path.
        // If 'nix path-info <...>.drv' returns a different path, then we're on pre 2.15.0 nix and
        // derivation build result is already present in the /nix/store.
        new_deriver
    } else {
        // If 'nix path-info <...>.drv' returns a different path, then we're on pre 2.15.0 nix and
        // derivation build result is already present in the /nix/store.
        //
        // Alternatively, the result of the derivation build may not be yet present
        // in the /nix/store. In this case, 'nix path-info' returns
        // 'error: path '...' is not valid'.
        deriver
    };
    if data
        .deploy_data
        .merged_settings
        .remote_build
        .unwrap_or(false)
    {
        if !data.supports_flakes {
            warn!("remote builds using non-flake nix are experimental");
        }

        build_profile_remotely(data, &deriver).await?;
    } else {
        build_profile_locally(data, &deriver).await?;
    }

    Ok(())
}

pub async fn push_profile(data: PushProfileData) -> Result<(), PushProfileError> {
    let ssh_opts_str = shlex::try_join(
        data.deploy_data
            .merged_settings
            .ssh_opts
            .iter()
            .map(String::as_str)
            .collect::<Vec<&str>>(),
    )
    .unwrap_or(data.deploy_data.merged_settings.ssh_opts.join(" "));

    // remote building guarantees that the resulting derivation is stored on the target system
    // no need to copy after building
    if !data
        .deploy_data
        .merged_settings
        .remote_build
        .unwrap_or(false)
    {
        info!(
            "Copying profile `{}` to node `{}`",
            data.deploy_data.profile_name, data.deploy_data.node_name
        );

        let mut copy_command = Command::new("nix");
        copy_command.arg("copy");

        if data.deploy_data.merged_settings.fast_connection != Some(true) {
            copy_command.arg("--substitute-on-destination");
        }

        if !data.check_sigs {
            copy_command.arg("--no-check-sigs");
        }

        let hostname = match data.deploy_data.cmd_overrides.hostname {
            Some(ref x) => x,
            None => &data.deploy_data.node.node_settings.hostname,
        };

        let compress = data
            .deploy_data
            .merged_settings
            .compress
            .unwrap_or(false);

        let copy_exit_status = copy_command
            .arg("--to")
            .arg(format!(
                "ssh://{}@{}?compress={}",
                data.deploy_defs.ssh_user, hostname, compress
            ))
            .arg(&data.deploy_data.profile.profile_settings.path)
            .env("NIX_SSHOPTS", ssh_opts_str)
            .status()
            .await
            .map_err(PushProfileError::Copy)?;

        match copy_exit_status.code() {
            Some(0) => (),
            a => return Err(PushProfileError::CopyExit(a)),
        };
    }

    Ok(())
}
