// SPDX-License-Identifier: MIT

pragma solidity 0.7.0;

contract BHash{
    uint256 public bn;
    bytes32 public lh;
    
    constructor(){
        bn = block.number;
        lh = blockhash(bn - 1);
    }
}
