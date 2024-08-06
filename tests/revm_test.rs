/// Test REVM functions
extern crate lazy_static;
use eyre::{ContextCompat, Result};
use hex::ToHex;
use lazy_static::lazy_static;
use num_bigint::BigInt;
use primitive_types::{H160, H256};
use revm::interpreter::opcode::{self, CREATE, CREATE2, SELFDESTRUCT};
use revm::primitives::Address;
use ruint::aliases::U256;
use std::collections::HashSet;
use std::convert::TryInto;
use std::env;
use std::iter::repeat_with;
use std::ops::Add;
use std::str::FromStr;
use tinyevm::instrument::bug::{Bug, BugType, MissedBranch};
use tracing::warn;

use tinyevm::{
    enable_tracing, fn_sig_to_prefix, ruint_u256_to_bigint, trim_prefix, TinyEVM, TX_GAS_LIMIT,
    UZERO,
};

const TRANSFER_TOKEN_VALUE: u64 = 9999;

/// An test data for integer bugs: (argument of U256 type, optional bug and pc, whether the tx reverts)
type IntegerTestData = (U256, Option<(BugType, usize)>, bool);

lazy_static! {
    static ref OWNER: Address = Address::from_str("0xf000000000000000000000000000000000000000").unwrap();
    // Target address to receive some ERC20 tokens
    static ref TO_ADDRESS: Address =
        Address::from_str("0x1000000000000000000000000000000000000000").unwrap();

    ////////////////////////////////////////////////////////////////////////////////
    // Contract C deployed address
    static ref CONTRACT_ADDRESS: Address =
        Address::from_str("0x0803d1e6309ed01468bb1e0837567edd758bc991").unwrap();

    // total supply of the tokens as defined in contract C, also equals initial token balance of owner
    static ref TOKEN_SUPPLY: U256 = U256::from_str_radix("10000000000000000000000", 10).unwrap();
    ////////////////////////////////////////////////////////////////////////////////

}

macro_rules! deploy_hex {
    ($hex_path: expr, $vm: ident, $addr: ident) => {
        let mut $vm = TinyEVM::default();
        let bytecode_hex = include_str!($hex_path);
        let bytecode = hex::decode(bytecode_hex).unwrap();

        let resp = $vm.deploy_helper(*OWNER, bytecode, UZERO, None, Some(*CONTRACT_ADDRESS));

        assert!(
            resp.is_ok(),
            "Deploying should succeed, but got: {:?}",
            resp
        );

        let resp = resp.unwrap();

        assert!(
            resp.success,
            "Deploying {} should succeed: {:?}",
            bytecode_hex, resp
        );

        println!("Contract deployed to {}", resp.data.encode_hex::<String>());
        let $addr = H160::from_slice(&resp.data);
    };
}

/// Assert all expected bugs are in the `found` bug lists
fn check_expected_bugs_are_found(expected: Vec<(BugType, usize)>, found: Vec<Bug>) {
    let expected_bugs: HashSet<(BugType, usize)> = expected.into_iter().collect();

    let found_bugs: HashSet<(BugType, usize)> = found
        .into_iter()
        .map(|bug| (bug.bug_type, bug.position))
        .collect();

    println!("Found bugs: {:?}", found_bugs);

    let diff: HashSet<_> = expected_bugs.difference(&found_bugs).collect();
    assert_eq!(0, diff.len(), "Expected bugs {diff:#?} should be found");
}

fn t_erc20_balance_query(vm: &mut TinyEVM, address: Address, expected_balance: U256) {
    let prefix = fn_sig_to_prefix("balanceOf(address)");
    let data = format!("{:0<32}{:0>40}", prefix, address.encode_hex::<String>());
    println!("data: {}", data);
    let data = hex::decode(data).unwrap();
    let resp = vm.contract_call_helper(*CONTRACT_ADDRESS, *OWNER, data, UZERO, None);
    assert!(
        resp.success,
        "Call contract to get ERC token balance should succeed"
    );

    println!("resp.data: {:?}", resp.data);

    let balance = U256::from_be_bytes::<32>(resp.data.as_slice().try_into().unwrap());

    assert_eq!(expected_balance, balance);
}

fn setup() {
    let _ = enable_tracing();
}

/// Convenient function create binary for the solidty function: transfer(address,uint256)
fn make_transfer_bin(to: Address, amount: U256) -> Vec<u8> {
    let prefix = fn_sig_to_prefix("transfer(address,uint256)");
    let transfer_hex = format!(
        "{}{:0>64}{:0>64x}",
        prefix,
        to.encode_hex::<String>(),
        amount
    );
    hex::decode(transfer_hex).unwrap()
}

#[test]
fn test_contract_deploy_transfer_query() {
    deploy_hex!("../tests/contracts/C.hex", exe, address);

    println!(
        "Expected contract address {:?}",
        H160::from(*CONTRACT_ADDRESS.0),
    );

    assert_eq!(
        H160::from(*CONTRACT_ADDRESS.0),
        address,
        "Contract address should match"
    );

    t_erc20_balance_query(&mut exe, *OWNER, *TOKEN_SUPPLY);
    t_erc20_balance_query(&mut exe, *TO_ADDRESS, U256::ZERO);

    let bin = make_transfer_bin(*TO_ADDRESS, U256::from(TRANSFER_TOKEN_VALUE));

    for _ in 0..2 {
        let result = exe.contract_call_helper(*CONTRACT_ADDRESS, *OWNER, bin.clone(), UZERO, None);
        assert!(result.success, "Call contract should exit successfully");
    }

    t_erc20_balance_query(
        &mut exe,
        *OWNER,
        *TOKEN_SUPPLY - U256::from(2 * TRANSFER_TOKEN_VALUE),
    );
    t_erc20_balance_query(
        &mut exe,
        *TO_ADDRESS,
        U256::ZERO + U256::from(2 * TRANSFER_TOKEN_VALUE),
    );
}

#[test]
fn test_contract_method_revert() {
    deploy_hex!("../tests/contracts/C.hex", exe, _address);

    let bin = make_transfer_bin(*TO_ADDRESS, U256::MAX);
    let result = exe.contract_call_helper(*CONTRACT_ADDRESS, *OWNER, bin, UZERO, None);
    println!("T resp: {:?}", result);
    assert!(!result.success, "Call contract should revert");
}

