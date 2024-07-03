// SPDX-License-Identifier: MIT
pragma solidity 0.8.12;

import "@openzeppelin/contracts/utils/Strings.sol";

contract ComplexContract {
    using Strings for uint256;
    uint256 private counter;
    string public counterStr;
    mapping (address => uint256) private balances;
    event BalanceUpdated(address indexed _from, uint256 _value, string newCounterStr);

    constructor() {
        counter = 0;
    }

    function complexFunction() public returns (string memory) {
        uint256 randNonce = 0;
        uint256 NUM_RANDOM_ADDRESSES = 10;

        for (uint256 i = 0; i < NUM_RANDOM_ADDRESSES; i++) {
            randNonce++;
            address randomAddress = address(uint160(uint256(keccak256(abi.encodePacked(block.timestamp, msg.sender, randNonce)))));
            uint256 randomAmount = uint256(keccak256(abi.encodePacked(block.difficulty, block.timestamp, randNonce))) % 1000;
            balances[randomAddress] = randomAmount;
            emit BalanceUpdated(randomAddress, randomAmount, counter.toString());
        }

        uint256 total = 0;
        for (uint256 i = 0; i < NUM_RANDOM_ADDRESSES; i++) {
            randNonce++;
            uint256 randomAmount = uint256(keccak256(abi.encodePacked(block.difficulty, block.timestamp, randNonce))) % 1000;
            total += randomAmount;
        }

        counter = (counter + total) % type(uint256).max;

        counter = counter * total % type(uint256).max;

        counterStr = counter.toString();

        return counterStr;
    }

    function getBalance(address addr) public view returns (uint256) {
        return balances[addr];
    }
}
