pragma solidity 0.7.0;
contract Test{
    function sign_distance(int256 i) public returns (uint256) {
        if (i > 10)
            return 1;
        else if (i > -2)
            return 2;
        else if (i <= -10)
            return 3;
    }
}
