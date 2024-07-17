import tinyevm
import unittest
from Crypto.Hash import keccak
import threading
import sys
from datetime import datetime
from concurrent.futures import ProcessPoolExecutor


salt = None

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
    print(datetime.now(), threading.current_thread().name, *args)

def run_test():
    tevm = tinyevm.TinyEVM()

    contract_bytecode = open('tests/contracts/C.hex').read()
    owner = '0x388C818CA8B9251b393131C08a736A67ccB19297'
    data = None
    value = None
    init_value = 0x223312323

    tevm.set_balance(owner, 0xffff0000000000000000000000000000000000000000000000000000000000ff)
    balance = tevm.get_balance(owner)
    tprint('balance before deployment: {}'.format(balance))

    resp = tevm.deterministic_deploy(contract_bytecode, salt, owner, data, value, init_value)
    tprint('Deployment resp: {}'.format(resp))

    assert resp.success

    bugs = list(resp.bug_data)
    for b in bugs:
        tprint('Bug: {} {}'.format(b.bug_type['type'], b.position))

    seen_pcs = resp.seen_pcs
    tprint('Seen PCs: {}'.format(seen_pcs.keys()))
    for addr in seen_pcs.keys():
        pcs = seen_pcs.get(addr)
        tprint('Addr: {} PCs: {}'.format(addr, len(pcs)))

    assert resp.success

    address = bytes(resp.data).hex()
    tprint('Contract deployed to: {}'.format(address or 'EMPTY'))

    fn = fn_sig('name()')
    resp = tevm.contract_call(address, None, fn, None)
    assert resp.success
    assert bytes(resp.data[64:]).decode().startswith('CToken')


def run_infinite_loop():
    tprint('Starting infinite loop')
    tevm = tinyevm.TinyEVM()

    contract_bytecode = open('tests/contracts/infinite_loop_Test.hex').read()
    owner = '0x388C818CA8B9251b393131C08a736A67ccB19297'
    data = None
    value = None
    init_value = 0x223312323

    tevm.set_balance(owner, 0xffff0000000000000000000000000000000000000000000000000000000000ff)
    balance = tevm.get_balance(owner)
    tprint('balance before deployment: {}'.format(balance))

    # todo update response object
    resp = tevm.deterministic_deploy(contract_bytecode, salt, owner, data, value, init_value)
    tprint('Deployment resp: {}'.format(resp))

    assert resp.success
    address = bytes(resp.data).hex()

    fn = fn_sig('test1(int256)')
    data = '0' * 64
    resp = tevm.contract_call(address, None, fn + data, None)
    assert not resp.success


class TestMultiThreading(unittest.TestCase):
    test_multithreading = 0
    test_multiprocessing = 1

    def test_multithread(self):
        if not self.test_multithreading:
            return
        threads = []
        for _ in range(8):
            t = threading.Thread(target=run_infinite_loop)
            threads.append(t)
            t.start()
        for t in threads:
            t.join()

    def test_multprocessing(self):
        if not self.test_multiprocessing:
            return

        with ProcessPoolExecutor() as executor:
            for _ in range(8):
                executor.submit(run_infinite_loop)
