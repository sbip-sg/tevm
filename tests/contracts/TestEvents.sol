
pragma solidity 0.8.15;

contract TestEvent {
    event Transfer (address indexed src, address indexed dst, uint256 wad);

    constructor() {
    }

    function makeEvent(uint256 i) public{
        emit Transfer(address(this), 0xF58764c35eD1528Ec78DF18BebB24Fa20f6A626F, i);
    }
}