fn single_bugtype_test_helper(
    contract_deploy_hex: &str,
    runs: usize,
    fn_sig: &str,
    fn_args_hex: &str,
    expected_bug: Option<(BugType, usize)>,
    expect_revert: bool,
) {
    // Uncomment to enable EVM internal logs
    // enable_print_tracing();
    let owner = OWNER.to_owned();

    let mut vm = TinyEVM::default();

    vm.set_evm_tracing(true);

    let bytecode = hex::decode(contract_deploy_hex).unwrap();

    let resp = vm
        .deploy_helper(owner, bytecode, UZERO, None, None)
        .unwrap();
    let address = Address::from_slice(&resp.data);

    println!("Contract deployed to {}", address.encode_hex::<String>());

    let prefix = fn_sig_to_prefix(fn_sig);
    let add_hex = format!("{}{}", prefix, trim_prefix(fn_args_hex, "0x"));

    println!("Contract fn hex: {add_hex}");
    let data = hex::decode(add_hex).unwrap();

    let mut has_revert = false;
    for _ in 0..runs {
        let resp = vm.contract_call_helper(address, owner, data.clone(), UZERO, None);
        println!("contract {} returns: {:?}", fn_sig, resp);

        has_revert = has_revert || !resp.success;
    }

    assert_eq!(
        expect_revert,
        has_revert,
        "Should {}revert",
        if expect_revert { "" } else { "not " }
    );

    let bugs = vm.bug_data();
    let bugs = bugs.iter().cloned().collect();
    if let Some(expected) = expected_bug {
        check_expected_bugs_are_found(vec![expected], bugs);
    }
}

#[test]
fn test_overflow() {
    setup();
    let u256_max_as_hex = format!("{:#x}", U256::MAX);
    let contract_hex = include_str!("../tests/contracts/IntegerOverflowAdd_deploy.hex");
    let num_runs = 1;
    let fn_sig = "run(uint256)";

    single_bugtype_test_helper(
        contract_hex,
        num_runs,
        fn_sig,
        &u256_max_as_hex,
        Some((BugType::IntegerOverflow, 162)),
        false,
    );
}

// test possible integer truncation bug for contracts compiled with solc version 0.5.0
#[test]
fn test_integer_truncation() {
    let tests = vec![
        (U256::from(12u64), None, false),
        (
            U256::from(200u64),
            Some((BugType::PossibleIntegerTruncation, 132)),
            false,
        ),
    ];
    single_run_test_helper(
        include_str!("../tests/contracts/int_cast_0.5.0.hex"),
        "add(uint8)",
        tests,
    );
}

// test possible integer truncation bug for contracts compiled with solc version 0.8.0
#[test]
fn test_revert_or_invalid() {
    let tests = vec![
        (U256::from(10u64), None, false),
        (
            U256::from(200u64),
            Some((BugType::RevertOrInvalid, 348)),
            true,
        ),
    ];
    single_run_test_helper(
        include_str!("../tests/contracts/int_cast_0.8.0.hex"),
        "add(uint8)",
        tests,
    );
}

#[test]
fn test_timestamp_and_block_number() {
    let fn_args = format!("{:0>64x}", U256::from(32u64));
    let tests = vec![
        (
            include_str!("../tests/contracts/block_number_dependency.060.hex"),
            "timestamp_bug(uint256)",
            &fn_args,
            Some((BugType::TimestampDependency, 244)),
            false,
        ),
        (
            include_str!("../tests/contracts/block_number_dependency.060.hex"),
            "blocknumber_bug(uint256)",
            &fn_args,
            Some((BugType::BlockNumberDependency, 207)),
            false,
        ),
    ];

    for (contract_hex, fn_sig, fn_args, expected_bug, revert) in tests {
        single_bugtype_test_helper(contract_hex, 1, fn_sig, fn_args, expected_bug, revert);
    }
}

#[test]
fn test_tx_origin() {
    setup();
    let contract_hex = include_str!("../tests/contracts/tx_origin.hex");
    let fn_args = "";
    let fn_sig = "run()";
    let expected_bug = Some((BugType::TxOriginDependency, 130));
    let revert = false;

    single_bugtype_test_helper(contract_hex, 1, fn_sig, fn_args, expected_bug, revert);
}

#[test]
fn test_tx_origin_v2() {
    setup();
    let contract_hex = include_str!("../tests/contracts/test_txorigin.hex");
    let fn_args = "";
    let fn_sig = "txorigin()";
    let expected_bug = Some((BugType::TxOriginDependency, 54));
    let revert = false;

    single_bugtype_test_helper(
        contract_hex,
        1,
        fn_sig,
        fn_args,
        expected_bug,
        revert, // this tests require(msg.sender == tx.origin) for one transaction
    );
}

#[test]
fn test_call_trace() {
    setup();
    deploy_hex!("../tests/contracts/calls_trace.hex", vm, address);

    let tests = vec![
        ("always_fail()", vec![(BugType::RevertOrInvalid, 167)], true),
        (
            "test_call_success()",
            vec![(BugType::Call(4, address), 654)],
            false,
        ),
        (
            "self_call()",
            vec![
                (BugType::Call(0, address), 372),
                (BugType::RevertOrInvalid, 102),
            ],
            false,
        ),
        (
            "test_call_success_success_failed()",
            vec![
                (BugType::Call(4, address), 514),
                (BugType::Call(4, address), 255),
                (BugType::RevertOrInvalid, 167),
                (BugType::RevertOrInvalid, 321),
            ],
            false,
        ),
    ];

    for (fn_sig, expected_bugs, expect_revert) in tests {
        let fn_hex = fn_sig_to_prefix(fn_sig);
        let data = hex::decode(fn_hex).unwrap();
        let resp =
            vm.contract_call_helper(Address::new(address.0), *OWNER, data.clone(), UZERO, None);
        assert_eq!(expect_revert, !resp.success);
        let bugs = &vm.bug_data();
        let bugs: Vec<_> = bugs.iter().cloned().collect();
        check_expected_bugs_are_found(expected_bugs, bugs.to_vec());
    }
}

#[test]
fn test_deterministic_deploy() {
    let contract_deploy_hex = include_str!("../tests/contracts/coverage.hex");
    let contract_deploy_bin = hex::decode(contract_deploy_hex).unwrap();
    let mut vm = TinyEVM::default();
    let c1 = vm
        .deploy_helper(*OWNER, contract_deploy_bin.clone(), UZERO, None, None)
        .unwrap();

    assert!(
        c1.success,
        "Deploy by initial nonce should succeed: {:?}",
        &c1
    );

    let c2 = vm
        .deploy_helper(*OWNER, contract_deploy_bin, UZERO, None, None)
        .unwrap();

    assert!(
        c2.success,
        "Deploy by auto updated nonce  should succeed: {:?}",
        &c2
    );

    assert_ne!(c1.data, c2.data, "Address of c1 and c2 should not equal");
}

