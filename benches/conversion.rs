use std::{str::FromStr, time::Duration};

use criterion::{criterion_group, criterion_main, Criterion};
use num_bigint::BigInt;

use revm::primitives::Address;
use tinyevm::{bigint_to_ruint_u256, trim_prefix};

#[allow(unused)]
fn bench_string_conversion(c: &mut Criterion) {
    c.bench_function("conversion_string_to_address_valid", |b| {
        let address = "0x4838B106FCe9647Bdf1E7877BF73cE8B0BAD5f97";
        b.iter(|| assert!(Address::from_str(trim_prefix(address, "0x")).is_ok()));
    });
    c.bench_function("conversion_string_to_address_invalid", |b| {
        let address = "";
        b.iter(|| assert!(Address::from_str(trim_prefix(address, "0x")).is_err()));
    });
    c.bench_function("conversion_bigint_to_ruint", |b| {
        let i = BigInt::from(0x1234567890abcdefu128);
        b.iter(|| assert!(bigint_to_ruint_u256(&i).is_ok()));
    });
}

criterion_group!(
    name = evm_benches;
    config = Criterion::default().measurement_time(Duration::from_secs(10));
    targets = bench_string_conversion
);

criterion_main!(evm_benches);
