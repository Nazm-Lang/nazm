program bintrees
    implicit none
    type node
        type(node), pointer :: left => null(), right => null()
    end type node
    character(len=32) :: arg
    character, parameter :: tab = achar(9)
    integer, parameter :: min_depth = 4
    integer :: max_depth, stretch, d
    integer(8) :: iterations, total, i
    type(node), pointer :: t, long_lived

    max_depth = 10
    if (command_argument_count() > 0) then
        call get_command_argument(1, arg)
        read (arg, *) max_depth
    end if
    max_depth = max(max_depth, min_depth + 2)
    stretch = max_depth + 1
    t => make(stretch)
    print '(a,i0,a,i0)', 'stretch tree of depth ', stretch, tab // ' check: ', check(t)
    call release(t)
    long_lived => make(max_depth)
    do d = min_depth, max_depth, 2
        iterations = ishft(1_8, max_depth - d + min_depth)
        total = 0
        do i = 1, iterations
            t => make(d)
            total = total + check(t)
            call release(t)
        end do
        print '(i0,a,i0,a,i0)', iterations, tab // ' trees of depth ', d, tab // ' check: ', total
    end do
    print '(a,i0,a,i0)', 'long lived tree of depth ', max_depth, tab // ' check: ', check(long_lived)
    call release(long_lived)

contains

    recursive function make(depth) result(n)
        integer, intent(in) :: depth
        type(node), pointer :: n
        allocate (n)
        if (depth > 0) then
            n%left => make(depth - 1)
            n%right => make(depth - 1)
        end if
    end function make

    recursive function check(n) result(c)
        type(node), pointer, intent(in) :: n
        integer(8) :: c
        if (associated(n%left)) then
            c = 1 + check(n%left) + check(n%right)
        else
            c = 1
        end if
    end function check

    recursive subroutine release(n)
        type(node), pointer :: n
        if (associated(n%left)) then
            call release(n%left)
            call release(n%right)
        end if
        deallocate (n)
    end subroutine release

end program bintrees