#[test]
fn test_deterministic_deploy_fail() {
    use hex_literal::hex;
    let constructor_revert_bin = hex!("6080604052348015600f57600080fd5b600080fdfe");
    let constructor_revert_bin = constructor_revert_bin.to_vec();
    let mut vm = TinyEVM::default();
    let c = vm
        .deploy_helper(*OWNER, constructor_revert_bin.clone(), UZERO, None, None)
        .unwrap();

    assert!(!c.success, "Deploy invalid deployment binary should fail",);
}

#[test]
fn test_deterministic_deploy_overwrite() -> Result<()> {
    setup();
    let contract_deploy_hex = include_str!("../tests/contracts/coverage.hex");
    let contract_deploy_bin = hex::decode(contract_deploy_hex).unwrap();
    let target_address = Address::from_slice(H160::random().as_bytes());
    let force_address = Some(target_address);
    let mut vm = TinyEVM::default();
    let c1 = vm
        .deploy_helper(
            *OWNER,
            contract_deploy_bin.clone(),
            UZERO,
            None,
            force_address,
        )
        .unwrap();

    assert!(
        c1.success,
        "Deploy the first time should succeed, resp: {:?}",
        c1
    );

    let c1_address = Address::from_slice(&c1.data);
    assert_eq!(
        target_address, c1_address,
        "Expecting the contract deployed to the target address"
    );

    let c1_code = {
        let accounts = &vm.exe.as_ref().unwrap().db().accounts;
        println!("accounts: {:?}", accounts);
        let account = accounts
            .get(&target_address)
            .context("Expecting first account has non nil value")?;
        account.info.code_hash
    };

    let c2 = vm
        .deploy_helper(*OWNER, contract_deploy_bin, UZERO, None, force_address)
        .unwrap();

    assert!(
        c2.success,
        "Deploy the second time should also succeed, resp: {:?}",
        c2
    );

    let c2_address = Address::from_slice(&c2.data);

    assert_eq!(
        c1_address, c2_address,
        "Deploy same contract to the same forced addess should result in the same address"
    );

    let c2_code = {
        let accounts = &vm.exe.as_ref().unwrap().db().accounts;
        let account = accounts
            .get(&target_address)
            .context("Expecting first account has non nil value")?;
        account.info.code_hash
    };

    assert_eq!(c1_code, c2_code,);
    Ok(())
}

fn test_heuristics_inner(
    input: u64,                                  // `i` in the function `coverage(uint256 i)`
    expected_missed_branches: Vec<MissedBranch>, // expected list of jumpi
    expected_coverages: Vec<usize>,              // expected list of coverage PCs
) {
    deploy_hex!("../tests/contracts/heuristics.hex", exe, address);

    let fn_sig = "coverage(uint256)";
    let fn_sig_hex = fn_sig_to_prefix(fn_sig);
    let fn_args_hex = format!("{:0>64x}", U256::from(input));

    let fn_hex = format!("{}{}", fn_sig_hex, fn_args_hex);

    let tx_data = hex::decode(fn_hex).unwrap();

    let resp = exe.contract_call_helper(Address::new(address.0), *OWNER, tx_data, UZERO, None);

    assert!(
        resp.success,
        "Transaction should succeed with input {}",
        input
    );

    let heuristics = resp.heuristics;

    let missed_branches: Vec<_> = heuristics.missed_branches.into_iter().skip(4).collect();
    let coverage: Vec<usize> = heuristics
        .coverage
        .into_iter()
        .skip(4) // skip 4 from function selector operations
        .collect();

    assert_eq!(
        expected_missed_branches, missed_branches,
        "All missed branches should be found with expected distances with input {}",
        input
    );

    assert_eq!(
        expected_coverages, coverage,
        "List of coverage PCs should match with input {}",
        input
    );
}

#[test]
fn test_heuristics() {
    setup();
    // Test coverage(200)
    let input = 200;
    let expected_missed_branches: Vec<MissedBranch> = vec![
        // (prev_pc, pc, is_jump_to_target, distance)
        // skips 4 from function selector operations
        (119, 127, true, 0x2649),
        (135, 143, false, 0x64),
    ]
    .into_iter()
    .map(|(prev_pc, pc, cond, distance)| (prev_pc, pc, cond, U256::from(distance as u64), 0).into())
    .collect();

    let expected_coverages = vec![127, 136];
    test_heuristics_inner(input, expected_missed_branches, expected_coverages);

    // Test coverage(50)
    let input = 50;
    let expected_missed_branches: Vec<MissedBranch> = vec![
        // (prev_pc, pc, is_jump_to_target, distance)
        (119, 127, true, 0x26df),
        (135, 143, true, 0x33),
        (151, 159, true, 0x30),
    ]
    .into_iter()
    .map(|(prev_pc, pc, cond, distance)| (prev_pc, pc, cond, U256::from(distance as u64), 0).into())
    .collect();

    let expected_coverages = vec![127, 143, 159];
    test_heuristics_inner(input, expected_missed_branches, expected_coverages);
}

#[test]
fn test_heuristics_signed_int() {
    deploy_hex!("../tests/contracts/heuristics-signed-int.hex", exe, address);

    let fn_sig = "coverage(int256)";
    let fn_sig_hex = fn_sig_to_prefix(fn_sig);
    let neg_50 = (!U256::from(50)).add(U256::from(1));
    let fn_args_hex = format!("{:0>64x}", neg_50);

    assert_eq!(
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffce", fn_args_hex,
        "-50 encoded as 256 bit hex"
    );

    let expected_missed_branches: Vec<MissedBranch> = vec![
        // (prev_pc, pc, distance)
        // skips 4 jumpis: callvalue, calldatasize, selector, calldata argument size check
        (155, 195, 9950),
        (235, 275, 51),
        (315, 355, 48),
    ]
    .into_iter()
    .map(|(prev_pc, pc, distance)| (prev_pc, pc, true, U256::from(distance as u64), 0).into())
    .collect();

    let fn_hex = format!("{}{}", fn_sig_hex, fn_args_hex);

    let tx_data = hex::decode(fn_hex).unwrap();

    let resp = exe.contract_call_helper(Address::new(address.0), *OWNER, tx_data, UZERO, None);

    assert!(resp.success, "Transaction should succeed.");

    let r = U256::from_be_bytes::<32>(resp.data.try_into().unwrap());
    println!("Result: {r}");

    let missed_branches: Vec<_> = resp
        .heuristics
        .missed_branches
        .into_iter()
        .skip(4)
        .collect();

    assert_eq!(
        expected_missed_branches.len(),
        missed_branches.len(),
        "All missed branches should be found"
    );

    assert_eq!(
        expected_missed_branches, missed_branches,
        "All missed branches should be found with expected distances"
    );
}

