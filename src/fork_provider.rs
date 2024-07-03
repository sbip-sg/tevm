use ethers::types::{Block, BlockId, Bytes, TxHash, H256};
use ethers_providers::{Http, Middleware, Provider};
use eyre::Result;
use hex::FromHex;
use primitive_types::{H160, U256};
use revm::primitives::Address;
use tokio::runtime::Runtime;
use tracing::debug;

use crate::cache::ProviderCache;

#[derive(Debug)]
pub struct ForkProvider<T: ProviderCache> {
    provider: Provider<Http>,
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
    pub fn new(provider: Provider<Http>, runtime: Runtime) -> Self {
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
        let block_number = self.block_on(async { self.provider.get_block_number().await })?;
        Ok(block_number.as_u64())
    }

    /// Get the nonce of an address
    pub fn get_transaction_count(
        &mut self,
        address: &Address,
        block_number: Option<u64>,
    ) -> Result<U256> {
        let address_str = format!("{:x}", address);
        if let Some(block_number) = block_number {
            if let Ok(cached) =
                self.cache
                    .get("eth", block_number, "eth_getTransactionCount", &address_str)
            {
                return Ok(U256::from_str_radix(cached.as_str(), 16).unwrap());
            }
        }

        let block_id = block_number.map(BlockId::from);
        let nonce = self.block_on(async {
            let addr = H160::from_slice(address.0.as_slice());
            self.provider.get_transaction_count(addr, block_id).await
        })?;

        if let Some(block_number) = block_number {
            self.cache.store(
                "eth",
                block_number,
                "eth_getTransactionCount",
                &address_str,
                &format!("{:x}", nonce),
            )?;
        }

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
            let addr = H160::from_slice(address.0.as_slice());
            self.provider.get_balance(addr, block_id).await
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
            let addr = H160::from_slice(address.0.as_slice());
            self.provider.get_code(addr, block_id).await
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

    pub fn get_block(&mut self, block_number: u64) -> Result<Option<Block<TxHash>>> {
        if let Ok(cached) = self.cache.get(
            "eth",
            block_number,
            "eth_getBlockByNumber",
            &format!("{:x}", block_number),
        ) {
            return Ok(Some(serde_json::from_str(&cached).unwrap()));
        }

        let block_id = BlockId::from(block_number);
        let block = self.block_on(async { self.provider.get_block(block_id).await })?;

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
        index: &H256,
        block_number: Option<u64>,
    ) -> Result<H256> {
        let store_key = format!("{:x}-{:x}", address, index);

        if let Some(block_number) = block_number {
            if let Ok(cached) = self
                .cache
                .get("eth", block_number, "eth_getStorageAt", &store_key)
            {
                return Ok(H256::from_slice(&hex::decode(cached).unwrap()));
            }
        }

        let block_id = block_number.map(BlockId::from);
        let storage = self.block_on(async {
            let addr = H160::from_slice(address.0.as_slice());
            self.provider.get_storage_at(addr, *index, block_id).await
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
