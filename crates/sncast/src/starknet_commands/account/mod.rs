use crate::starknet_commands::account::add::Add;
use crate::starknet_commands::account::create::Create;
use crate::starknet_commands::account::delete::Delete;
use crate::starknet_commands::account::deploy::Deploy;
use anyhow::{anyhow, bail, Context, Result};
use camino::Utf8PathBuf;
use clap::{Args, Subcommand};
use serde_json::json;
use sncast::helpers::config::CastConfigBuilder;
use sncast::{
    chain_id_to_network_name, decode_chain_id,
    helpers::scarb_utils::{get_package_tool_sncast, get_scarb_manifest, get_scarb_metadata},
};
use starknet::{core::types::FieldElement, signers::SigningKey};
use std::{fs::OpenOptions, io::Write};
use toml::Value;

pub mod add;
pub mod create;
pub mod delete;
pub mod deploy;

#[derive(Args)]
#[command(about = "Creates and deploys an account to the Starknet")]
pub struct Account {
    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Add(Add),
    Create(Create),
    Deploy(Deploy),
    Delete(Delete),
}

pub fn prepare_account_json(
    private_key: &SigningKey,
    address: FieldElement,
    deployed: bool,
    class_hash: Option<FieldElement>,
    salt: Option<FieldElement>,
) -> serde_json::Value {
    let mut account_json = json!({
        "private_key": format!("{:#x}", private_key.secret_scalar()),
        "public_key": format!("{:#x}", private_key.verifying_key().scalar()),
        "address": format!("{address:#x}"),
        "deployed": deployed,
    });

    if let Some(salt) = salt {
        account_json["salt"] = serde_json::Value::String(format!("{salt:#x}"));
    }
    if let Some(class_hash) = class_hash {
        account_json["class_hash"] = serde_json::Value::String(format!("{class_hash:#x}"));
    }

    account_json
}

#[allow(clippy::too_many_arguments)]
pub fn write_account_to_accounts_file(
    account: &str,
    accounts_file: &Utf8PathBuf,
    chain_id: FieldElement,
    account_json: serde_json::Value,
) -> Result<()> {
    if !accounts_file.exists() {
        std::fs::create_dir_all(accounts_file.clone().parent().unwrap())?;
        std::fs::write(accounts_file.clone(), "{}")?;
    }

    let contents = std::fs::read_to_string(accounts_file.clone())?;
    let mut items: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|_| anyhow!("Failed to parse accounts file at = {}", accounts_file))?;

    let network_name = chain_id_to_network_name(chain_id);

    if !items[&network_name][account].is_null() {
        bail!(
            "Account with name = {} already exists in network with chain_id = {}",
            account,
            decode_chain_id(chain_id)
        );
    }
    items[&network_name][account] = account_json;

    std::fs::write(
        accounts_file.clone(),
        serde_json::to_string_pretty(&items).unwrap(),
    )?;
    Ok(())
}

pub fn add_created_profile_to_configuration(
    path_to_scarb_toml: &Option<Utf8PathBuf>,
    config: &CastConfigBuilder,
) -> Result<()> {
    let manifest_path = match path_to_scarb_toml.clone() {
        Some(path) => path,
        None => get_scarb_manifest().context("Failed to obtain manifest path from scarb")?,
    };
    let metadata = get_scarb_metadata(&manifest_path)?;
    let account_name = config.account.clone().unwrap_or_default();
    if let Ok(tool_sncast) = get_package_tool_sncast(&metadata) {
        let property = tool_sncast
            .get(&account_name)
            .and_then(|profile_| profile_.get("account"));
        if property.is_some() {
            bail!(
                "Failed to add profile = {} to the Scarb.toml. Profile already exists",
                account_name
            );
        }
    }

    let toml_string = {
        let mut tool_sncast = toml::value::Table::new();
        let mut new_profile = toml::value::Table::new();

        new_profile.insert(
            "url".to_string(),
            Value::String(config.rpc_url.clone().unwrap_or_default()),
        );
        new_profile.insert(
            "account".to_string(),
            Value::String(config.account.clone().unwrap_or_default()),
        );
        if let Some(keystore) = config.keystore.clone() {
            new_profile.insert("keystore".to_string(), Value::String(keystore.to_string()));
        } else {
            new_profile.insert(
                "accounts-file".to_string(),
                Value::String(
                    config
                        .accounts_file
                        .clone()
                        .map(|p| p.to_string())
                        .unwrap_or_default(),
                ),
            );
        }

        let account_path = Utf8PathBuf::from(&config.account.clone().unwrap_or_default());
        let profile_name = account_path.file_stem().unwrap_or(&account_name);
        tool_sncast.insert(profile_name.into(), Value::Table(new_profile));

        let mut tool = toml::value::Table::new();
        tool.insert("sncast".to_string(), Value::Table(tool_sncast));

        let mut config = toml::value::Table::new();
        config.insert("tool".to_string(), Value::Table(tool));

        toml::to_string(&Value::Table(config)).context("Failed to convert toml to string")?
    };

    let mut scarb_toml = OpenOptions::new()
        .append(true)
        .open(manifest_path)
        .context("Failed to open Scarb.toml")?;
    scarb_toml
        .write_all(format!("\n{toml_string}").as_bytes())
        .context("Failed to write to the Scarb.toml")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use sealed_test::prelude::rusty_fork_test;
    use sealed_test::prelude::sealed_test;
    use sncast::helpers::config::CastConfigBuilder;
    use sncast::helpers::constants::DEFAULT_ACCOUNTS_FILE;
    use std::fs;

    use crate::starknet_commands::account::add_created_profile_to_configuration;

    #[sealed_test(files = ["tests/data/contracts/constructor_with_params/Scarb.toml"])]
    fn test_add_created_profile_to_configuration_happy_case() {
        let config = CastConfigBuilder {
            rpc_url: Some(String::from("http://some-url")),
            account: Some(String::from("some-name")),
            accounts_file: Some("accounts".into()),
            ..Default::default()
        };
        let res = add_created_profile_to_configuration(&None, &config);

        assert!(res.is_ok());

        let contents = fs::read_to_string("Scarb.toml").expect("Failed to read Scarb.toml");
        assert!(contents.contains("[tool.sncast.some-name]"));
        assert!(contents.contains("account = \"some-name\""));
        assert!(contents.contains("url = \"http://some-url\""));
        assert!(contents.contains("accounts-file = \"accounts\""));
    }

    #[sealed_test(files = ["tests/data/contracts/constructor_with_params/Scarb.toml"])]
    fn test_add_created_profile_to_configuration_profile_already_exists() {
        let config = CastConfigBuilder {
            rpc_url: Some(String::from("http://some-url")),
            account: Some(String::from("myprofile")),
            accounts_file: Some(DEFAULT_ACCOUNTS_FILE.into()),
            ..Default::default()
        };
        let res = add_created_profile_to_configuration(&None, &config);

        assert!(res.is_err());
    }
}
