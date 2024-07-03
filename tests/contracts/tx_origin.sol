// SPDX-License-Identifier: MIT
pragma solidity >0.4.22;

contract TxOrigin{
    address owner;
    constructor(){
        owner = msg.sender;
    }
    function run() external view returns (uint r){
        if (tx.origin == owner){
            r = 100;
        }
    }
}
