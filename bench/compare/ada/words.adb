with Ada.Command_Line;
with Ada.Containers.Indefinite_Hashed_Maps;
with Ada.Containers.Indefinite_Vectors;
with Ada.Integer_Text_IO;
with Ada.Strings.Hash;
with Ada.Text_IO; use Ada.Text_IO;

procedure Words is
   package Maps is new Ada.Containers.Indefinite_Hashed_Maps
     (String, Natural, Ada.Strings.Hash, "=");
   package Vectors is new Ada.Containers.Indefinite_Vectors (Positive, String);

   Counts : Maps.Map;
   Sorted : Vectors.Vector;
   File   : File_Type;

   procedure Count (W : String) is
      C : constant Maps.Cursor := Counts.Find (W);
   begin
      if Maps.Has_Element (C) then
         Counts.Replace_Element (C, Maps.Element (C) + 1);
      else
         Counts.Insert (W, 1);
      end if;
   end Count;

   function Before (A, B : String) return Boolean is
     (Counts (A) > Counts (B) or else (Counts (A) = Counts (B) and then A < B));
   package Sorting is new Vectors.Generic_Sorting (Before);
begin
   Open (File, In_File, Ada.Command_Line.Argument (1));
   while not End_Of_File (File) loop
      declare
         Line  : constant String := Get_Line (File);
         Start : Positive := Line'First;
      begin
         for I in Line'Range loop
            if Line (I) = ' ' then
               if I > Start then
                  Count (Line (Start .. I - 1));
               end if;
               Start := I + 1;
            end if;
         end loop;
         if Start <= Line'Last then
            Count (Line (Start .. Line'Last));
         end if;
      end;
   end loop;
   Close (File);
   for C in Counts.Iterate loop
      Sorted.Append (Maps.Key (C));
   end loop;
   Sorting.Sort (Sorted);
   Ada.Integer_Text_IO.Put (Integer (Counts.Length), Width => 0);
   New_Line;
   for I in 1 .. 10 loop
      Put_Line (Sorted (I) & Natural'Image (Counts (Sorted (I))));
   end loop;
end Words;