#[test]
fn test_bug_data_in_deploy() {
    let contract_hex = include_str!("../tests/contracts/bug-in-constructor.hex");
    let constructor_args_hex = format!("{:0>64x}", U256::from(2u64));

    let mut vm = TinyEVM::default();
    let owner = OWNER.to_owned();

    vm.set_account_balance(owner, U256::MAX).unwrap();

    let bytecode = hex::decode(format!("{}{}", contract_hex, constructor_args_hex)).unwrap();

    let resp = vm
        .deploy_helper(owner, bytecode, UZERO, None, None)
        .unwrap();

    assert!(resp.success, "Contract deploy should succeed.");

    println!("bugs: {:?}", resp.bug_data);

    assert!(
        !resp.bug_data.is_empty(),
        "Contract deploy should return bug data: {:?}",
        &resp.bug_data
    );

    assert!(
        &resp
            .bug_data
            .iter()
            .any(|bug| bug.bug_type == BugType::IntegerOverflow),
        "A Integer overflow bug should be found in: {:?}",
        &resp.bug_data
    )
}

#[test]
fn test_deploy_with_args_and_value() {
    let contract_hex = include_str!("../tests/contracts/deploy_with_args_and_value.hex");
    let a = U256::from_str_radix("ccaa", 16).unwrap();
    let b = U256::from_str_radix("ffee", 16).unwrap();
    let value = U256::from_str_radix("fffffffff", 16).unwrap();
    let constructor_args_hex = format!("{:0>64x}{:0>64x}", a, b);

    let mut vm = TinyEVM::default();
    let owner = OWNER.to_owned();

    vm.set_account_balance(owner, U256::MAX).unwrap();

    let bytecode = hex::decode(format!("{}{}", contract_hex, constructor_args_hex)).unwrap();

    let resp = vm
        .deploy_helper(owner, bytecode, value, None, None)
        .unwrap();

    println!("resp: {resp:?}");

    assert!(
        resp.success,
        "Deploy contract with constructor args and value should succeed."
    );

    let address = Address::from_slice(&resp.data);
    assert_eq!(
        value,
        vm.get_eth_balance(address).unwrap(),
        "Contract should hold the expected balance"
    );

    let mut t_read_value = |fn_sig, expected_value| {
        let fn_sig_hex = fn_sig_to_prefix(fn_sig);
        let tx_data = hex::decode(fn_sig_hex).unwrap();

        let resp = vm.contract_call_helper(address, owner, tx_data, UZERO, None);
        assert!(
            resp.success,
            "Read public value with {} error {:?}.",
            fn_sig, resp.exit_reason
        );

        let v = U256::from_be_bytes::<32>(resp.data.as_slice().try_into().unwrap());
        assert_eq!(expected_value, v, "Incorrect value read from {}", fn_sig);
    };

    t_read_value("x()", a);
    t_read_value("y()", b);
}

#[test]
fn test_div_zero() {
    single_run_test_helper(
        include_str!("../tests/contracts/divzeros.hex"),
        "test(uint256)",
        vec![(U256::from(0), Some((BugType::IntegerDivByZero, 121)), false)],
    );

    single_run_test_helper(
        include_str!("../tests/contracts/sample_all_bugs.BugSample.hex"),
        "div_by_zero(uint256)",
        vec![(U256::from(0), Some((BugType::IntegerDivByZero, 121)), false)],
    );
}

#[test]
fn test_mod_zero() {
    setup();
    let tests = vec![
        (U256::from(1), Some((BugType::IntegerModByZero, 149)), false),
        (U256::from(2), Some((BugType::IntegerModByZero, 178)), false),
        (U256::from(3), Some((BugType::IntegerModByZero, 242)), false),
    ];
    single_run_test_helper(
        include_str!("../tests/contracts/divzeros.hex"),
        "test(uint256)",
        tests,
    );
}

#[test]
fn test_gas_usage() {
    setup();
    let owner = *OWNER;
    // deploy_hex!("../tests/contracts/gasusage.hex", exe, address);

    let mut vm = TinyEVM::default();
    let bytecode_hex = include_str!("../tests/contracts/gasusage.hex");
    let bytecode = hex::decode(bytecode_hex).unwrap();

    let resp = vm.deploy_helper(*OWNER, bytecode, UZERO, None, None);

    assert!(resp.is_ok(), "Contract deploy should succeed.");

    // let resp = resp.unwrap();
    let address = Address::from_slice(&resp.unwrap().data);

    let fn_sig = "run()";
    let bin = fn_sig_to_prefix(fn_sig);
    let bin = hex::decode(bin).unwrap();
    let resp = vm.contract_call_helper(address, owner, bin, UZERO, None);

    let value = U256::from_be_bytes::<32>(resp.data.as_slice().try_into().unwrap());
    assert_eq!(
        value,
        U256::from(9998),
        "Return value of run() should match"
    );

    assert!(
        resp.gas_usage > 900000,
        "Gas usage should match, actual value {}",
        resp.gas_usage
    );
}

#[test]
fn test_set_get_storage() {
    let owner = *OWNER;
    deploy_hex!("../tests/contracts/storage.hex", exe, addr);
    let index = U256::ZERO;
    let target_value = U256::from(99u64);
    let address = format!("{:040x}", addr);
    let index = format!("{:064x}", index);
    let value = format!("{:064x}", target_value);

    let r = exe.set_storage(address.clone(), index.clone(), value);
    assert!(
        r.is_ok(),
        "Set storage by address and index should succeed."
    );
    let val = exe.get_storage(address, index);
    assert!(val.is_ok(), "Get storage should return some data");
    assert_eq!(
        ruint_u256_to_bigint(&target_value),
        val.unwrap(),
        "Storage should be updated to the target value"
    );

    let fn_sig = "val()";
    let fn_sig_hex = fn_sig_to_prefix(fn_sig);
    let bin = hex::decode(fn_sig_hex).unwrap();
    let resp = exe.contract_call_helper(Address::new(addr.0), owner, bin, UZERO, None);

    assert!(
        resp.success,
        "Call val() to get value should succeed in resp: {:?}",
        resp
    );

    let result = U256::from_be_bytes::<32>(resp.data.as_slice().try_into().unwrap());
    assert_eq!(
        target_value, result,
        "Set storage should modify the corresponding value in contract in {:?}",
        resp
    );
}

#[test]
fn test_set_get_code() {
    setup();
    let owner = Address::new(H160::random().0);
    let mut vm = TinyEVM::default();

    let bytecode = "6080604052348015600f57600080fd5b506004361060285760003560e01c806306661abd14602d575b600080fd5b60336049565b6040518082815260200191505060405180910390f35b600063075bcd1590509056fea2646970667358221220e78a1be79408618d44865cc7414258752af2f0f5a4a71e57ec8ee4cb78af994164736f6c63430007000033";
    let code = hex::decode(bytecode).unwrap();

    let r = vm.set_code_by_address(owner, code);
    assert!(r.is_ok(), "Set code by address should succeed.");

    let actual = vm
        .get_code_by_address(owner)
        .unwrap()
        .encode_hex::<String>();

    println!("actual {}", actual);

    assert!(
        &actual.starts_with(bytecode),
        "Get code should match what's set"
    );
}

