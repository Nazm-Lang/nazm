with Ada.Command_Line;
with Ada.Text_IO;
with Ada.Unchecked_Deallocation;

procedure Bintrees is
   Tab : constant Character := Character'Val (9);

   type Node;
   type Node_Access is access Node;
   type Node is record
      Left, Right : Node_Access;
   end record;

   procedure Free is new Ada.Unchecked_Deallocation (Node, Node_Access);

   function Make (Depth : Natural) return Node_Access is
   begin
      if Depth > 0 then
         return new Node'(Make (Depth - 1), Make (Depth - 1));
      end if;
      return new Node'(null, null);
   end Make;

   function Check (N : Node_Access) return Long_Integer is
     (if N.Left = null then 1 else 1 + Check (N.Left) + Check (N.Right));

   procedure Release (N : in out Node_Access) is
   begin
      if N.Left /= null then
         Release (N.Left);
         Release (N.Right);
      end if;
      Free (N);
   end Release;

   function Img (X : Long_Integer) return String is
      S : constant String := Long_Integer'Image (X);
   begin
      return S (S'First + 1 .. S'Last);
   end Img;

   Min_Depth : constant := 4;
   Max_Depth : Natural := 10;
   T, Long_Lived : Node_Access;
begin
   if Ada.Command_Line.Argument_Count > 0 then
      Max_Depth := Natural'Value (Ada.Command_Line.Argument (1));
   end if;
   Max_Depth := Natural'Max (Max_Depth, Min_Depth + 2);
   T := Make (Max_Depth + 1);
   Ada.Text_IO.Put_Line ("stretch tree of depth " & Img (Long_Integer (Max_Depth + 1))
                         & Tab & " check: " & Img (Check (T)));
   Release (T);
   Long_Lived := Make (Max_Depth);
   for D in 0 .. (Max_Depth - Min_Depth) / 2 loop
      declare
         Depth      : constant Natural := Min_Depth + 2 * D;
         Iterations : constant Long_Integer := 2 ** (Max_Depth - Depth + Min_Depth);
         Total      : Long_Integer := 0;
      begin
         for I in 1 .. Iterations loop
            T := Make (Depth);
            Total := Total + Check (T);
            Release (T);
         end loop;
         Ada.Text_IO.Put_Line (Img (Iterations) & Tab & " trees of depth "
                               & Img (Long_Integer (Depth)) & Tab & " check: " & Img (Total));
      end;
   end loop;
   Ada.Text_IO.Put_Line ("long lived tree of depth " & Img (Long_Integer (Max_Depth))
                         & Tab & " check: " & Img (Check (Long_Lived)));
   Release (Long_Lived);
end Bintrees;
