use super::ProviderCache;
use eyre::Result;
use std::{
    env,
    fs::{self, File},
    io::Write,
    path::Path,
};

#[derive(Default, Debug, Clone)]
pub struct FileSystemProviderCache {}

impl ProviderCache for FileSystemProviderCache {
    fn store(
        &self,
        chain: &str,
        block: u64,
        api: &str,
        request_hash: &str,
        response: &str,
    ) -> Result<()> {
        let home_dir = env::var("HOME")?;
        let path = Path::new(&home_dir)
            .join(".tinyevm")
            .join(chain)
            .join(block.to_string())
            .join(api);
        fs::create_dir_all(&path)?;
        let mut file = File::create(path.join(request_hash))?;
        file.write_all(response.as_bytes())?;
        Ok(())
    }

    fn get(
        &self,
        chain: &str,
        block: u64,
        api: &str,
        request_hash: &str,
    ) -> Result<String> {
        let home_dir = env::var("HOME")?;
        let path = Path::new(&home_dir)
            .join(".tinyevm")
            .join(chain)
            .join(block.to_string())
            .join(api)
            .join(request_hash);
        Ok(fs::read_to_string(path)?)
    }
}
