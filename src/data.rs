// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use merge::Merge;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Deserialize, Debug, Clone, Merge)]
pub struct GenericSettings {
    #[serde(rename(deserialize = "sshUser"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub ssh_user: Option<String>,

    #[merge(strategy = merge::option::overwrite_none)]
    pub user: Option<String>,

    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "sshOpts")
    )]
    #[merge(strategy = merge::vec::append)]
    pub ssh_opts: Vec<String>,

    #[serde(rename(deserialize = "compress"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub compress: Option<bool>,

    #[serde(rename(deserialize = "fastConnection"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub fast_connection: Option<bool>,

    #[serde(rename(deserialize = "autoRollback"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub auto_rollback: Option<bool>,

    #[serde(rename(deserialize = "confirmTimeout"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub confirm_timeout: Option<u16>,

    #[serde(rename(deserialize = "activationTimeout"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub activation_timeout: Option<u16>,

    #[serde(rename(deserialize = "tempPath"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub temp_path: Option<PathBuf>,

    #[serde(rename(deserialize = "magicRollback"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub magic_rollback: Option<bool>,

    #[serde(rename(deserialize = "sudo"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub sudo: Option<String>,

    #[serde(default,rename(deserialize = "remoteBuild"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub remote_build: Option<bool>,

    #[serde(rename(deserialize = "interactiveSudo"))]
    #[merge(strategy = merge::option::overwrite_none)]
    pub interactive_sudo: Option<bool>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct NodeSettings {
    pub hostname: String,
    pub profiles: HashMap<String, Profile>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "profilesOrder")
    )]
    pub profiles_order: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ProfileSettings {
    pub path: String,
    #[serde(rename(deserialize = "profilePath"))]
    pub profile_path: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Profile {
    #[serde(flatten)]
    pub profile_settings: ProfileSettings,
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Node {
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
    #[serde(flatten)]
    pub node_settings: NodeSettings,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Data {
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
    pub nodes: HashMap<String, Node>,
}
