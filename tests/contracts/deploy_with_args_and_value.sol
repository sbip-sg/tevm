// SPDX-License-Identifier: MIT
pragma solidity 0.7.0;
contract DeployWithArgsAndValue{
    uint256 public x;
    uint256 public y;
    constructor(uint256 a, uint256 b) payable {
        x = a;
        y = b;
    }
}
