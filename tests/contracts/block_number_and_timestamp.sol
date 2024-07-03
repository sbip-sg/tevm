// SPDX-License-Identifier: MIT
pragma solidity ^0.6.0;

contract TimestampDependecny{
    uint256 private luckyNumber;

    function genNumber(uint256 _n, uint256 _nonce, uint256 _modulus) internal view returns (uint256){
        return uint256(keccak256(abi.encodePacked(address(this), msg.sender, _n, _nonce))) % _modulus;
    }
    
    constructor () public{
        luckyNumber = genNumber(18982918, 1989328473927389472891913123111, 198190001188917777777);
    }
    
    function timestamp_bug(uint256 _guess) external view returns (bool){
        uint256 t = genNumber(_guess, block.timestamp, 1982891991991977181);
        return t == luckyNumber;
    }

    function blocknumber_bug(uint256 _guess) external view returns (bool){
        uint256 t = genNumber(_guess, block.number, 1982891991991977183);
        return t == luckyNumber;
    }
}
