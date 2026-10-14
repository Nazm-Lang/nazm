program fib;

function Fib(N: Int64): Int64;
begin
  if N < 2 then Fib := N else Fib := Fib(N - 1) + Fib(N - 2);
end;

begin
  WriteLn(Fib(35));
end.