#[test]
fn test_exp_overflow() {
    setup();
    let owner = *OWNER;
    deploy_hex!("../tests/contracts/exp_overflow.hex", vm, address);

    let fn_sig = "exp(uint256)";
    let fn_sig_hex = fn_sig_to_prefix(fn_sig);
    let bin = format!("{}{:0>64x}", fn_sig_hex, 200);
    let bin = hex::decode(bin).unwrap();

    println!("Calling deployed contract: {:?}", address);
    println!(
        "Current accounts: {:?}",
        vm.exe.as_ref().unwrap().db().accounts
    );

    let resp = vm.contract_call_helper(Address::new(address.0), owner, bin, UZERO, None);

    assert!(
        !resp.bug_data.into_iter().any(|b| b.opcode == opcode::EXP),
        "Not expecting exp overflow"
    );

    let bin = format!("{}{:0>64x}", fn_sig_hex, 257);
    let bin = hex::decode(bin).unwrap();

    let resp = vm.contract_call_helper(Address::new(address.0), owner, bin, UZERO, None);

    let bugs = &resp.bug_data;

    assert!(
        &bugs.iter().any(|b| b.opcode == opcode::EXP),
        "Expecting exp overflow in {:?}",
        bugs
    );
}

fn single_run_test_helper(contract_bin_hex: &str, fn_sig: &str, tests: Vec<IntegerTestData>) {
    for (arg, expected_bug, expect_revert) in tests {
        let fn_args_hex = {
            if arg == U256::ZERO {
                "0".repeat(64) // temporary fix for 0 value
            } else {
                format!("{:0>64x}", arg)
            }
        };
        single_bugtype_test_helper(
            contract_bin_hex,
            1,
            fn_sig,
            &fn_args_hex,
            expected_bug,
            expect_revert,
        );
    }
}

#[test]
fn test_deadloop() {
    setup();
    let owner = *OWNER;
    deploy_hex!("../tests/contracts/deadloop.hex", vm, address);

    let fn_sig = "run()";
    let bin = fn_sig_to_prefix(fn_sig);
    let bin = hex::decode(bin).unwrap();
    let resp = vm.contract_call_helper(Address::new(address.0), owner, bin, UZERO, None);

    assert!(!resp.success, "Expect deadloop to crash");
    println!("resp: {:?}", resp);
    assert!(
        resp.gas_usage >= TX_GAS_LIMIT,
        "Gas usage should exceed the tx max gas limit"
    );
}

#[test]
fn test_blockhash() {
    let owner = *OWNER;
    let bytecode = include_str!("../tests/contracts/block_hash.hex");
    let bytecode = hex::decode(bytecode).unwrap();
    let mut vm = TinyEVM::default();

    let test_cases = [
        // (block.number, hash of previous block)
        (
            U256::from(19),
            "bb8a6a4669ba250d26cd7a459eca9d215f8307e33aebe50379bc5a3617ec3444",
        ),
        (
            U256::from(123323),
            "a3f98895c80ab4ac19223ea27a3e1ff0d8b1cedc645e2bdf3d5118de9cbcead4",
        ),
    ];

    for (block, hash) in test_cases {
        let block_env = &mut vm.exe.as_mut().unwrap().block_mut();
        block_env.number = block;

        let resp = vm
            .deploy_helper(owner, bytecode.clone(), UZERO, None, None)
            .unwrap();

        let addr = Address::from_slice(&resp.data);

        let previous_blockhash = {
            let bin = hex::decode(fn_sig_to_prefix("lh()")).unwrap();
            let resp = vm.contract_call_helper(addr, owner, bin, UZERO, None);
            format!(
                "{:x}",
                U256::from_be_bytes::<32>(resp.data.try_into().unwrap())
            )
        };

        let current_block = {
            let bin = hex::decode(fn_sig_to_prefix("bn()")).unwrap();
            let resp = vm.contract_call_helper(addr, owner, bin, UZERO, None);
            U256::from_be_bytes::<32>(resp.data.try_into().unwrap())
        };

        assert_eq!(
            block, current_block,
            "EVM should use the blocked number set in env"
        );

        assert_eq!(
            hash, previous_blockhash,
            "In memory EVM should use hash(blocknum) as hash of a block"
        );
    }
}

#[test]
fn test_tod() {
    setup();
    deploy_hex!("../tests/contracts/test_tod.hex", vm, addr);
    let owner = *OWNER;
    vm.clear_instrumentation();

    let bin = hex::decode(fn_sig_to_prefix("play_TOD27()")).unwrap();
    let resp = vm.contract_call_helper(Address::new(addr.0), owner, bin, UZERO, None);
    assert!(resp.success, "Call should succeed");
    let bugs = vm.bug_data().clone();

    let expected_sstore_pcs: HashSet<usize> = vec![501, 554, 561].into_iter().collect();

    let actual: HashSet<usize> = bugs
        .into_iter()
        .filter(|b| matches!(b.bug_type, BugType::Sload(_) | BugType::Sstore(_, _)))
        .map(|b| b.position)
        .collect();

    assert_eq!(
        expected_sstore_pcs, actual,
        "Expect all program counters for SSTORE match"
    );

    let val = U256::from(1);
    let arg_hex = format!("{:0>64x}", val);
    let bin = format!("{}{}", fn_sig_to_prefix("write_a(uint256)"), arg_hex);
    let bin = hex::decode(bin).unwrap();

    let resp = vm.contract_call_helper(Address::new(addr.0), owner, bin, UZERO, None);
    assert!(resp.success, "Call should succeed");
    let bugs = vm.bug_data().clone();

    println!("{:?}", bugs);

    let idx = U256::from_str_radix(
        "77889682276648159348121498188387380826073215901308117747004906171223545284475",
        10,
    )
    .unwrap();

    let expected_sstore = Bug::new(BugType::Sstore(idx, val), 85, 631, 0);

    assert!(
        resp.bug_data
            .into_iter()
            .any(|b| b.bug_type == expected_sstore.bug_type
                && b.opcode == expected_sstore.opcode
                && b.position == expected_sstore.position),
        "The expected SSTORE should be found"
    );
}

