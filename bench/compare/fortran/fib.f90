program fib_main
    implicit none
    print '(i0)', fib(35_8)
contains
    recursive function fib(n) result(r)
        integer(8), intent(in) :: n
        integer(8) :: r
        if (n < 2) then
            r = n
        else
            r = fib(n - 1) + fib(n - 2)
        end if
    end function fib
end program fib_main
