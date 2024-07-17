use eyre::Result;

#[cfg(not(feature = "redis"))]
pub mod filesystem_cache;

#[cfg(feature = "redis")]
pub mod redis_cache;

#[cfg(not(feature = "redis"))]
pub use filesystem_cache::FileSystemProviderCache as DefaultProviderCache;
#[cfg(feature = "redis")]
pub use redis_cache::RedisProviderCache as DefaultProviderCache;

pub trait ProviderCache: Clone + Default {
    fn store(
        &self,
        chain: &str,
        block: u64,
        api: &str,
        request_hash: &str,
        response: &str,
    ) -> Result<()>;

    fn get(&self, chain: &str, block: u64, api: &str, request_hash: &str) -> Result<String>;
}
