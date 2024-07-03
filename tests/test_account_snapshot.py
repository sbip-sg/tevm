import tinyevm
from Crypto.Hash import keccak
import threading
import sys
from datetime import datetime
import json
from eth_abi import encode


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

def encode_balance_of(address):
    # Compute the method id for the balanceOf function
    method_id = "0x70a08231"  # function signature for balanceOf(address)

    # ABI encode the address
    address_param = encode('address', address).hex()

    # Concatenate the method id and the address parameter
    data = method_id + address_param

    return data

tevm = tinyevm.TinyEVM()

# constructor args
_initialSupply = 10000*10**18
_name = "Tether USD"
_symbol = "TUSD"
_decimals = 18


def transfer_1000(contract, sender):
    fn = fn_sig('transfer(address,uint256)')
    data = encode(['address', 'uint256'], ['0x44Eadb1b1288F4883F2166846800335bfFa290be', 1000]).hex()
    resp = tevm.contract_call(contract, sender, fn + data, None)
    assert resp.success


def deploy_contract(salt=None, owner='0x388C818CA8B9251b393131C08a736A67ccB19297'):
    """
    Deploy and check the balance of the contract
    """
    with open('tests/contracts/TetherToken_solc_output.json') as f:
        out = json.load(f)

    binary = out['contracts']['TetherToken.sol']['TetherToken']['evm']['bytecode']['object']


    data = encode(['uint256', 'string', 'string', 'uint256'], [_initialSupply, _name, _symbol, _decimals]).hex()

    value = None
    init_value = 0x223312323

    tevm.set_balance(owner, 0xffff0000000000000000000000000000000000000000000000000000000000ff)

    resp = tevm.deterministic_deploy(binary, salt, owner, data, value, init_value)
    tprint('Deployment resp: {}'.format(resp))

    assert resp.success

    contract = bytes(resp.data).hex()
    tprint('Contract deployed to: {}'.format(contract or 'EMPTY'))

    return contract

def redeploy_contract(salt=None, owner='0x388C818CA8B9251b393131C08a736A67ccB19297'):
    """
    Deploy and check the balance of the contract
    """

    contract = deploy_contract(salt, owner)
    data_balance_check = fn_sig('balanceOf(address)') + encode(['address'], [owner]).hex()

    resp = tevm.contract_call(contract, owner, data_balance_check, None)

    assert resp.success
    balance =  int.from_bytes(bytes(resp.data), 'big')
    assert balance == _initialSupply

    transfer_1000(contract, owner)

    resp = tevm.contract_call(contract, owner, data_balance_check, None)

    assert resp.success
    balance =  int.from_bytes(bytes(resp.data), 'big')
    assert balance == _initialSupply - 1000



def reset_contract_call(contract, owner):
    """
    Take a snapshot of an account and restore it
    """
    data_balance_check = fn_sig('balanceOf(address)') + encode(['address'], [owner]).hex()

    resp = tevm.contract_call(contract, owner, data_balance_check, None)

    assert resp.success
    balance =  int.from_bytes(bytes(resp.data), 'big')
    assert balance == _initialSupply

    tevm.take_snapshot(contract)

    transfer_1000(contract, owner)

    resp = tevm.contract_call(contract, owner, data_balance_check, None)
    assert _initialSupply - 1000 == int.from_bytes(bytes(resp.data), 'big')


    random_address = '0x253397db4016dE1983D29f7DEc2901c54dB81A22'
    tevm.copy_snapshot(contract, random_address)

    tevm.restore_snapshot(contract)

    # Balance should be restored
    resp = tevm.contract_call(contract, owner, data_balance_check, None)
    assert _initialSupply == int.from_bytes(bytes(resp.data), 'big')

    # Balance should also match in the copied address
    resp = tevm.contract_call(random_address, owner, data_balance_check, None)
    assert _initialSupply == int.from_bytes(bytes(resp.data), 'big')



def test_snapshot(benchmark):
    owner = '0x9C33eaCc2F50E39940D3AfaF2c7B8246B681A374'
    contract = deploy_contract(salt='0x1fff0000000000000000000000000000000000000000000000000000000000ff', owner=owner)
    def reset_contract():
        reset_contract_call(contract, owner)

    benchmark(reset_contract)

def test_redeploy(benchmark):
    benchmark(redeploy_contract)
