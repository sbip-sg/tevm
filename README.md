# tinyevm

Dynamic library providing API to EVM executor


## Start development

* Clone this repository

``` bash
git clone git@github.com:sbip-sg/tevm.git
```

* Run test in tinyevm

- For unit tests, run

``` bash
make test
```

- For Rust APIs benchmark test, run

``` bash
make bench
```


## How to test the underline REVM

<!-- TODO add documentation on how to disable the instrumentation completely -->


## How to contribute

* Clone this repository

``` bash
git clone git@github.com:sbip-sg/tinyevm.git
```

* Create a fork for the task you are working on


``` bash
cd tinyevm
git checkout -b [name-of-your-fork]
```

Continue working on this fork, commit as often as you like. When you
feel like it's ready to merge, create a PR on github to request
merging your fork to `develop`.

After PR is appproved, please merge your changes to `develop`:
  * If there is no conflict, you can merge the changes directly on github
  * In case there are any conflicts preventing the merge, follow these steps
    * `git fetch --all`
    * `git merge origin/develop`
    * Resolve all the conflicts, after testing, make another commit and you can continue merging the changes.
  * After squashing and mering the PR, delete your branch.


## Sample usage

``` python
# todo add a python example
```

You can find example usage in Python in the `example/` folder.

Example files:

``` text
example/
├── C.hex                          # Bytecode for contract C as hex
├── example-bug-detected.py        # Sample bug-detection in Python
├── example.py                     # Sample code in Python
├── int_cast_0.5.0.hex             # Bytecode for contract IntCast as hex compiled with solc version 0.5.0
├── int_cast_0.8.0.hex             # Bytecode for contract IntCast as hex compiled with solc version 0.8.0
├── int_cast.sol                   # Source code for the IntCast contract
├── tx_origin.hex                  # Bytecode for contract TxOrigin as hex compiled with solc version 0.8.12
├── tx_origin.sol                  # Source code for the TxOrigin contract
├── calls_trace.hex                # Bytecode for contract `sample` as hex compiled with solc version 0.7.0
├── call_trace.sol                 # Source code for the `sample` contract for call tracing test
├── block_hash.sol                 # Source code for the `BHash` contract for blockhash test
├── block_hash.hex                 # Bytecode for contract `BHash` as hex compiled with solc version 0.7.0
├── test_tod.sol                   # Source code for testing SSLOAD/SSTORE instrumentation
├── test_tod.hex                   # Bytecode for the tet contract in test_tod.sol
├── balance.sol                    # Source code for the `Balance` contract for balance get/set test
├── balance.hex                    # Bytecode for contract `Balance` as hex compiled with solc version 0.7.0
├── self_destruct.sol              # Source code for the `Des` contract for SELFDESTRUCT detection
├── self_destruct.hex              # Bytecode for contract `Des` as hex compiled with solc 0.8.10
├── contract_creation.sol          # Source code for testing code coverage calculated with program counter
├── contract_creation_B.hex        # Bytecode for contract `B` in contract_creation.sol
├── deploy_with_args_and_value.hex # Bytecode for contract DeployWithArgsAndValue as hex compiled with solc version 0.7.0
├── deploy_with_args_and_value.sol # Source code for the `DeployWithArgsAndValue` contract for testing contract with constructor arguments
├── ...
```

