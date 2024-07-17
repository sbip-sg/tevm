// SPDX-License-Identifier: MIT
pragma solidity 0.8.19;
contract Test{
    mapping (int256=>int256) public counter1;
    mapping (int256=>int256) public counter2;
    int256 public counter = 1;
    function f_fast() public {
        counter += 1;
        counter1[counter] += 1;
    }
    function f_slow() public{
        for (int256 i = 0; i < 50; i++){
            counter += 1;
            counter1[counter] += 1;
        }
    }

    function fast_seq() public returns (int256){
        for (int256 i = 0; i < 5; i++){
            f_fast();
        }
        return counter;
    }
    function slow_seq() public returns (int256){
        for (int256 i = 0; i < 5; i++){
            f_slow();
        }
        return counter;
    }
}
