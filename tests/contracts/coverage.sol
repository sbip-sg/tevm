// SPDX-License-Identifier: MIT
pragma solidity ^0.7.0;
contract coverage{
    function guess(uint256 i) public pure returns (uint256){
        if (i > 10000){
            return 100;
        }else if (i > 100){
            return 10;            
        }
        return 0;
    }
}
