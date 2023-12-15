use crate::starknet_commands::account::Account;
use crate::starknet_commands::show_config::ShowConfig;
use crate::starknet_commands::{
    account, call::Call, declare::Declare, deploy::Deploy, invoke::Invoke, multicall::Multicall,
    script::Script,
};
use anyhow::{anyhow, Result};

use crate::starknet_commands::declare::BuildConfig;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use sncast::helpers::config::{CastConfig, CastConfigBuilder};
use sncast::helpers::constants::DEFAULT_MULTICALL_CONTENTS;
use sncast::{
    chain_id_to_network_name, get_block_id, get_chain_id, get_nonce, get_provider,
    print_command_result, AccountInfo, ValueFormat, WaitForTx,
};
use starknet::providers::jsonrpc::HttpTransport;
use starknet::providers::JsonRpcClient;
use tokio::runtime::Runtime;

mod starknet_commands;

#[derive(Parser)]
#[command(version)]
#[command(about = "sncast - a Starknet Foundry CLI", long_about = None)]
#[clap(name = "sncast")]
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// Profile name in Scarb.toml config file
    #[clap(short, long)]
    profile: Option<String>,

    /// Path to Scarb.toml that is to be used; overrides default behaviour of searching for scarb.toml in current or parent directories
    #[clap(short = 's', long)]
    path_to_scarb_toml: Option<Utf8PathBuf>,

    /// RPC provider url address; overrides url from Scarb.toml
    #[clap(short = 'u', long = "url")]
    rpc_url: Option<String>,

    /// Account to be used for contract declaration;
    /// When using keystore (`--keystore`), this should be a path to account file    
    /// When using accounts file, this should be an account name
    #[clap(short = 'a', long)]
    account: Option<String>,

    #[command(flatten)]
    account_ref: AccountGroup,

    /// If passed, values will be displayed as integers
    #[clap(long, conflicts_with = "hex_format")]
    int_format: bool,

    /// If passed, values will be displayed as hex
    #[clap(long, conflicts_with = "int_format")]
    hex_format: bool,

    /// If passed, output will be displayed in json format
    #[clap(short, long)]
    json: bool,

    /// If passed, command will wait until transaction is accepted or rejected
    #[clap(short = 'w', long)]
    wait: bool,

    /// Adjusts the time after which --wait assumes transaction was not received or rejected
    #[clap(long)]
    wait_timeout: Option<u16>,

    /// Adjusts the time between consecutive attempts to fetch transaction by --wait flag
    #[clap(long)]
    wait_retry_interval: Option<u8>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser)]
#[group(requires = "account", multiple = false)]
pub struct AccountGroup {
    /// Path to the file holding accounts info
    #[clap(short = 'f', long = "accounts-file")]
    accounts_file_path: Option<Utf8PathBuf>,

    /// Path to keystore file; if specified, --account should be a path to starkli JSON account file
    #[clap(short, long)]
    keystore: Option<Utf8PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Declare a contract
    Declare(Declare),

    /// Deploy a contract
    Deploy(Deploy),

    /// Call a contract
    Call(Call),

    /// Invoke a contract
    Invoke(Invoke),

    /// Execute multiple calls
    Multicall(Multicall),

    /// Create and deploy an account
    Account(Account),

    /// Show current configuration being used
    ShowConfig(ShowConfig),

    /// Run a deployment script
    Script(Script),
}

impl Cli {
    fn value_format(&self) -> ValueFormat {
        // Clap validates that both are not passed at same time
        if self.hex_format {
            ValueFormat::Hex
        } else if self.int_format {
            ValueFormat::Int
        } else {
            ValueFormat::Default
        }
    }