#[test]
fn test_get_set_balance() {
    // Test balance set get
    let from = Address::from_slice(H160::random().as_bytes());
    let owner = from;

    let target_balance = U256::from(232321u64);
    let mut vm = TinyEVM::default();

    let balance = vm.get_eth_balance(owner).unwrap();
    assert_eq!(UZERO, balance, "Expect empty account has zero balance");

    vm.set_account_balance(owner, target_balance).unwrap();

    let balance = vm.get_eth_balance(owner).unwrap();
    assert_eq!(
        balance, target_balance,
        "Expect changed to the target balance"
    );

    // Verify the balance actually changed from a contract
    let bytecode = include_str!("../tests/contracts/balance.hex");
    let bytecode = hex::decode(bytecode).unwrap();
    let resp = vm
        .deploy_helper(owner, bytecode, UZERO, None, None)
        .unwrap();
    assert!(resp.success, "Deployment should succeed");
    let addr = Address::from_slice(&resp.data);

    vm.set_account_balance(addr, target_balance).unwrap();

    let bin = hex::decode(fn_sig_to_prefix("selfbalance()")).unwrap();
    let resp = vm.contract_call_helper(addr, owner, bin, UZERO, None);
    assert!(resp.success, "Call error {:?}", resp);
    assert_eq!(
        target_balance,
        U256::from_be_bytes::<32>(resp.data.try_into().unwrap()),
        "Should be able to change the balance of a contract"
    );

    let bin = format!(
        "{}{:0>64}",
        fn_sig_to_prefix("balance(address)"),
        owner.encode_hex::<String>()
    );

    let bin = hex::decode(bin).unwrap();
    let resp = vm.contract_call_helper(addr, owner, bin, UZERO, None);
    assert!(resp.success, "Call error {:?}", resp);
    assert_eq!(
        target_balance,
        U256::from_be_bytes::<32>(resp.data.try_into().unwrap()),
        "Should be able to get the changed balance of others from inside a contract"
    );
}

#[test]
fn test_selfdestruct_and_create() {
    setup();
    deploy_hex!("../tests/contracts/self_destruct.hex", vm, addr);

    let bin = hex::decode(fn_sig_to_prefix("kill()")).unwrap();
    let resp = vm.contract_call_helper(Address::new(addr.0), *OWNER, bin, UZERO, None);
    assert!(resp.success, "Call error {:?}", resp);

    let bugs = resp.bug_data;
    assert!(
        &bugs
            .iter()
            .clone()
            .any(|b| matches!(b.bug_type, BugType::Unclassified) && b.opcode == SELFDESTRUCT),
        "Selfdestruct should be detected"
    );

    assert!(
        &bugs
            .iter()
            .clone()
            .any(|b| matches!(b.bug_type, BugType::Unclassified)
                && (b.opcode == CREATE || b.opcode == CREATE2)),
        "CREATE/CREATE2 should be detected"
    );
}

#[test]
fn test_seen_pcs() {
    // Deploy contract B
    deploy_hex!("../tests/contracts/contract_creation_B.hex", vm, address);

    // Give owner some ether
    let owner: Address = *OWNER;
    vm.set_account_balance(
        owner,
        U256::from_str_radix("9999999999999999999999", 16).unwrap(),
    )
    .unwrap();

    // Call b.add() with some ether
    let bin = hex::decode(fn_sig_to_prefix("add()")).unwrap();
    let resp = vm.contract_call_helper(
        Address::new(address.0),
        *OWNER,
        bin,
        U256::from_str_radix("999999", 16).unwrap(),
        None,
    );
    assert!(resp.success, "Call error {:?}", resp);

    let seen_pcs = &vm.pcs_by_address().get(&Address::new(address.0));
    assert!(
        seen_pcs.is_some(),
        "Seen PCs should be found for the target contract "
    );
    let seen_pcs = seen_pcs.unwrap();

    println!("Seen PCs: {:?}", seen_pcs);
    assert!(!seen_pcs.is_empty(), "Seen PCs should have some values");
}

#[test]
fn test_runtime_configuration() {
    setup();
    deploy_hex!("../tests/contracts/contract_creation_B.hex", vm, address);
    let address = Address::new(address.0);

    vm.instrument_config_mut().pcs_by_address = false;

    vm.set_account_balance(
        *OWNER,
        U256::from_str_radix("9999999999999999999999", 16).unwrap(),
    )
    .unwrap();

    // Call b.add() with some ether
    let bin = hex::decode(fn_sig_to_prefix("add()")).unwrap();
    let resp = vm.contract_call_helper(
        address,
        *OWNER,
        bin,
        U256::from_str_radix("999999", 16).unwrap(),
        None,
    );
    assert!(resp.success, "Call error {:?}", resp);

    let seen_pcs = &vm.pcs_by_address().get(&address);
    assert!(
        seen_pcs.is_none() || seen_pcs.unwrap().is_empty(),
        "No PCs by address should be recorded"
    );
}

#[test]
fn test_library_method_with_large_string() {
    deploy_hex!("../tests/contracts/VeLogo.hex", vm, address);
    let fn_sig = "tokenURI(uint256,uint256,uint256,uint256)";
    let fn_args_hex: String = repeat_with(H256::random).take(4).map(hex::encode).collect();
    let address = Address::new(address.0);

    let add_hex = format!("{}{}", fn_sig_to_prefix(fn_sig), fn_args_hex);
    let data = hex::decode(add_hex).unwrap();
    let r = vm.contract_call_helper(address, *OWNER, data, UZERO, None);
    assert!(r.success);
    r.seen_pcs
        .into_iter()
        .for_each(|e| println!("seen_pcs len: {} {}", e.0, e.1.len()));
}

#[test]
fn test_reset_storage() {
    deploy_hex!("../tests/contracts/storage.hex", vm, addr);
    let index = U256::ZERO;

    let target_value = U256::from(99u64);

    println!(
        "Setting storage for address: {:?} index: {:?} to value: {:?}",
        addr, index, target_value
    );
    let addr = Address::new(addr.0);

    let r = vm.set_storage_by_address(addr, index, target_value);

    assert!(r.is_ok(), "Set storage should succeed");

    let val = vm.get_storage_by_address(addr, index);
    assert!(val.is_ok(), "Get storage should return some data");
    assert_eq!(
        target_value,
        val.unwrap(),
        "Storage should be updated to the target value"
    );

    let r = vm.reset_storage(addr);
    println!("r: {:?}", r);
    let val = vm.get_storage_by_address(addr, index);
    println!("val: {:?}", val);
    assert!(val.is_ok(), "Get storage should return some data");
    assert_eq!(U256::ZERO, val.unwrap(), "Storage should be cleared");
}

