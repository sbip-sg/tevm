use eyre::Result;

pub mod filesystem_cache;
pub mod redis_cache;

pub trait ProviderCache: Clone + Default {
    fn store(
        &self,
        chain: &str,
        block: u64,
        api: &str,
        request_hash: &str,
        response: &str,
    ) -> Result<()>;

    fn get(
        &self,
        chain: &str,
        block: u64,
        api: &str,
        request_hash: &str,
    ) -> Result<String>;
}
