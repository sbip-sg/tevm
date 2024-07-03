use super::ProviderCache;
use eyre::Result;
use redis::{Client, Commands};
use std::env;

#[derive(Clone)]
pub struct RedisProviderCache {
    client: Client,
}

impl Default for RedisProviderCache {
    fn default() -> Self {
        let node =
            env::var("TINYEVM_REDIS_NODE").expect("Redis node is required");
        RedisProviderCache::new(&node).unwrap()
    }
}

impl RedisProviderCache {
    pub fn new(node: &str) -> Result<Self> {
        let client = Client::open(node)?;
        Ok(Self { client })
    }
}

impl ProviderCache for RedisProviderCache {
    fn store(
        &self,
        chain: &str,
        block: u64,
        api: &str,
        request_hash: &str,
        response: &str,
    ) -> Result<()> {
        let key = format!(
            "{}_{}_{}_{}_{}",
            "tinyevm", chain, block, api, request_hash
        );
        let mut conn = self.client.get_connection()?;
        conn.set(key, response)?;
        Ok(())
    }

    fn get(
        &self,
        chain: &str,
        block: u64,
        api: &str,
        request_hash: &str,
    ) -> Result<String> {
        let key = format!(
            "{}_{}_{}_{}_{}",
            "tinyevm", chain, block, api, request_hash
        );
        let mut conn = self.client.get_connection()?;
        let val = conn.get(key)?;
        Ok(val)
    }
}
