//test for two bugs type exception disorder + unchecked send

pragma solidity ^0.7.0;
contract Test {
    function txorigin() public {
        require(msg.sender == tx.origin);
    }
}
