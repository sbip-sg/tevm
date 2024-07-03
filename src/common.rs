/// Common constants, data structures and functions to be used by both rust-evm and revm
use eyre::Result;
use hex::ToHex;
use num_bigint::BigInt;
use primitive_types::H256;
use ruint::aliases::U256;
use sha3::{Digest, Keccak256};

/// Default max block gas limit
pub const MAX_BLOCK_GAS: u64 = 1_000_000_000_000_000;
/// U256 zero
pub const UZERO: U256 = U256::ZERO;

/// H256 zero
pub const HZERO: H256 = H256::zero();
/// Gas limit for one transaction
pub const TX_GAS_LIMIT: u64 = 30_000_000;

/// Get binary prefix by function signature
pub fn fn_sig_to_prefix(fn_sig: &str) -> String {
    let ret = Keccak256::digest(fn_sig.as_bytes());
    let ret: String = ret.encode_hex();
    ret[..8].to_owned()
}

/// Decode hex string as vector of bytes, removing any `0x` prefix
pub fn decode_hex_str(data: &str) -> Result<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if data.starts_with("0x") || data.starts_with("0X") {
        let data = data[2..].to_owned();
        Ok(hex::decode(data)?)
    } else {
        Ok(hex::decode(data)?)
    }
}

/// Remove leading prefix from a string, ignoring case
pub fn trim_prefix<'a>(data: &'a str, prefix: &'a str) -> &'a str {
    if data.to_uppercase().starts_with(&prefix.to_uppercase()) {
        &data[2..]
    } else {
        data
    }
}

/// Convert ruint U256 to BigInt
pub fn ruint_u256_to_bigint(u: &U256) -> BigInt {
    BigInt::from_bytes_le(num_bigint::Sign::Plus, &u.as_le_bytes())
}

/// Convert unsigned BigInt to ruint U256
pub fn bigint_to_ruint_u256(b: &BigInt) -> Result<U256> {
    let (sign, bytes) = b.to_bytes_be();
    if sign == num_bigint::Sign::Minus {
        return Err(eyre::eyre!("BigInt is negative"));
    }
    let bytes = &bytes[..32];
    Ok(U256::from_be_slice(bytes.into()))
}
