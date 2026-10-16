! Fortran has no hash map: an open-addressing table of word positions in the text, FNV-1a (32-bit,
! so no product overflows), then the ten most frequent by selection, ties by word ascending.
program words
    implicit none
    integer, parameter :: cap = 65536
    character(len=:), allocatable :: text
    integer :: wstart(cap), wlen(cap), u, size, i, j, s, distinct, best, k
    integer(8) :: cnt(cap), h
    logical :: taken(cap)
    character(len=256) :: path

    call get_command_argument(1, path)
    open(newunit=u, file=trim(path), access='stream', form='unformatted', action='read')
    inquire(unit=u, size=size)
    allocate(character(len=size) :: text)
    read(u) text
    close(u)

    wlen = 0
    cnt = 0
    distinct = 0
    i = 1
    do while (i <= size)
        if (text(i:i) == ' ' .or. text(i:i) == achar(10)) then
            i = i + 1
            cycle
        end if
        s = i
        do while (i <= size)
            if (text(i:i) == ' ' .or. text(i:i) == achar(10)) exit
            i = i + 1
        end do
        h = 2166136261_8
        do j = s, i - 1
            h = iand(ieor(h, int(ichar(text(j:j)), 8)) * 16777619_8, 4294967295_8)
        end do
        k = int(iand(h, int(cap - 1, 8))) + 1
        do while (wlen(k) /= 0)
            if (wlen(k) == i - s) then
                if (text(wstart(k):wstart(k) + wlen(k) - 1) == text(s:i - 1)) exit
            end if
            k = mod(k, cap) + 1
        end do
        if (wlen(k) == 0) then
            wstart(k) = s
            wlen(k) = i - s
            distinct = distinct + 1
        end if
        cnt(k) = cnt(k) + 1
    end do

    print '(i0)', distinct
    taken = .false.
    do j = 1, 10
        best = 0
        do k = 1, cap
            if (wlen(k) == 0 .or. taken(k)) cycle
            if (best == 0) then
                best = k
            else if (cnt(k) > cnt(best) .or. (cnt(k) == cnt(best) .and. &
                     llt(text(wstart(k):wstart(k) + wlen(k) - 1), text(wstart(best):wstart(best) + wlen(best) - 1)))) then
                best = k
            end if
        end do
        taken(best) = .true.
        print '(a,1x,i0)', text(wstart(best):wstart(best) + wlen(best) - 1), cnt(best)
    end do
end program words
