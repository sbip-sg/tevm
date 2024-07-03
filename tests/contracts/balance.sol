// SPDX-License-Identifier: MIT
pragma solidity 0.7.0;
contract Balance{
    function balance(address addr) public view returns (uint256){
        return addr.balance;
    }

    function selfbalance() public view returns (uint256){
        return address(this).balance;
    }
}