    fn to_config_builder(&self) -> CastConfigBuilder {
        CastConfigBuilder {
            rpc_url: self.rpc_url.clone(),
            account: self.account.clone(),
            keystore: self.account_ref.keystore.clone(),
            accounts_file: self.account_ref.accounts_file_path.clone(),
            wait_timeout: self.wait_timeout,
            wait_retry_interval: self.wait_retry_interval,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let value_format = cli.value_format();

    let config = CastConfigBuilder::from_scarb(&cli.profile, &cli.path_to_scarb_toml)?
        .merge(cli.to_config_builder())
        .build()?;

    let provider = get_provider(&config.rpc_url)?;
    let runtime = Runtime::new().expect("Failed to instantiate Runtime");

    if let Commands::Script(script) = cli.command {
        let mut result = starknet_commands::script::run(
            &script.script_module_name,
            &cli.path_to_scarb_toml,
            &provider,
            runtime,
            &config,
        );

        print_command_result("script", &mut result, value_format, cli.json)?;
        Ok(())
    } else {
        runtime.block_on(run_async_command(cli, config, provider, value_format))
    }
}

#[allow(clippy::too_many_lines)]
async fn run_async_command(
    cli: Cli,
    config: CastConfig,
    provider: JsonRpcClient<HttpTransport>,
    value_format: ValueFormat,
) -> Result<()> {
    let wait_config = WaitForTx {
        wait: cli.wait,
        timeout: config.wait_timeout,
        retry_interval: config.wait_retry_interval,
    };
    let build_config = BuildConfig {
        scarb_toml_path: cli.path_to_scarb_toml.clone(),
        json: cli.json,
    };
    match cli.command {
        Commands::Declare(declare) => {
            let account = config.account_info.get_account(&provider).await?;
            let mut result = starknet_commands::declare::declare(
                &declare.contract,
                declare.max_fee,
                &account,
                declare.nonce,
                build_config,
                wait_config,
            )
            .await;

            print_command_result("declare", &mut result, value_format, cli.json)?;
            Ok(())
        }
        Commands::Deploy(deploy) => {
            let account = config.account_info.get_account(&provider).await?;
            let mut result = starknet_commands::deploy::deploy(
                deploy.class_hash,
                deploy.constructor_calldata,
                deploy.salt,
                deploy.unique,
                deploy.max_fee,
                &account,
                deploy.nonce,
                wait_config,
            )
            .await;

            print_command_result("deploy", &mut result, value_format, cli.json)?;
            Ok(())
        }
        Commands::Call(call) => {
            let block_id = get_block_id(&call.block_id)?;

            let mut result = starknet_commands::call::call(
                call.contract_address,
                call.function.as_ref(),
                call.calldata,
                &provider,
                block_id.as_ref(),
            )
            .await;

            print_command_result("call", &mut result, value_format, cli.json)?;
            Ok(())
        }
        Commands::Invoke(invoke) => {
            let account = config.account_info.get_account(&provider).await?;
            let mut result = starknet_commands::invoke::invoke(
                invoke.contract_address,
                &invoke.function,
                invoke.calldata,
                invoke.max_fee,
                &account,
                invoke.nonce,
                wait_config,
            )
            .await;

            print_command_result("invoke", &mut result, value_format, cli.json)?;
            Ok(())
        }
        Commands::Multicall(multicall) => {
            match &multicall.command {
                starknet_commands::multicall::Commands::New(new) => {
                    if let Some(output_path) = &new.output_path {
                        let mut result =
                            starknet_commands::multicall::new::new(output_path, new.overwrite);
                        print_command_result("multicall new", &mut result, value_format, cli.json)?;
                    } else {
                        println!("{DEFAULT_MULTICALL_CONTENTS}");
                    }
                }
                starknet_commands::multicall::Commands::Run(run) => {
                    let account = config.account_info.get_account(&provider).await?;
                    let mut result = starknet_commands::multicall::run::run(
                        &run.path,
                        &account,
                        run.max_fee,
                        wait_config,
                    )
                    .await;

                    print_command_result("multicall run", &mut result, value_format, cli.json)?;
                }
            }
            Ok(())
        }
        Commands::Account(account) => match account.command {
            account::Commands::Add(add) => {
                let accounts_file = config
                    .account_info
                    .as_accounts_file()?
                    .accounts_file
                    .clone();
                let mut result = starknet_commands::account::add::add(
                    &config.rpc_url,
                    &add.name.clone(),
                    &accounts_file,
                    &cli.path_to_scarb_toml,
                    &provider,
                    &add,
                )
                .await;

                print_command_result("account add", &mut result, value_format, cli.json)?;
                Ok(())
            }
            account::Commands::Create(create) => {
                let chain_id = get_chain_id(&provider).await?;
                let account_name = match config.account_info.clone() {
                    AccountInfo::Keystore(keystore) => keystore.account.clone().to_string(),
                    AccountInfo::AccountsFile(_) => create
                        .name
                        .ok_or_else(|| anyhow!("required argument --name not provided"))?,
                };
                let mut result = starknet_commands::account::create::create(
                    &config.rpc_url,
                    &config.account_info,
                    &account_name,
                    &provider,
                    cli.path_to_scarb_toml,
                    chain_id,
                    create.salt,
                    create.add_profile,
                    create.class_hash,
                )
                .await;

                print_command_result("account create", &mut result, value_format, cli.json)?;
                Ok(())
            }
            account::Commands::Deploy(deploy) => {
                let chain_id = get_chain_id(&provider).await?;

                let account_name = config.account_info.as_keystore().ok().map_or_else(
                    || {
                        deploy
                            .name
                            .and_then(|name| if name.is_empty() { None } else { Some(name) })
                            .ok_or_else(|| anyhow!("required argument --name not provided"))
                    },
                    |k| Ok(k.account.to_string()),
                )?;
                let mut result = starknet_commands::account::deploy::deploy(
                    &provider,
                    &config.account_info,
                    account_name,
                    chain_id,
                    deploy.max_fee,
                    wait_config,
                    deploy.class_hash,
                )
                .await;

                print_command_result("account deploy", &mut result, value_format, cli.json)?;
                Ok(())
            }
            account::Commands::Delete(delete) => {
                let network_name = match delete.network {
                    Some(network) => network,
                    None => chain_id_to_network_name(get_chain_id(&provider).await?),
                };

                let mut result = starknet_commands::account::delete::delete(
                    &delete.name,
                    &config.account_info.as_accounts_file()?.accounts_file,
                    &cli.path_to_scarb_toml,
                    delete.delete_profile,
                    &network_name,
                    delete.yes,
                );

                print_command_result("account delete", &mut result, value_format, cli.json)?;
                Ok(())
            }
        },
        Commands::ShowConfig(_) => {
            let mut result = starknet_commands::show_config::show_config(
                &provider,
                config,
                cli.profile,
                cli.path_to_scarb_toml,
            )
            .await;
            print_command_result("show-config", &mut result, value_format, cli.json)?;
            Ok(())
        }
        Commands::Script(_) => unreachable!(),
    }
}
