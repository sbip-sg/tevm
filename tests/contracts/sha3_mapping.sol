// SPDX-License-Identifier: MIT

pragma solidity 0.8.17;

contract StorageTest {
    uint256 a;     // slot 0
    uint256[2] b;  // slots 1-2

    struct Entry {
        uint256 id;
        uint256 value;
    }
    Entry c;       // slots 3-4
    Entry[] d;

    function arrLocation(uint256 slot, uint256 index, uint256 elementSize)
        public
        returns (uint256)
    {
        Entry memory e = Entry(11, 22);
        d.push(e);
        return uint256(keccak256(abi.encodePacked(slot))) + (index * elementSize);
    }
}
