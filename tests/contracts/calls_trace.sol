pragma solidity ^0.7.0;
contract example{
    function always_fail() public {
        revert();
    }
    function self_call() public {
        address(this).call("");
    }
    function always_success() public{
        // do nothing
    }
    function test_call_failed() public{
        (bool success, bytes memory result)  = address(this).call(hex"31ffb467"); // always_fail()
        require (success);
    }
    function test_call_success() public{
        address(this).call(hex"bcf95ac1"); // always_success()
    }
    function test_call_success_success_failed() public{
        address(this).call(hex"60193614"); // test_call_failed()
    }
}
