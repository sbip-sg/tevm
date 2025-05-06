use criterion::{Criterion, criterion_group, criterion_main};
use revm::primitives::Address;
use ruint::aliases::U256;
use tinyevm::{TinyEVM, UZERO};

const OWNER: Address = Address::repeat_byte(0x01);
const DEPLOY_TO_ADDRESS: Address = Address::repeat_byte(0x02);

// Reusing can be slower because there are more data inside the instrumentation log
fn bench_contract_deterministic_deploy(c: &mut Criterion) {
    c.bench_function("deploy_contract_deterministic", |b| {
        let source = include_str!("../tests/contracts/calls_trace.hex");
        let source = hex::decode(source).unwrap();
        let mut exe = TinyEVM::default();

        b.iter(|| {
            {
                exe.deploy_helper(OWNER, source.clone(), UZERO, None, Some(DEPLOY_TO_ADDRESS))
                    .unwrap();
            };
        })
    });
}

fn bench_contract_deploy_on_different_executors(c: &mut Criterion) {
    c.bench_function("deploy_contract_deploy_on_different_executors", |b| {
        let source = include_str!("../tests/contracts/calls_trace.hex");
        let source = hex::decode(source).unwrap();

        b.iter(|| {
            let mut exe = TinyEVM::default();
            {
                exe.deploy_helper(
                    OWNER,
                    source.clone(),
                    U256::from(0),
                    None,
                    Some(DEPLOY_TO_ADDRESS),
                )
                .unwrap();
            };
        })
    });
}

criterion_group!(
    name = deploy;
    config = Criterion::default();
    targets = bench_contract_deterministic_deploy, bench_contract_deploy_on_different_executors
);

criterion_main!(deploy);
