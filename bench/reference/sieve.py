# The same sieve as `bench/programs/sieve.nz`, statement for statement.
#
# Written the way the Nazm program is written rather than the way Python would be:
# `while` rather than `for`, a list of ints rather than a bytearray, no slice
# assignment. A Pythonic sieve is a different algorithm and would be measuring that.

def main() -> int:
    limit = 400000
    sieve = []
    i = 0
    while i <= limit:
        sieve.append(1)
        i = i + 1

    p = 2
    while p * p <= limit:
        if sieve[p] == 1:
            m = p * p
            while m <= limit:
                sieve[m] = 0
                m = m + p
        p = p + 1

    count = 0
    n = 2
    while n <= limit:
        count = count + sieve[n]
        n = n + 1
    return count


print(main())
