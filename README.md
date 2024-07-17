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

* Using the naive implementation which clones the whole data structure

``` bash
============================================================================================================= test session starts ==============================================================================================================
platform linux -- Python 3.9.7, pytest-7.2.2, pluggy-1.0.0
benchmark: 4.0.0 (defaults: timer=time.perf_counter disable_gc=False min_rounds=5 min_time=0.000005 max_time=1.0 calibration_precision=10 warmup=False warmup_iterations=100000)
rootdir: /home/garfield/projects/sbip-sg/tinyevm
plugins: cov-4.1.0, web3-5.31.2, hypothesis-6.82.0, anyio-3.6.2, benchmark-4.0.0, xdist-3.3.1
collected 7 items

tests/test_global_snapshot.py ......Filename: /home/garfield/projects/sbip-sg/tinyevm/tests/test_global_snapshot.py

Line #    Mem usage    Increment  Occurrences   Line Contents
=============================================================
   153     97.3 MiB     97.3 MiB           1   @profile
   154                                         def test_compare_memory():
   155     97.3 MiB      0.0 MiB        1002       x_fns_fastseq = [('fast_seq()', 1 + 5 * i) for i in range(1, 1000)]
   156     97.3 MiB      0.0 MiB         102       x_fns_slowseq = [('slow_seq()', 1 + 5 * 50 * i) for i in range(1, 100)]
   157                                             global tevm
   158
   159                                             # Running fastseq taking 100 snapshots
   160     97.3 MiB      0.0 MiB           1       tevm = tinyevm.TinyEVM()
   161     97.3 MiB      0.0 MiB           1       gc.collect()
   162    363.6 MiB    266.2 MiB           1       run_global_snapshot(True, x_fns_fastseq, take_snapshot_after_each_tx=True)
   163
   164                                             # Running slowseq taking 100 snapshots
   165    217.3 MiB   -146.3 MiB           1       tevm = tinyevm.TinyEVM()
   166    217.3 MiB      0.0 MiB           1       gc.collect()
   167    229.4 MiB     12.1 MiB           1       run_global_snapshot(True, x_fns_slowseq, take_snapshot_after_each_tx=True)
   168
   169                                             # Running 1000 fastseq without taking snapshots
   170    150.0 MiB    -79.4 MiB           1       tevm = tinyevm.TinyEVM()
   171    150.0 MiB      0.0 MiB           1       gc.collect()
   172    150.0 MiB      0.0 MiB           1       run_global_snapshot(True, x_fns_fastseq, disable_snapshot=True)
   173
   174                                             # Running 100 slowseq without taking snapshots
   175    150.0 MiB      0.0 MiB           1       tevm = tinyevm.TinyEVM()
   176    150.0 MiB      0.0 MiB           1       gc.collect()
   177    150.0 MiB      0.0 MiB           1       run_global_snapshot(True, x_fns_slowseq, disable_snapshot=True)


.


------------------------------------------------------------------------------------------------------------------------------------ benchmark: 6 tests -----------------------------------------------------------------------------------------------------------------------------------
Name (time in us)                                                                                               Min                   Max                  Mean              StdDev                Median                 IQR            Outliers         OPS            Rounds  Iterations
-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
test_global_snapshot[False-x_fns4-True-fastseq don't take snapshot]                                        380.7210 (1.0)        864.9290 (1.0)        414.0279 (1.0)       47.1819 (1.0)        399.9720 (1.0)       14.6963 (1.0)       126;194  2,415.2963 (1.0)        1917           1
test_global_snapshot[False-x_fns1-False-fastseq take and restore snapshot, don't keep after restore]       395.2560 (1.04)     1,188.2490 (1.37)       432.9322 (1.05)      71.7241 (1.52)       408.4960 (1.02)      25.4053 (1.73)      165;235  2,309.8306 (0.96)       1899           1
test_global_snapshot[True-x_fns0-False-fastseq take and restore snapshot, keep after restore]              405.6830 (1.07)     1,157.2750 (1.34)       491.9984 (1.19)     129.4663 (2.74)       421.2585 (1.05)     100.5715 (6.84)        99;86  2,032.5268 (0.84)        448           1
test_global_snapshot[False-x_fns5-True-slowseq don't take snapshot]                                      4,965.2520 (13.04)    7,654.7660 (8.85)     5,348.2192 (12.92)    419.9397 (8.90)     5,206.4350 (13.02)    247.7357 (16.86)       12;12    186.9781 (0.08)        173           1
test_global_snapshot[False-x_fns3-False-slowseq take and restore snapshot, don't keep after restore]     4,989.4110 (13.11)    9,267.5720 (10.71)    5,320.8998 (12.85)    383.5033 (8.13)     5,216.9890 (13.04)    197.7330 (13.45)        7;11    187.9381 (0.08)        171           1
test_global_snapshot[True-x_fns2-False-slowseq take and restore snapshot, keep after restore]            5,056.2300 (13.28)    7,849.9280 (9.08)     5,392.2435 (13.02)    327.4688 (6.94)     5,311.0770 (13.28)    268.5015 (18.27)        15;6    185.4516 (0.08)        176           1
-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------

Legend:
  Outliers: 1 Standard Deviation from Mean; 1.5 IQR (InterQuartile Range) from 1st Quartile and 3rd Quartile.
  OPS: Operations Per Second, computed as 1 / Mean
============================================================================================================== 7 passed in 8.21s ===============================================================================================================
```

## Changes

### 1.0.0

Breaking changes:
- `deterministic_deploy` signature is now changed to `deterministic_deploy(contract_deploy_code, owner=None, data=None, value=None, init_value=None, deploy_to_address=None)`, i.e., `salt` is removed. [Related to [REVM-1182](https://github.com/bluealloy/revm/issues/1182)]. If you want to similate the previous behaviour, it's suggested to use the `deploy_to_address` parameter to specify the address where the contract will be deployed.
- `enabled` in `REVMConfig` is removed. To disable all instrumentation, use
