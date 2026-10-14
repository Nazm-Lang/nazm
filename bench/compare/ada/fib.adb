with Ada.Long_Integer_Text_IO;
with Ada.Text_IO;

procedure Fib is
   function Fib (N : Long_Integer) return Long_Integer is
     (if N < 2 then N else Fib (N - 1) + Fib (N - 2));
begin
   Ada.Long_Integer_Text_IO.Put (Fib (35), Width => 0);
   Ada.Text_IO.New_Line;
end Fib;
