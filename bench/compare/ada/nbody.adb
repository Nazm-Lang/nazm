with Ada.Command_Line;
with Ada.Long_Float_Text_IO;
with Ada.Numerics.Long_Elementary_Functions;
with Ada.Text_IO;

procedure Nbody is
   use Ada.Numerics.Long_Elementary_Functions;

   Pi            : constant Long_Float := 3.141592653589793;
   Solar_Mass    : constant Long_Float := 4.0 * Pi * Pi;
   Days_Per_Year : constant Long_Float := 365.24;

   type Planet is record
      X, Y, Z, VX, VY, VZ, Mass : Long_Float;
   end record;

   Bodies : array (1 .. 5) of Planet :=
     ((0.0, 0.0, 0.0, 0.0, 0.0, 0.0, Solar_Mass),
      (4.84143144246472090E+00, -1.16032004402742839E+00, -1.03622044471123109E-01,
       1.66007664274403694E-03 * Days_Per_Year, 7.69901118419740425E-03 * Days_Per_Year,
       -6.90460016972063023E-05 * Days_Per_Year, 9.54791938424326609E-04 * Solar_Mass),
      (8.34336671824457987E+00, 4.12479856412430479E+00, -4.03523417114321381E-01,
       -2.76742510726862411E-03 * Days_Per_Year, 4.99852801234917238E-03 * Days_Per_Year,
       2.30417297573763929E-05 * Days_Per_Year, 2.85885980666130812E-04 * Solar_Mass),
      (1.28943695621391310E+01, -1.51111514016986312E+01, -2.23307578892655734E-01,
       2.96460137564761618E-03 * Days_Per_Year, 2.37847173959480950E-03 * Days_Per_Year,
       -2.96589568540237556E-05 * Days_Per_Year, 4.36624404335156298E-05 * Solar_Mass),
      (1.53796971148509165E+01, -2.59193146099879641E+01, 1.79258772950371181E-01,
       2.68067772490389322E-03 * Days_Per_Year, 1.62824170038242295E-03 * Days_Per_Year,
       -9.51592254519715870E-05 * Days_Per_Year, 5.15138902046611451E-05 * Solar_Mass));

   procedure Offset_Momentum is
      PX, PY, PZ : Long_Float := 0.0;
   begin
      for B of Bodies loop
         PX := PX + B.VX * B.Mass;
         PY := PY + B.VY * B.Mass;
         PZ := PZ + B.VZ * B.Mass;
      end loop;
      Bodies (1).VX := -PX / Solar_Mass;
      Bodies (1).VY := -PY / Solar_Mass;
      Bodies (1).VZ := -PZ / Solar_Mass;
   end Offset_Momentum;

   function Energy return Long_Float is
      E          : Long_Float := 0.0;
      DX, DY, DZ : Long_Float;
   begin
      for I in Bodies'Range loop
         E := E + 0.5 * Bodies (I).Mass * (Bodies (I).VX * Bodies (I).VX
                + Bodies (I).VY * Bodies (I).VY + Bodies (I).VZ * Bodies (I).VZ);
         for J in I + 1 .. Bodies'Last loop
            DX := Bodies (I).X - Bodies (J).X;
            DY := Bodies (I).Y - Bodies (J).Y;
            DZ := Bodies (I).Z - Bodies (J).Z;
            E := E - Bodies (I).Mass * Bodies (J).Mass / Sqrt (DX * DX + DY * DY + DZ * DZ);
         end loop;
      end loop;
      return E;
   end Energy;

   procedure Advance (DT : Long_Float) is
      DX, DY, DZ, D2, Mag : Long_Float;
   begin
      for I in Bodies'Range loop
         for J in I + 1 .. Bodies'Last loop
            DX := Bodies (I).X - Bodies (J).X;
            DY := Bodies (I).Y - Bodies (J).Y;
            DZ := Bodies (I).Z - Bodies (J).Z;
            D2 := DX * DX + DY * DY + DZ * DZ;
            Mag := DT / (D2 * Sqrt (D2));
            Bodies (I).VX := Bodies (I).VX - DX * Bodies (J).Mass * Mag;
            Bodies (I).VY := Bodies (I).VY - DY * Bodies (J).Mass * Mag;
            Bodies (I).VZ := Bodies (I).VZ - DZ * Bodies (J).Mass * Mag;
            Bodies (J).VX := Bodies (J).VX + DX * Bodies (I).Mass * Mag;
            Bodies (J).VY := Bodies (J).VY + DY * Bodies (I).Mass * Mag;
            Bodies (J).VZ := Bodies (J).VZ + DZ * Bodies (I).Mass * Mag;
         end loop;
      end loop;
      for B of Bodies loop
         B.X := B.X + DT * B.VX;
         B.Y := B.Y + DT * B.VY;
         B.Z := B.Z + DT * B.VZ;
      end loop;
   end Advance;

   procedure Show (E : Long_Float) is
   begin
      Ada.Long_Float_Text_IO.Put (E, Fore => 1, Aft => 9, Exp => 0);
      Ada.Text_IO.New_Line;
   end Show;

   N : Long_Integer := 1000;
begin
   if Ada.Command_Line.Argument_Count > 0 then
      N := Long_Integer'Value (Ada.Command_Line.Argument (1));
   end if;
   Offset_Momentum;
   Show (Energy);
   for Step in 1 .. N loop
      Advance (0.01);
   end loop;
   Show (Energy);
end Nbody;
