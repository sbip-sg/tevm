// SPDX-License-Identifier: MIT
pragma solidity >0.4.2;

contract IntCast{
    uint8 v1  = 128;
    function add(uint8 n) external view returns (uint8){
        return v1 + n;
    }
}
