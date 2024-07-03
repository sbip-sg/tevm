// SPDX-License-Identifier: MIT
pragma solidity 0.7.0;
contract Test{
    uint256 public i = 0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe;
    constructor(uint256 m) {
        if (m * i < m) {
            i = m;
        }
    }
}
