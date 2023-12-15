use anyhow::Result;
use camino::Utf8PathBuf;
use clap::Args;
use sncast::helpers::config::CastConfig;
use sncast::helpers::response_structs::ShowConfigResponse;
use sncast::{chain_id_to_network_name, get_chain_id};
use starknet::providers::jsonrpc::HttpTransport;
use starknet::providers::JsonRpcClient;

#[derive(Args)]
#[command(about = "Show current configuration being used", long_about = None)]
pub struct ShowConfig {}

#[allow(clippy::ptr_arg)]
pub async fn show_config(
    provider: &JsonRpcClient<HttpTransport>,
    cast_config: CastConfig,
    profile: Option<String>,
    scarb_path: Option<Utf8PathBuf>,
) -> Result<ShowConfigResponse> {
    let chain_id_field = get_chain_id(provider).await?;
    let chain_id = chain_id_to_network_name(chain_id_field);
    let accounts_file_path = cast_config
        .account_info
        .as_accounts_file()
        .ok()
        .map(|p| p.accounts_file.clone());
    let keystore = cast_config
        .account_info
        .as_keystore()
        .ok()
        .map(|p| p.keystore.clone());
    Ok(ShowConfigResponse {
        profile,
        chain_id,
        rpc_url: cast_config.rpc_url,
        account: cast_config.account_info.account_name(),
        scarb_path,
        accounts_file_path,
        keystore,
        wait_timeout: cast_config.wait_timeout,
        wait_retry_interval: cast_config.wait_retry_interval,
    })
}
