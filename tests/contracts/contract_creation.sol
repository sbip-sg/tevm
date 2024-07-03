// SPDX-License-Identifier: MIT
pragma solidity 0.8.17;

contract A {
    uint256 public balance = 0;
    address payable public owner;
    constructor(){
        owner = payable(msg.sender);
    }
    function topUp() public payable returns (uint256){
        balance += msg.value;
        return msg.value;
    }
    function withdraw() public{
        require(owner == msg.sender, "Requies owner to withdraw");
        owner.transfer(address(this).balance);
    }

    function setOwner(address _owner) public{
        require(_owner == msg.sender, "Requies owner to withdraw");
        owner = payable(_owner);
    }
}



contract B{
    A public a;
    mapping(address => uint256) public balances;
    constructor(){
        a = new A();
    }

    function add() public payable{
        require(msg.value > 0, "Need value to add");
        balances[msg.sender] += msg.value;
        require(a.topUp{value: msg.value}() > 0, "Topup failed");
    }

}
