use criterion::{criterion_group, criterion_main, Criterion};
use primitive_types::{H160, U256};
use revm::primitives::Address;
use tinyevm::{fn_sig_to_prefix, TinyEVM, UZERO};

const OWNER: Address = Address::repeat_byte(0x01);
const DEPLOY_TO_ADDRESS: Address = Address::repeat_byte(0x02);

// ~100ms per loop
fn bench_infinite_loop_math(c: &mut Criterion) {
    c.bench_function("infinite_loop_with_simple_math", |b| {
        let source = include_str!("../tests/contracts/infinite_loop_Test2.hex");
        let bytecode = hex::decode(source).unwrap();
        let fn_sig = "test1(int256)";
        let fn_args_hex = format!("{:0>64x}", U256::from(0));
        let add_hex = format!("{}{}", fn_sig_to_prefix(fn_sig), fn_args_hex);

        let data = hex::decode(add_hex).unwrap();
        let mut exe = TinyEVM::default();

        let resp = {
            exe.deploy_helper(OWNER, bytecode, UZERO, None, Some(DEPLOY_TO_ADDRESS))
                .unwrap()
        };

        assert!(resp.success, "Contract deploy should succeed.");
        let address = Address::from_slice(&resp.data);

        b.iter(|| {
            let _ = exe.contract_call_helper(address, OWNER, data.clone(), UZERO, None);
        })
    });
}

// <300ms per loop
fn bench_infinite_loop_adderss_call(c: &mut Criterion) {
    c.bench_function("infinite_loop_with_address_call", |b| {
        let source = include_str!("../tests/contracts/infinite_loop_Test.hex");
        let bytecode = hex::decode(source).unwrap();
        let fn_sig = "test1(int256)";
        let fn_args_hex = format!("{:0>64x}", U256::from(0));
        let add_hex = format!("{}{}", fn_sig_to_prefix(fn_sig), fn_args_hex);

        let data = hex::decode(add_hex).unwrap();
        let mut exe = TinyEVM::default();

        let resp = {
            exe.deploy_helper(OWNER, bytecode, UZERO, None, Some(DEPLOY_TO_ADDRESS))
                .unwrap()
        };

        assert!(resp.success, "Contract deploy should succeed.");
        let address = Address::from_slice(&resp.data);

        b.iter(|| {
            let _ = exe.contract_call_helper(address, OWNER, data.clone(), UZERO, None);
        })
    });
}

criterion_group!(
    name = infinite_loop;
    config = Criterion::default();
    targets = bench_infinite_loop_math, bench_infinite_loop_adderss_call
);

criterion_main!(infinite_loop);
