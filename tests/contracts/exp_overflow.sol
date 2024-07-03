// SPDX-License-Identifier: MIT
pragma solidity =0.7.0;
contract ExpOverflow{
    uint public a; 
    function exp(uint input) public { 
        a = 2**input; 
    } 
}