#[test]
fn test_sha3_mapping() {
    setup();
    deploy_hex!("../tests/contracts/sha3_mapping.hex", vm, addr);
    let addr = Address::new(addr.0);

    let prefix = fn_sig_to_prefix("arrLocation(uint256,uint256,uint256)");
    let args = format!(
        "{:0>64x}{:0>64x}{:0>64x}",
        U256::from(5u64),
        U256::from(1u64),
        U256::from(256u64),
    );

    let bin = format!("{}{}", prefix, args);
    println!("bin: {}", bin);
    let bin = hex::decode(bin).unwrap();

    let resp = vm.contract_call_helper(addr, *OWNER, bin, UZERO, None);
    assert!(resp.success, "Call error {:?}", resp);
    let actual_mapping = resp.heuristics.sha3_mapping;
    println!("sha3_mappings: {:?}", actual_mapping);
    let expected_hash =
        H256::from_str("0x036b6384b5eca791c62761152d0c79bb0604c104a5fb6f4eb0703f3154bb3db0")
            .unwrap();
    let expected_key: Vec<u8> = vec![
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 5,
    ];

    assert_eq!(
        actual_mapping.get(&expected_hash),
        Some(&expected_key),
        "Mapping should be found"
    );
}

#[test]
fn test_seen_addresses() {
    setup();
    let mut vm = TinyEVM::default();
    let r = vm.set_env_field_value("block_number".into(), format!("{:0>64x}", U256::from(1u64)));
    assert!(r.is_ok(), "Set block number should succeed");

    let bytecode = include_str!("./contracts/contract_addresses_A.hex");
    let bytecode = hex::decode(bytecode).unwrap();
    let resp = vm
        .deploy_helper(*OWNER, bytecode, UZERO, None, None)
        .unwrap();
    assert!(resp.success, "Deploy contract A should succeed");
    let addr_a = Address::from_slice(&resp.data);

    println!("address A: {:?}", addr_a);

    {
        let config = vm.instrument_config_mut();
        config.record_branch_for_target_only = true;
        config.target_address = addr_a;
    }

    let bytecode = include_str!("./contracts/contract_addresses_B.hex");
    let bytecode = format!("{}{:0>64}", bytecode, addr_a.encode_hex::<String>());
    println!("Deploying contract B with bytecode: {}", bytecode);
    let bytecode = hex::decode(bytecode).unwrap();
    let resp = vm
        .deploy_helper(*OWNER, bytecode, UZERO, None, None)
        .unwrap();
    assert!(
        resp.success,
        "Deploy contract B should succeed: {}",
        String::from_utf8_lossy(&resp.data)
    );

    let addr = Address::from_slice(&resp.data);

    println!("address B: {:?}", addr);

    let prefix = fn_sig_to_prefix("getBlockNumber()");
    let args = "";

    let bin = format!("{}{}", prefix, args);

    let bin = hex::decode(bin).unwrap();

    let resp = vm.contract_call_helper(addr, *OWNER, bin, UZERO, None);
    println!("resp: {:?}", resp);
    assert!(resp.success, "Call error {:?}", resp);

    let seen = resp.heuristics.seen_addresses;
    println!("seen_addresses: {:?}", seen);

    assert!(seen.contains(&addr_a), "Contract A should be seen");
    assert!(seen.contains(&addr), "Contract B should be seen");

    assert_eq!(
        &seen[0], &addr_a,
        "Contract A should be the first in the seen addresses"
    );
}

#[test]
fn test_distance_signed() {
    setup();
    deploy_hex!("../tests/contracts/test_distance_signed.hex", vm, address);
    let address = Address::new(address.0);
    let fn_sig = "sign_distance(int256)";
    let fn_sig_hex = fn_sig_to_prefix(fn_sig);
    let input = U256::from(5);
    let fn_args_hex = format!("{:0>64x}", input);

    let expected_distances = [
        6, // distance at line 4
        7, // distance at line 6
    ];

    let fn_hex = format!("{}{}", fn_sig_hex, fn_args_hex);

    let tx_data = hex::decode(fn_hex).unwrap();

    let resp = vm.contract_call_helper(address, *OWNER, tx_data, UZERO, None);

    assert!(resp.success, "Transaction should succeed.");

    let mut buffer = [0u8; 32];
    let data = &resp.data[..32];
    buffer.copy_from_slice(data);
    let r: U256 = U256::from_be_bytes(buffer);

    assert_eq!(U256::from(2), r, "Return value should be 2");

    let missed_branches_distance = resp
        .heuristics
        .missed_branches
        .into_iter()
        .map(|b| b.distance)
        .collect::<Vec<_>>();

    println!("Missed branches distances: {:?}", missed_branches_distance);

    let failed_to_find = expected_distances
        .iter()
        .filter(|d| !(missed_branches_distance.contains(&U256::from(**d))))
        .collect::<Vec<_>>();

    println!(
        "Failed to find missed branches distances: {:?}",
        failed_to_find
    );
    assert!(
        failed_to_find.is_empty(),
        "All expected distances should be found"
    );
}

#[test]
fn test_peephole_optimized_if_equal() {
    setup();
    deploy_hex!(
        "../tests/contracts/test_peephole_optimized.hex",
        vm,
        address
    );
    let address = Address::new(address.0);

    let fn_sig = "func1(uint8)";
    let fn_sig_hex = fn_sig_to_prefix(fn_sig);
    let input = U256::from(1);
    let fn_args_hex = format!("{:0>64x}", input);

    let expected_missed_branches: (usize, usize, U256) = (166, 181, U256::from(0x2007));
    // [MissedBranch { prev_pc: 11, cond: true, dest_pc: 16, distance: 115792089237316195423570985008687907853269984665640564039457584007913129639935, address_index: 0 }, MissedBranch { prev_pc: 25, cond: false, dest_pc: 65, distance: 33, address_index: 0 }, MissedBranch { prev_pc: 42, cond: true, dest_pc: 70, distance: 1, address_index: 0 }, MissedBranch { prev_pc: 354, cond: true, dest_pc: 363, distance: 1, address_index: 0 }, MissedBranch { prev_pc: 312, cond: true, dest_pc: 317, distance: 1, address_index: 0 }, MissedBranch { prev_pc: 166, cond: true, dest_pc: 181, distance: 8199, address_index: 0 }]

    let fn_hex = format!("{}{}", fn_sig_hex, fn_args_hex);

    let tx_data = hex::decode(fn_hex).unwrap();

    let resp = vm.contract_call_helper(address, *OWNER, tx_data, UZERO, None);

    assert!(resp.success, "Transaction should succeed.");

    println!("resp: {resp}");

    let found_missed_branch_at_func1 = resp.heuristics.missed_branches.iter().any(
        |MissedBranch {
             prev_pc,
             dest_pc,
             distance,
             ..
         }| { (*prev_pc, *dest_pc, *distance) == expected_missed_branches },
    );
    assert!(
        found_missed_branch_at_func1,
        "Missed branch at func1 should be found"
    );
}

