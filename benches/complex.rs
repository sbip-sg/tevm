use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use revm::primitives::Address;
use tinyevm::{fn_sig_to_prefix, UZERO};

const OWNER: Address = Address::repeat_byte(0x01);
const DEPLOY_TO_ADDRESS: Address = Address::repeat_byte(0x02);

#[allow(unused)]
fn bench_call_complex_function(c: &mut Criterion) {
    c.bench_function("call_complex_function", |b| {
        let source = include_str!("../tests/contracts/complex_contract.hex");
        let bytecode = hex::decode(source).unwrap();
        let mut exe = tinyevm::TinyEVM::default();
        let owner = OWNER;
        let deploy_to_address = Some(DEPLOY_TO_ADDRESS);

        let resp = {
            exe.deploy_helper(owner, bytecode, UZERO, None, deploy_to_address)
                .unwrap()
        };

        assert!(resp.success, "Contract deploy should succeed.");
        let address = Address::from_slice(&resp.data);

        let fn_sig = "complexFunction()";
        b.iter(|| {
            let data = hex::decode(fn_sig_to_prefix(fn_sig)).unwrap();

            let r = exe.contract_call_helper(address, owner, data, UZERO, None);
            // assert!(r.success); // this function can revert sometimes
            assert!(r.gas_usage > 0);
        })
    });
}

criterion_group!(
    name = complex;
    config = Criterion::default().measurement_time(Duration::from_secs(10));
    targets = bench_call_complex_function,
);

criterion_main!(complex);
