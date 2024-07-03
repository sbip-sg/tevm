// Compile this contract with version 0.3.6
contract Test{
    function test(uint256 i) public returns (uint256){
        if (i==0){
            return 323 / i;
        }else if (i==1){
            return 323 % (i - 1);
        }else if (i==2){
            return addmod(3, 100, (i - 2));
        }else if (i==3){
            int a = -3;
            int b = 0;
            if (a % b > 0){
                return 1;
            }
        }
        return i;
    }
}