#[test]
fn test_fork() -> Result<()> {
    setup();
    if env::var("TINYEVM_CI_TESTS").is_ok() {
        warn!("Skipping tests on CI");
        return Ok(());
    }

    let fork_url = Some("https://eth.llamarpc.com".into());
    let block_id = Some(17869485);

    let mut evm = TinyEVM::new(fork_url, block_id)?;

    let sender = Some("0xC6CDE7C39eB2f0F0095F41570af89eFC2C1Ea828".into());
    let contract = "dAC17F958D2ee523a2206206994597C13D831ec7".into();
    // balanceOf("0xf977814e90da44bfa03b6295a0616a897441acec")
    let data =
        Some("70a08231000000000000000000000000f977814e90da44bfa03b6295a0616a897441acec".into());
    let value = None;
    let result = evm.contract_call(contract, sender, data, value)?;

    assert!(result.success, "Call error {:?}", result);

    println!("result: {:?}", result);

    let balance: [u8; 32] = result.data.as_slice().try_into()?;
    let balance = U256::from_be_bytes(balance);

    assert_eq!(U256::from_str_radix("2691791472364000", 10)?, balance,);

    Ok(())
}

#[test]
fn test_call_forked_contract_from_local_contract() -> Result<()> {
    setup();
    if env::var("TINYEVM_CI_TESTS").is_ok() {
        warn!("Skipping tests on CI");
        return Ok(());
    }

    let bin = include_str!("../tests/contracts/test_fork.hex");
    let fork_url = Some("https://bscrpc.com".into());
    let block_id = Some(0x1e08bd6);

    let mut evm = TinyEVM::new(fork_url, block_id)?;

    let resp = evm.deploy(bin.into(), None)?;

    assert!(resp.success, "Deploy error {:?}", resp);

    let wbnb_address: String = "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c".into();

    let _busd_address: String = "0x55d398326f99059ff775485246999027b3197955".into();

    let _pancake_address: String = "0x10ed43c718714eb63d5aa57b78b54704e256024e".into();

    let sender: String = "0x18f4ea83d0bd40e75c8222255bc855a974568dd4".into();

    let init_balance = BigInt::from_str("998888888888888888888").unwrap();

    evm.set_balance(sender.clone(), init_balance).unwrap();

    let value = BigInt::from_str("18888888888888888888").unwrap();

    println!("Sender sending ether to WBNB");

    let resp = evm.contract_call(wbnb_address, Some(sender), None, Some(value))?;

    assert!(resp.success, "Call error {:?}", resp);

    let block_number = evm.get_env_value_by_field("block_number".into()).unwrap();

    let block_timestamp = evm
        .get_env_value_by_field("block_timestamp".into())
        .unwrap();

    println!(
        "block_number: {} block_timestamp: {}",
        block_number, block_timestamp
    );

    let remote_addresses = evm.get_forked_addresses()?;
    let remote_addresses = remote_addresses
        .iter()
        .map(|a| a.to_string())
        .collect::<HashSet<String>>();
    let expected_addresses = HashSet::from([
        "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c".into(),
        "0x72b61c6014342d914470ec7ac2975be345796c2b".into(),
    ]);
    assert_eq!(expected_addresses, remote_addresses);

    let remote_storage_indices =
        evm.get_forked_slots("0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c".into())?;

    assert_eq!(
        ruint_u256_to_bigint(
            &U256::from_str_radix(
                "5aca9f8e8ddd72ad4b96de957d6bd49b602eab95954cc54154e3c000532f36a2",
                16
            )
            .unwrap()
        ),
        *remote_storage_indices.first().unwrap()
    );

    Ok(())
}

#[test]
fn test_sturdy_hack() -> Result<()> {
    setup();
    if env::var("TINYEVM_CI_TESTS").is_ok() {
        warn!("Skipping tests on CI");
        return Ok(());
    }

    let bin = include_str!("../tests/contracts/SturdyFinance_ReadonlyRE.hex");
    let fork_url = Some("https://eth.llamarpc.com".into());
    let block_id = Some(17_460_609);

    let mut evm = TinyEVM::new(fork_url, block_id)?;

    let resp = evm.deploy(bin.into(), None)?;

    assert!(resp.success, "Deploy error {:?}", resp);

    let attacker = format!("0x{:0>40}", hex::encode(&resp.data));

    let sender: String = "0x18f4ea83d0bd40e75c8222255bc855a974568dd4".into();

    let init_balance = BigInt::from_str("10998888888888888888888").unwrap();

    let weth_address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
    let balance_of_query_data = format!("{:0<32}{}", "70a08231", &attacker[2..]);
    let sender_start_weth_balance = evm
        .contract_call(
            weth_address.into(),
            None,
            Some(balance_of_query_data.clone()),
            None,
        )
        .map(|resp| {
            let balance: [u8; 32] = resp.data.as_slice().try_into().unwrap();
            U256::from_be_bytes(balance)
        })?;

    evm.set_balance(sender.clone(), init_balance).unwrap();

    let data = "ca1ba028".into(); // testExploit()
    let _resp = evm.contract_call(attacker, Some(sender), Some(data), None)?;

    let sender_end_weth_balance = evm
        .contract_call(weth_address.into(), None, Some(balance_of_query_data), None)
        .map(|resp| {
            let balance: [u8; 32] = resp.data.as_slice().try_into().unwrap();
            U256::from_be_bytes(balance)
        })?;

    assert!(
        sender_end_weth_balance.gt(&sender_start_weth_balance),
        "Exploit failed"
    );

    Ok(())
}

#[test]
fn test_events() -> Result<()> {
    let bin = include_str!("../tests/contracts/TestEvents.hex");
    let mut vm = TinyEVM::default();
    let resp = vm.deploy(bin.into(), None)?;
    assert!(resp.success, "Deploy error {:?}", resp);
    let contract = format!("0x{:0>40}", hex::encode(&resp.data));
    println!("Contract address: {}", contract);
    let data = format!(
        "{}{:064x}",
        "1401d2b5", // makeEvent(3232)
        U256::from(3232)
    );
    let resp = vm.contract_call(contract.clone(), None, Some(data.clone()), None)?;
    assert!(resp.success, "Call error {:?}", resp);
    assert!(resp.events.is_empty(), "Expecting no events");
    assert!(resp.traces.is_empty(), "Expecting no call traces");

    vm.set_evm_tracing(true);
    let resp = vm.contract_call(contract.clone(), None, Some(data), None)?;

    assert!(resp.success, "Call error {:?}", resp);
    assert!(resp.events.len() == 1, "Expecting one event");
    assert!(resp.traces.len() == 1, "Expecting one call trace");
    let event = resp.events.first().unwrap();
    assert_eq!(contract, event.address);
    assert_eq!(
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef", // Keccak-256 encoding of `Transfer(address,address,uint256)`
        event.topics[0]
    );

    Ok(())
}
