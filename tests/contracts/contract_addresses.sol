// SPDX-License-Identifier: MIT

pragma solidity 0.8.17;

contract A{
    uint256 public bn;
    bytes32 public lh;

    constructor(){
        bn = block.number;
        lh = blockhash(bn - 1);
    }
}

contract B{
    A a;
    constructor(address _a){
        a = A(_a);
    }

    function getBlockNumber() public view returns(uint256){
        return a.bn();
    }
}
