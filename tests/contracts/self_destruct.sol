// SPDX-License-Identifier: MIT
pragma solidity 0.8.10;

contract Des {
    function clone(address implementation) internal returns (address instance){
        assembly {
            mstore(0x00, or(shr(0xe8, shl(0x60, implementation)), 0x3d602d80600a3d3981f3363d3d373d3d3d363d73000000))
            mstore(0x20, or(shl(0x78, implementation), 0x5af43d82803e903d91602b57fd5bf3))
            instance := create(0, 0x09, 0x37)
        }
        require(instance != address(0), "ERC1167: create failed");
    }
    function kill() public payable {
        clone(address(this));
        selfdestruct(payable(0x4675C7e5BaAFBFFbca748158bEcBA61ef3b0a263));
    }
}
