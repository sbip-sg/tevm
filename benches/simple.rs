use criterion::{criterion_group, criterion_main, Criterion};
use revm::primitives::Address;
use std::time::Duration;
use tinyevm::{fn_sig_to_prefix, trim_prefix, TinyEVM, UZERO};
const OWNER: Address = Address::repeat_byte(0x01);
const DEPLOY_TO_ADDRESS: Address = Address::repeat_byte(0x02);

#[allow(unused)]
fn bench_call_get_balance(c: &mut Criterion) {
    c.bench_function("call_call_get_balance", |b| {
        let source = include_str!("../tests/contracts/TetherToken.hex");
        let bytecode = hex::decode(source).unwrap();
        let mut exe = TinyEVM::default();

        let resp = {
            exe.deploy_helper(OWNER, bytecode, UZERO, None, Some(DEPLOY_TO_ADDRESS))
                .unwrap()
        };

        assert!(resp.success, "Contract deploy should succeed.");
        let address = Address::from_slice(&resp.data);

        let fn_sig = "balanceOf(address)";
        let owner = OWNER.to_string();
        let fn_args_hex = trim_prefix(&owner, "0x");
        let data = format!("{}{}", fn_sig_to_prefix(fn_sig), fn_args_hex);
        let data = hex::decode(data).unwrap();
        b.iter(|| {
            let r = exe.contract_call_helper(address, OWNER, data.clone(), UZERO, None);
            assert!(r.success);
        })
    });
}

criterion_group!(
    name = evm_benches;
    config = Criterion::default().measurement_time(Duration::from_secs(10));
    targets = bench_call_get_balance
);

criterion_main!(evm_benches);
