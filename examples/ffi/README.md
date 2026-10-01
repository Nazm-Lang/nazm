# Calling C (N42)

Save this as `ffi.nz` beside `add.c`. It is kept here rather than as a `.nz` file because the
compiler written in Nazm, which reads every `.nz` file in the tree, does not know foreign
declarations.

```nazm
extern "C" fn c_add(a: Int, b: Int) -> Int = "c_add";
extern "C" fn c_is_even(n: Int) -> Bool = "c_is_even";

fn classify(f: ForeignCap, n: Int) -> Int ! { foreign } {
    if c_is_even(n) { c_add(n, 1000) } else { c_add(n, 100) }
}

fn main(io: IoCap, f: ForeignCap) -> Int ! { io, foreign } {
    print(int_to_str(classify(f, 3)));
    0
}
```

```sh
clang -c add.c -o add.o
nazm build ffi.nz --link add.o -o ffi
./ffi            # 103 (3 is odd: c_add(3, 100)), then main's result: 1030
```

Only `Int` (`int64_t`) and `Bool` (`bool`) cross. Every call needs a `ForeignCap`, and `nazm run`
refuses the program: the interpreter cannot call C. `docs/spec.md`, *Foreign functions*, and
`docs/limitations.md` say what is and is not promised at the boundary.