The contract `C` used in this example is compiled from [data_structures.sol](https://github.com/cassc/evm-play/tree/main/contracts).


## Python Module

### Local development

* Clean up the previous build

    ``` bash
    make clean
    ```

* Install development dependencies

    ``` bash
    pip install -r requirements-dev.txt
    ```

* Build and install the Python library

    ``` bash
    maturing develop
    ```
* Run test, this will run the Rust test first, install the development version of TinyEVM and run the Python tests

    ``` bash
    make test
    ```

### Build and release Python library

* The following command will build a `whl` file inside `target/wheels` folder
    ``` bash
    maturin build --release
    # or to compile for python 3.9
    maturin build --release -i 3.9
    ```
* You can install this file with `pip` command
    ``` bash
    pip install target/wheels/*.whl --force-reinstall
    ```

Here's the corrected version:

### Cache Web3 Requests to Redis

By default, requests to the Web3 endpoints are cached in the file system. If you wish to use Redis as the cache, compile with the `provider_cache_redis` flag:

```bash
maturin build --release -i 3.9 --cargo-extra-args="--features provider_cache_redis"
```

Additionally, you must set the environment variable `TINYEVM_REDIS_NODE` to a valid Redis endpoint.

# Benchmarks

## Global snapshot benchmarks

To run the test:

``` bash
maturing develop --release
pytest -s --show-capture all    tests/test_global_snapshot.py
```


* Results from legacy Tinyevm (using instrumented REVM) commit `839b0b8822702b49096bc2bf3f092c7a1aab13a3`:

``` text
============================================================================================================= test session starts ==============================================================================================================
platform linux -- Python 3.9.7, pytest-7.2.2, pluggy-1.0.0
benchmark: 4.0.0 (defaults: timer=time.perf_counter disable_gc=False min_rounds=5 min_time=0.000005 max_time=1.0 calibration_precision=10 warmup=False warmup_iterations=100000)
rootdir: /home/garfield/projects/sbip-sg/tinyevm
plugins: cov-4.1.0, web3-5.31.2, hypothesis-6.82.0, anyio-3.6.2, benchmark-4.0.0, xdist-3.3.1
collected 5 items

...
...

------------------------------------------------------------------------------------------------------------- benchmark: 4 tests ------------------------------------------------------------------------------------------------------------
Name (time in ms)                                                             Min                 Max                Mean             StdDev              Median                IQR            Outliers     OPS            Rounds  Iterations
---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
test_global_snapshot[x_fns2-True-fastseq no snapshot]                    118.6619 (1.0)      124.2569 (1.0)      122.1113 (1.0)       1.7266 (1.0)      122.0048 (1.0)       1.8421 (1.0)           2;1  8.1892 (1.0)           8           1
test_global_snapshot[x_fns0-False-fastseq take and restore snapshot]     123.2069 (1.04)     131.6260 (1.06)     126.0777 (1.03)      3.4155 (1.98)     124.7658 (1.02)      5.4766 (2.97)          2;0  7.9316 (0.97)          8           1
test_global_snapshot[x_fns1-False-slowseq take and restore snapshot]     448.7344 (3.78)     490.6087 (3.95)     466.8714 (3.82)     18.1452 (10.51)    456.7645 (3.74)     29.2119 (15.86)         1;0  2.1419 (0.26)          5           1
test_global_snapshot[x_fns3-True-slowseq no snapshot]                    453.6979 (3.82)     516.7671 (4.16)     473.7815 (3.88)     25.0584 (14.51)    463.8226 (3.80)     25.1137 (13.63)         1;0  2.1107 (0.26)          5           1
---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
```

* Results from the current branch:

``` text
------------------------------------------------------------------------------------------------------------ benchmark: 4 tests ------------------------------------------------------------------------------------------------------------
Name (time in ms)                                                             Min                 Max                Mean            StdDev              Median               IQR            Outliers      OPS            Rounds  Iterations
--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
test_global_snapshot[x_fns2-True-fastseq no snapshot]                     95.1790 (1.0)      101.4989 (1.0)       97.1787 (1.0)      2.3811 (1.0)       96.0393 (1.0)      2.9408 (1.0)           2;0  10.2903 (1.0)          10           1
test_global_snapshot[x_fns0-False-fastseq take and restore snapshot]     101.7576 (1.07)     111.5388 (1.10)     106.1005 (1.09)     2.9359 (1.23)     106.2893 (1.11)     4.1448 (1.41)          3;0   9.4250 (0.92)         10           1
test_global_snapshot[x_fns1-False-slowseq take and restore snapshot]     315.4810 (3.31)     337.3184 (3.32)     327.1796 (3.37)     8.0853 (3.40)     329.3743 (3.43)     9.8087 (3.34)          2;0   3.0564 (0.30)          5           1
test_global_snapshot[x_fns3-True-slowseq no snapshot]                    315.5748 (3.32)     328.8318 (3.24)     321.7635 (3.31)     5.0368 (2.12)     321.1631 (3.34)     7.1114 (2.42)          2;0   3.1079 (0.30)          5           1
--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
```


## Changes

### 1.0.0

Breaking changes:
- `deterministic_deploy` signature is now changed to `deterministic_deploy(contract_deploy_code, owner=None, data=None, value=None, init_value=None, deploy_to_address=None)`, i.e., `salt` is removed. [Related to [REVM-1182](https://github.com/bluealloy/revm/issues/1182)]. If you want to similate the previous behaviour, it's suggested to use the `deploy_to_address` parameter to specify the address where the contract will be deployed.
- `enabled` in `REVMConfig` is removed. To disable all instrumentation, use
