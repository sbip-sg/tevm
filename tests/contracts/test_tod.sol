pragma solidity 0.7.0;
contract Test {
    address payable winner_TOD27;
    uint256 test;
    function play_TOD27() public {
        // if (keccak256(abi.encode(guess)) == keccak256(abi.encode("hello"))) {
        //     winner_TOD27 = msg.sender;
        // }
        // require (keccak256(abi.encode(guess)) == keccak256(abi.encode("hello")));
        winner_TOD27 = payable(msg.sender);
        test = 1;
    }

    function getReward_TOD27() public payable {
        winner_TOD27.transfer(msg.value);
    }
    mapping (address => uint256) public test_;
    function write_a(uint input) public {
        test_[address(0)] = input;
    }
    function read_a() public returns (uint){
        return test_[address(0)];
    }
}
