use anyhow::{Result, anyhow};
use conversions::{IntoConv, string::IntoHexStr};
use starknet_api::block::BlockNumber;
use starknet_rust::{
    core::types::{BlockId, MaybePreConfirmedBlockWithTxHashes},
    providers::{JsonRpcClient, Provider, jsonrpc::HttpTransport},
};
use starknet_types_core::felt::Felt;
use std::collections::HashMap;
use tokio::runtime::Runtime;
use url::Url;

pub struct BlockNumberMap<'a> {
    url_to_latest_block_number: HashMap<Url, BlockNumber>,
    url_and_hash_to_block_number: HashMap<(Url, Felt), BlockNumber>,
    runtime: &'a Runtime,
}

impl<'a> BlockNumberMap<'a> {
    pub fn new(runtime: &'a Runtime) -> Self {
        Self {
            url_to_latest_block_number: HashMap::default(),
            url_and_hash_to_block_number: HashMap::default(),
            runtime,
        }
    }

    pub fn get_latest_block_number(&mut self, url: Url) -> Result<BlockNumber> {
        let block_number = if let Some(block_number) = self.url_to_latest_block_number.get(&url) {
            *block_number
        } else {
            let latest_block_number = self.fetch_latest_block_number(url.clone())?;

            self.url_to_latest_block_number
                .insert(url, latest_block_number);

            latest_block_number
        };

        Ok(block_number)
    }

    pub fn get_block_number_for_hash(&mut self, url: Url, hash: Felt) -> Result<BlockNumber> {
        let block_number = if let Some(block_number) =
            self.url_and_hash_to_block_number.get(&(url.clone(), hash))
        {
            *block_number
        } else {
            let block_number = self.fetch_block_number_for_hash(url.clone(), hash)?;

            self.url_and_hash_to_block_number
                .insert((url, hash), block_number);

            block_number
        };

        Ok(block_number)
    }

    #[must_use]
    pub fn get_url_to_latest_block_number(&self) -> &HashMap<Url, BlockNumber> {
        &self.url_to_latest_block_number
    }

    fn fetch_latest_block_number(&self, url: Url) -> Result<BlockNumber> {
        let client = JsonRpcClient::new(HttpTransport::new(url));

        Ok(self
            .runtime
            .block_on(async { client.block_number().await })
            .map(BlockNumber)?)
    }

    fn fetch_block_number_for_hash(&self, url: Url, block_hash: Felt) -> Result<BlockNumber> {
        let client = JsonRpcClient::new(HttpTransport::new(url));

        let hash = BlockId::Hash(block_hash.into_());

        match self
            .runtime
            .block_on(async { client.get_block_with_tx_hashes(hash).await })
        {
            Ok(MaybePreConfirmedBlockWithTxHashes::Block(block)) => Ok(BlockNumber(block.block_number)),
            _ => Err(anyhow!(
                "Could not get the block number for block with hash 0x{}",
                block_hash.into_hex_string()
            )),
        }
    }
}
