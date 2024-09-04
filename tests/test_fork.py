import tinyevm
import unittest
from Crypto.Hash import keccak

def fn_sig(sig):
    k = keccak.new(digest_bits=256)
    k.update(sig.encode())
    return k.hexdigest()[:8]

def tprint(*args):
    print('>>>> ', end='')
    print(*args, flush=True)

class TestTinyEVM(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # tinyevm.enable_tracing()
        pass

    def test_get_balance_from_fork(self):
        fork_url = "https://eth.llamarpc.com"
        tevm = tinyevm.TinyEVM(fork_url, 17890805)

        assert 1378414300424348501 == tevm.get_balance('0x8ee335785a9c08219CEf04d46f1f01865F102Bf4')
