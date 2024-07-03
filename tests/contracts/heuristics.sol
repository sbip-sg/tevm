// SPDX-License-Identifier: MIT
pragma solidity 0.7.0;
contract Test{
    function coverage(uint256 i) public pure returns (uint256){
        if (i > 10000){
            return 100;
        }else if (i > 100){
            return 10;            
        } else if (i == 2){
            return 2;
        }
        return 0;
    }
}
