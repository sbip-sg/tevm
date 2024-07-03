pragma solidity 0.8.19;
contract Test{
    uint var1;
    uint var2;
    function func1(uint8 input) public {
        if (input == 0x2008)
            var1 = input;
        else
            var1 = 1;
    }
    function func2(uint input) public {
        if (input == 0x2007)
            var2 = input;
        else
            var2 = 1;
    }
    function func3(uint input) public{
        require(var1 > 10 && var2 > 10);
        selfdestruct(payable(msg.sender));
    }
}

