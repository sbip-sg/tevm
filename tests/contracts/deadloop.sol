// SPDX-License-Identifier: MIT
pragma solidity =0.7.0;
contract DeadLoop{
    uint256 i = 0;
    function run() public returns (uint256){
        for(uint256 j=0; ; j++){
            i = j++;
        }
        return i;
    }
}
