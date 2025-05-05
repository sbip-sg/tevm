use alloy::{
    eips::BlockId,
    providers::{Provider, RootProvider},
    rpc::types::{Block, BlockTransactionsKind},
    transports::http::{Client, Http},
};

use eyre::Result;
use hex::FromHex;
use revm::primitives::{Address, Bytes};
use ruint::aliases::U256;
use tokio::runtime::Runtime;
use tracing::debug;

use crate::cache::ProviderCache;

pub type AlloyHttpProvider = RootProvider<Http<Client>>;

#[derive(Debug)]
pub struct ForkProvider<T: ProviderCache> {
    provider: AlloyHttpProvider,
    cache: T,
    runtime: Runtime,
}

impl<T: ProviderCache> Clone for ForkProvider<T> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            runtime: Runtime::new().unwrap(),
            cache: self.cache.clone(),
        }
    }
}

impl<T: ProviderCache> ForkProvider<T> {
    pub fn new(provider: AlloyHttpProvider, runtime: Runtime) -> Self {
        Self {
            provider,
            runtime,
            cache: T::default(),
        }
    }

    fn block_on<F: core::future::Future>(&self, f: F) -> F::Output {
        self.runtime.block_on(f)
    }

    /// Returns the latest block number on chain
    pub fn get_block_number(&self) -> Result<u64> {
        let number = self.block_on(async { self.provider.get_block_number().await })?;
        Ok(number)
    }

    /// Get the nonce of an address
    pub fn get_transaction_count(
        &mut self,
        address: &Address,
        block_number: Option<u64>,
    ) -> Result<u64> {
        let address_str = format!("{:x}", address);
        if let Some(block_number) = block_number {
            if let Ok(cached) =
                self.cache
                    .get("eth", block_number, "eth_getTransactionCount", &address_str)
            {
                return Ok(u64::from_str_radix(cached.as_str(), 16).unwrap());
            }
        }

        let block_id = block_number.map(BlockId::from);
        let nonce = self.block_on(async {
            if let Some(block_id) = block_id {
                self.provider
                    .get_transaction_count(*address)
                    .block_id(block_id)
                    .await
            } else {
                self.provider.get_transaction_count(*address).await
            }
        })?;

        Ok(nonce)
    }

    /// Get the balance of an address
    pub fn get_balance(&mut self, address: &Address, block_number: Option<u64>) -> Result<U256> {
        let address_str = format!("{:x}", address);
        if let Some(block_number) = block_number {
            if let Ok(cached) = self
                .cache
                .get("eth", block_number, "eth_getBalance", &address_str)
            {
                return Ok(U256::from_str_radix(cached.as_str(), 16).unwrap());
            }
        }

        let block_id = block_number.map(BlockId::from);
        let balance = self.block_on(async {
            let rpc = self.provider.get_balance(*address);
            if let Some(block_id) = block_id {
                rpc.block_id(block_id).await
            } else {
                rpc.await
            }
        })?;

        if let Some(block_number) = block_number {
            self.cache.store(
                "eth",
                block_number,
                "eth_getBalance",
                &address_str,
                &format!("{:x}", balance),
            )?;
        }

        Ok(balance)
    }

    pub fn get_code(&mut self, address: &Address, block_number: Option<u64>) -> Result<Bytes> {
        let address_str = format!("{:x}", address);
        if let Some(block_number) = block_number {
            if let Ok(cached) = self
                .cache
                .get("eth", block_number, "eth_getCode", &address_str)
            {
                return Ok(Bytes::from_hex(cached).unwrap());
            }
        }

        let block_id = block_number.map(BlockId::from);
        let code = self.block_on(async {
            let rpc = self.provider.get_code_at(*address);
            if let Some(block_id) = block_id {
                rpc.block_id(block_id).await
            } else {
                rpc.await
            }
        })?;

        if let Some(block_number) = block_number {
            self.cache.store(
                "eth",
                block_number,
                "eth_getCode",
                &address_str,
                &format!("{:x}", code),
            )?;
        }
        Ok(code)
    }

    pub fn get_block(&mut self, block_number: u64) -> Result<Option<Block>> {
        if let Ok(cached) = self.cache.get(
            "eth",
            block_number,
            "eth_getBlockByNumber",
            &format!("{:x}", block_number),
        ) {
            return Ok(Some(serde_json::from_str(&cached).unwrap()));
        }

        let block = self.block_on(async {
            self.provider
                .get_block(BlockId::from(block_number), BlockTransactionsKind::Hashes)
                .await
        })?;

        let _ = self.cache.store(
            "eth",
            block_number,
            "eth_getBlockByNumber",
            &format!("{:x}", block_number),
            &serde_json::to_string(&block)?,
        );
        Ok(block)
    }

    pub fn get_storage_at(
        &mut self,
        address: &Address,
        index: &U256,
        block_number: Option<u64>,
    ) -> Result<U256> {
        let store_key = format!("{:x}-{:x}", address, index);

        if let Some(block_number) = block_number {
            if let Ok(cached) = self
                .cache
                .get("eth", block_number, "eth_getStorageAt", &store_key)
            {
                return Ok(U256::from_str_radix(&cached, 16)?);
            }
        }

        let block_id = block_number.map(BlockId::from);
        let storage = self.block_on(async {
            let rpc = self.provider.get_storage_at(*address, *index);
            if let Some(block_id) = block_id {
                rpc.block_id(block_id).await
            } else {
                rpc.await
            }
        })?;

        debug!(
            "get_storage_at from remote: {:x} {} {}",
            address, index, storage
        );

        if let Some(block_number) = block_number {
            self.cache.store(
                "eth",
                block_number,
                "eth_getStorageAt",
                &store_key,
                &format!("{:x}", storage),
            )?;
        }

        Ok(storage)
    }
}
