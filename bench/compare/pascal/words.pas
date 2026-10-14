program words;
{$mode delphi}
uses SysUtils, Classes, Generics.Collections, Generics.Defaults;

type
  TCount = TPair<string, Integer>;

function ByCount(constref A, B: TCount): Integer;
begin
  Result := B.Value - A.Value;
  if Result = 0 then Result := CompareStr(A.Key, B.Key);
end;

var
  Lines: TStringList;
  Counts: TDictionary<string, Integer>;
  Sorted: TList<TCount>;
  Line, W: string;
  P: TCount;
  N, I: Integer;
begin
  Lines := TStringList.Create;
  Lines.LoadFromFile(ParamStr(1));
  Counts := TDictionary<string, Integer>.Create;
  for Line in Lines do
    for W in Line.Split([' ']) do
      if W <> '' then
        if Counts.TryGetValue(W, N) then Counts[W] := N + 1 else Counts.Add(W, 1);
  Sorted := TList<TCount>.Create;
  for P in Counts do Sorted.Add(P);
  Sorted.Sort(TComparer<TCount>.Construct(ByCount));
  WriteLn(Counts.Count);
  for I := 0 to 9 do WriteLn(Sorted[I].Key, ' ', Sorted[I].Value);
end.
