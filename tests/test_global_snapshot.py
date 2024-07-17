import tinyevm
from Crypto.Hash import keccak
import threading
import sys
from datetime import datetime
import json
from eth_abi import encode
from memory_profiler import memory_usage, profile
import gc
import pytest

# @pytest.fixture
# def benchmark_memory():
#     def benchmark_memory_fn(func, *args, **kwargs):
#         mem_usage = memory_usage((func, args, kwargs), interval=0.1, timeout=1)
#         return max(mem_usage) - min(mem_usage)
#     return benchmark_memory_fn

def handle_exception(exc_type, exc_value, exc_traceback):
    if issubclass(exc_type, KeyboardInterrupt):
        sys.__excepthook__(exc_type, exc_value, exc_traceback)
        return

    print("Uncaught exception", exc_type, exc_value)
    sys.exit(1)

sys.excepthook = handle_exception


def fn_sig(sig):
    k = keccak.new(digest_bits=256)
    k.update(sig.encode())
    return k.hexdigest()[:8]

def tprint(*args):
    # print(datetime.now(), threading.current_thread().name, *args)
    pass

def encode_balance_of(address):
    # Compute the method id for the balanceOf function
    method_id = "0x70a08231"  # function signature for balanceOf(address)

    # ABI encode the address
    address_param = encode('address', address).hex()

    # Concatenate the method id and the address parameter
    data = method_id + address_param

    return data

tevm = tinyevm.TinyEVM()

def transfer_1000(contract, sender):
    fn = fn_sig('transfer(address,uint256)')
    data = encode(['address', 'uint256'], ['0x44Eadb1b1288F4883F2166846800335bfFa290be', 1000]).hex()
    resp = tevm.contract_call(contract, sender, fn + data, None)
    assert resp.success


def deploy_contract(salt=None, owner='0x388C818CA8B9251b393131C08a736A67ccB19297'):
    """
    Deploy and check the balance of the contract
    """
    # generated with
    # solc  --combined-json abi,bin,bin-runtime,srcmap,srcmap-runtime  tests/contracts/snapshots.sol > tests/contracts/snapshots.compiled.json
    with open('tests/contracts/snapshots.compiled.json') as f:
        out = json.load(f)

    binary = out['contracts']['tests/contracts/snapshots.sol:Test']['bin']

    tevm.set_balance(owner, 0xffff0000000000000000000000000000000000000000000000000000000000ff)

    salt = None
    data = ''
    value = None
    init_value = None
    resp = tevm.deterministic_deploy(binary, salt, owner, data, value, init_value)
    tprint('Deployment resp: {}'.format(resp))

    assert resp.success

    contract = bytes(resp.data).hex()
    tprint('Contract deployed to: {}'.format(contract or 'EMPTY'))

    return contract


def run_global_snapshot(keep_snapshot: bool, x_fns: list, disable_snapshot=False, take_snapshot_after_each_tx=False):
    owner = '0x1e209e340405D4211a3185f97628179917883505'
    init_count_value = 1

    # deploy some contracts to populate a few more acounts
    deploy_contract(salt='0x1fff00000000000000000000000000000000000000000000000000000000eeff')
    deploy_contract(salt='0x1fff000000000000000000000000000000000000000000000000000000cceeff')
    deploy_contract(salt='0x1fff000000000000000000000000000000000000000000000000000000aaeeff')

    # deploy and take a global snapshot
    contract = deploy_contract(owner=owner)
    snapshot_id = None
    if not disable_snapshot:
        snapshot_id = tevm.take_global_snapshot()

    # make sure we've deployed successfully by checking the initial states
    call_data = fn_sig('counter()')
    resp = tevm.contract_call(contract, None, call_data, None)
    tprint(f"counter() resp: {resp.data}")
    assert resp.success
    count =  int.from_bytes(bytes(resp.data), 'big')
    assert count == init_count_value

    # make some transactions
    for (fn, expected_return_value) in x_fns:
        call_data = fn_sig(fn)
        resp = tevm.contract_call(contract, owner, call_data, None)
        assert resp.success
        count =  int.from_bytes(bytes(resp.data), 'big')
        if expected_return_value is not None:
            assert count == expected_return_value
        if take_snapshot_after_each_tx and not disable_snapshot:
            tevm.take_global_snapshot()

    # restore the snapshot
    if not disable_snapshot:
        for _ in range(1000):
            tevm.restore_global_snapshot(snapshot_id, keep_snapshot)

    # check the global state reverted to previous state
    call_data = fn_sig('counter()')
    resp = tevm.contract_call(contract, owner, call_data, None)
    assert resp.success
    count =  int.from_bytes(bytes(resp.data), 'big')
    if not disable_snapshot:
        assert count == init_count_value



x_fns_fastseq = [('fast_seq()', None),] * 1000

x_fns_slowseq = [('slow_seq()', None), ] * 100

@pytest.mark.parametrize("x_fns, disable_snapshot, name", [
    (x_fns_fastseq, False, "fastseq, plus take and restore snapshot"),
    (x_fns_slowseq, False, "slowseq, plus take and restore snapshot"),
    (x_fns_fastseq, True, "fastseq"),
    (x_fns_slowseq, True, "slowseq"),
])
def test_global_snapshot(benchmark, x_fns, disable_snapshot, name):
    global tevm
    tevm = tinyevm.TinyEVM()
    def run():
        run_global_snapshot(True, x_fns, disable_snapshot=disable_snapshot)
    benchmark(run)



@profile
def test_compare_memory():
    x_fns_fastseq = [('fast_seq()', 1 + 5 * i) for i in range(1, 1000)]
    x_fns_slowseq = [('slow_seq()', 1 + 5 * 50 * i) for i in range(1, 100)]
    global tevm


    # Running 100 slowseq without taking snapshots
    tevm = tinyevm.TinyEVM()
    gc.collect()
    run_global_snapshot(True, x_fns_slowseq, disable_snapshot=True)
    gc.collect()

    # Running 1000 fastseq without taking snapshots
    tevm = tinyevm.TinyEVM()
    gc.collect()
    run_global_snapshot(True, x_fns_fastseq, disable_snapshot=True)
    gc.collect()

    # Running slowseq taking 100 snapshots
    tevm = tinyevm.TinyEVM()
    gc.collect()
    run_global_snapshot(True, x_fns_slowseq, take_snapshot_after_each_tx=True)
    gc.collect()

    # Running fastseq taking 1000 snapshots
    tevm = tinyevm.TinyEVM()
    gc.collect()
    run_global_snapshot(True, x_fns_fastseq, take_snapshot_after_each_tx=True)
    gc.collect()
