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
        tinyevm.enable_tracing()

    def test_get_set_balance(self):
        tevm = tinyevm.TinyEVM()
        assert tevm.get_balance('0x388C818CA8B9251b393131C08a736A67ccB19297') == 0
        tevm.set_balance('0x388C818CA8B9251b393131C08a736A67ccB19297', 9999)
        assert tevm.get_balance('0x388C818CA8B9251b393131C08a736A67ccB19297') == 9999


    def test_get_set_code(self):
        tevm = tinyevm.TinyEVM()
        code = open('tests/contracts/C_deployed.hex').read()
        assert tevm.get_code('0x388C818CA8B9251b393131C08a736A67ccB19297') == ''
        tevm.set_code('0x388C818CA8B9251b393131C08a736A67ccB19297', code)
        # EVM appends 0x00 to the end of the code, hence using start_with instead of equal
        assert tevm.get_code('0x388C818CA8B9251b393131C08a736A67ccB19297').startswith(code)

    def test_get_set_env_field(self):
        tevm = tinyevm.TinyEVM()

        # block_number
        assert tevm.get_env_value_by_field('block_number') == '0x0000000000000000000000000000000000000000000000000000000000000000'
        tevm.set_env_field_value('block_number', '0x00000000000000000000000000000000000000000000000000000000000000ff')
        assert tevm.get_env_value_by_field('block_number') == '0x00000000000000000000000000000000000000000000000000000000000000ff'

        # origin
        assert tevm.get_env_value_by_field('origin') == '0x0000000000000000000000000000000000000000'
        tevm.set_env_field_value('origin', '0xafe87013dc96ede1e116a288d80fcaa0effe5fe5')
        assert tevm.get_env_value_by_field('origin') == '0xafe87013dc96ede1e116a288d80fcaa0effe5fe5'

    def test_get_change_tx_gas_limit(self):
        tevm = tinyevm.TinyEVM()
        tevm.tx_gas_limit = 100

        assert tevm.tx_gas_limit == 100, 'default_tx_gas_limit should be changed to 100'
        contract_bytecode = open('tests/contracts/C.hex').read()
        owner = '0x388C818CA8B9251b393131C08a736A67ccB19297'
        data = None
        value = None
        init_value = 0x223312323

        try:
            tevm.deterministic_deploy(contract_bytecode, owner, data, value, init_value)
        except RuntimeError as e:
            tprint('Expected error: {}'.format(e))
            assert 'OutOfGas' in str(e), 'should raise out of gas error'


    def test_instrument_config(self):
        tevm = tinyevm.TinyEVM()

        # get default config
        config = tevm.get_instrument_config()
        assert config.enabled
        assert config.target_address == '0x0000000000000000000000000000000000000000'

        config.target_address = '0x388C818CA8B9251b393131C08a736A67ccB19297'
        tevm.configure(config)

        assert config.target_address == '0x388C818CA8B9251b393131C08a736A67ccB19297'

    def test_deployment(self):
        tevm = tinyevm.TinyEVM()

        contract_bytecode = open('tests/contracts/C.hex').read()
        salt = None
        owner = '0x388C818CA8B9251b393131C08a736A67ccB19297'
        data = None
        value = None
        init_value = 0x223312323

        tevm.set_balance(owner, 0xffff0000000000000000000000000000000000000000000000000000000000ff)
        balance = tevm.get_balance(owner)
        tprint('balance before deployment: {}'.format(balance))

        # todo update response object
        resp = tevm.deterministic_deploy(contract_bytecode, owner, data, value, init_value)
        tprint('Deployment resp: {}'.format(resp))

        bugs = list(resp.bug_data)
        for b in bugs:
            tprint('Bug: {} {}'.format(b.bug_type['type'], b.position))

        assert resp.success

        address = bytes(resp.data).hex()
        tprint('Addr: {} PCs: {}'.format(address, resp.pcs_by_address(address)))

        tprint('Contract deployed to: {}'.format(address or 'EMPTY'))

        fn = fn_sig('name()')
        resp = tevm.contract_call(address, None, fn, None)
        tprint('Contract name: {}'.format(bytes(resp.data[64:]).decode()))

        heuristics = resp.heuristics
        tprint('Heuristics: {}'.format(heuristics))
