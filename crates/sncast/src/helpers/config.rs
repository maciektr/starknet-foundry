use crate::helpers::constants::{DEFAULT_ACCOUNTS_FILE, WAIT_RETRY_INTERVAL, WAIT_TIMEOUT};
use crate::helpers::scarb_utils::{
    get_package_tool_sncast, get_profile, get_property, get_property_optional, get_scarb_manifest,
    get_scarb_metadata,
};
use anyhow::{anyhow, bail, Context, Result};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct CastConfig {
    pub rpc_url: String,
    pub account_info: AccountInfo,
    pub wait_timeout: u16,
    pub wait_retry_interval: u8,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct CastConfigBuilder {
    pub rpc_url: Option<String>,
    pub account: Option<String>,
    pub accounts_file: Option<Utf8PathBuf>,
    pub keystore: Option<Utf8PathBuf>,
    pub wait_timeout: Option<u16>,
    pub wait_retry_interval: Option<u8>,
}

impl CastConfigBuilder {
    pub fn from_scarb(profile: &Option<String>, path: &Option<Utf8PathBuf>) -> Result<Self> {
        let manifest_path = match path.clone() {
            Some(path) => {
                if !path.exists() {
                    bail!("{path} file does not exist!");
                }
                path
            }
            None => get_scarb_manifest().context("Failed to obtain manifest path from scarb")?,
        };

        if !manifest_path.exists() {
            return Ok(Self::default());
        }

        let metadata = get_scarb_metadata(&manifest_path)?;

        match get_package_tool_sncast(&metadata) {
            Ok(package_tool_sncast) => Self::from_package_tool_sncast(package_tool_sncast, profile),
            Err(_) => Ok(Self::default()),
        }
    }

    fn from_package_tool_sncast(
        package_tool_sncast: &Value,
        profile: &Option<String>,
    ) -> Result<CastConfigBuilder> {
        let tool = get_profile(package_tool_sncast, profile)?;

        Ok(CastConfigBuilder {
            rpc_url: get_property(tool, "url"),
            account: get_property(tool, "account"),
            accounts_file: get_property(tool, "accounts-file"),
            keystore: get_property_optional(tool, "keystore"),
            wait_timeout: get_property_optional(tool, "wait-timeout"),
            wait_retry_interval: get_property_optional(tool, "wait-retry-interval"),
        })
    }

    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        CastConfigBuilder {
            rpc_url: self.rpc_url.or(other.rpc_url),
            account: self.account.or(other.account),
            accounts_file: self.accounts_file.or(other.accounts_file),
            keystore: self.keystore.or(other.keystore),
            wait_timeout: self.wait_timeout.or(other.wait_timeout),
            wait_retry_interval: self.wait_retry_interval.or(other.wait_retry_interval),
        }
    }

    pub fn build(self) -> Result<CastConfig> {
        let accounts_file = self.accounts_file.unwrap_or(DEFAULT_ACCOUNTS_FILE.into());
        let accounts_file = Utf8PathBuf::from(shellexpand::tilde(&accounts_file).to_string());
        let account_info = AccountInfo::new(self.account, self.keystore, accounts_file)?;
        let rpc_url = self
            .rpc_url
            .ok_or_else(|| anyhow!("RPC url not passed nor found in Scarb.toml"))?;
        Ok(CastConfig {
            account_info,
            rpc_url,
            wait_timeout: self.wait_timeout.unwrap_or(WAIT_TIMEOUT),
            wait_retry_interval: self.wait_retry_interval.unwrap_or(WAIT_RETRY_INTERVAL),
        })
    }
}

#[derive(Clone, Debug)]
pub enum AccountInfo {
    Keystore(KeystoreAccountInfo),
    AccountsFile(AccountsFileAccountInfo),
}

impl AccountInfo {
    pub fn new(
        account: Option<String>,
        keystore: Option<Utf8PathBuf>,
        accounts_file: Utf8PathBuf,
    ) -> anyhow::Result<Self> {
        if let Some(keystore) = keystore {
            let account = account
                .ok_or_else(|| anyhow!("Account name not passed nor found in Scarb.toml"))?;
            Ok(Self::for_keystore(Utf8PathBuf::from(account), keystore))
        } else {
            Ok(Self::for_accounts_file(account, accounts_file))
        }
    }

    #[must_use]
    pub fn for_keystore(account: Utf8PathBuf, keystore: Utf8PathBuf) -> Self {
        AccountInfo::Keystore(KeystoreAccountInfo { account, keystore })
    }

    #[must_use]
    pub fn for_accounts_file(account: Option<String>, accounts_file: Utf8PathBuf) -> Self {
        AccountInfo::AccountsFile(AccountsFileAccountInfo {
            account,
            accounts_file,
        })
    }

    pub fn as_accounts_file(&self) -> Result<&AccountsFileAccountInfo> {
        match self {
            AccountInfo::AccountsFile(info) => Ok(info),
            AccountInfo::Keystore(_) => bail!("accounts file not defined"),
        }
    }

    pub fn as_keystore(&self) -> Result<&KeystoreAccountInfo> {
        match self {
            AccountInfo::Keystore(info) => Ok(info),
            AccountInfo::AccountsFile(_) => bail!("keystore not defined"),
        }
    }

    #[must_use]
    pub fn account_name(&self) -> Option<String> {
        match self {
            AccountInfo::AccountsFile(info) => info.account.clone(),
            AccountInfo::Keystore(info) => Some(info.account.to_string()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct KeystoreAccountInfo {
    pub account: Utf8PathBuf,
    pub keystore: Utf8PathBuf,
}

#[derive(Clone, Debug)]
pub struct AccountsFileAccountInfo {
    pub account: Option<String>,
    pub accounts_file: Utf8PathBuf,
}
