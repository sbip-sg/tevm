use criterion::{criterion_group, criterion_main, Criterion};
use revm::primitives::Address;
use tinyevm::{fn_sig_to_prefix, TinyEVM, UZERO};

const OWNER: Address = Address::repeat_byte(0x01);
const DEPLOY_TO_ADDRESS: Address = Address::repeat_byte(0x02);

#[allow(unused)]
fn bench_call_tracing_with_shared_executor(c: &mut Criterion) {
    c.bench_function("call_tracing_with_shared_executor", |b| {
        let source = include_str!("../tests/contracts/calls_trace.hex");
        let bytecode = hex::decode(source).unwrap();
        let fn_sig = "test_call_success_success_failed()";
        let fn_args_hex = "";
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

#[allow(unused)]
fn bench_call_tracing_with_different_executor(c: &mut Criterion) {
    c.bench_function("call_tracing_with_different_executor", |b| {
        let source = include_str!("../tests/contracts/calls_trace.hex");
        let bytecode = hex::decode(source).unwrap();
        let fn_sig = "test_call_success_success_failed()";
        let fn_args_hex = "";
        let add_hex = format!("{}{}", fn_sig_to_prefix(fn_sig), fn_args_hex);

        let data = hex::decode(add_hex).unwrap();
        b.iter(|| {
            let mut exe = TinyEVM::default();
            let resp = exe
                .deploy_helper(
                    OWNER,
                    bytecode.clone(),
                    UZERO,
                    None,
                    Some(DEPLOY_TO_ADDRESS),
                )
                .unwrap();

            let address = Address::from_slice(&resp.data);

            let _ = exe.contract_call_helper(address, OWNER, data.clone(), UZERO, None);
        })
    });
}

criterion_group!(
    name = evm_benches;
    config = Criterion::default();
    targets = bench_call_tracing_with_shared_executor, bench_call_tracing_with_different_executor
);

criterion_main!(evm_benches);
