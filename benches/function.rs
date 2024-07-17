use std::{iter::repeat_with, time::Duration};

use criterion::{criterion_group, criterion_main, Criterion};
use primitive_types::H256;
use revm::primitives::Address;
use tinyevm::{fn_sig_to_prefix, TinyEVM, UZERO};

const OWNER: Address = Address::repeat_byte(0x01);
const DEPLOY_TO_ADDRESS: Address = Address::repeat_byte(0x02);

#[allow(unused)]
fn bench_call_function_returning_large_string(c: &mut Criterion) {
    c.bench_function("call_function_returning_large_string", |b| {
        let source = include_str!("../tests/contracts/VeLogo.hex");
        let bytecode = hex::decode(source).unwrap();
        let mut exe = TinyEVM::default();

        let resp = {
            exe.deploy_helper(OWNER, bytecode, UZERO, None, Some(DEPLOY_TO_ADDRESS))
                .unwrap()
        };

        assert!(resp.success, "Contract deploy should succeed.");
        let address = Address::from_slice(&resp.data);

        let fn_sig = "tokenURI(uint256,uint256,uint256,uint256)";
        b.iter(|| {
            let fn_args_hex: String = repeat_with(H256::random).take(4).map(hex::encode).collect();

            let add_hex = format!("{}{}", fn_sig_to_prefix(fn_sig), fn_args_hex);

            let data = hex::decode(add_hex).unwrap();

            let r = exe.contract_call_helper(address, OWNER, data, UZERO, None);
            assert!(r.success);
        })
    });
}

#[allow(unused)]
// TODO this repeats most part of the previous test function, refactor
fn bench_call_function_returning_large_string_no_instrumentation(c: &mut Criterion) {
    c.bench_function(
        "call_function_returning_large_string_no_instrumetation",
        |b| {
            let source = include_str!("../tests/contracts/VeLogo.hex");
            let bytecode = hex::decode(source).unwrap();
            let mut exe = TinyEVM::default();
            exe.instrument_config_mut().enabled = false;

            let resp = {
                exe.deploy_helper(OWNER, bytecode, UZERO, None, Some(DEPLOY_TO_ADDRESS))
                    .unwrap()
            };

            assert!(resp.success, "Contract deploy should succeed.");
            let address = Address::from_slice(&resp.data);

            let fn_sig = "tokenURI(uint256,uint256,uint256,uint256)";
            b.iter(|| {
                let fn_args_hex: String =
                    repeat_with(H256::random).take(4).map(hex::encode).collect();

                let add_hex = format!("{}{}", fn_sig_to_prefix(fn_sig), fn_args_hex);

                let data = hex::decode(add_hex).unwrap();

                let r = exe.contract_call_helper(address, OWNER, data, UZERO, None);
                assert!(r.success);
            })
        },
    );
}

criterion_group!(
    name = evm_benches;
    config = Criterion::default().measurement_time(Duration::from_secs(10));
    targets = bench_call_function_returning_large_string,
    bench_call_function_returning_large_string_no_instrumentation,
);

criterion_main!(evm_benches);
