program bintrees;
{$mode objfpc}
uses SysUtils;

type
  PNode = ^TNode;
  TNode = record
    Left, Right: PNode;
  end;

function Make(Depth: Integer): PNode;
begin
  New(Result);
  if Depth > 0 then
  begin
    Result^.Left := Make(Depth - 1);
    Result^.Right := Make(Depth - 1);
  end
  else
  begin
    Result^.Left := nil;
    Result^.Right := nil;
  end;
end;

function Check(N: PNode): Int64;
begin
  if N^.Left = nil then Result := 1
  else Result := 1 + Check(N^.Left) + Check(N^.Right);
end;

procedure Release(N: PNode);
begin
  if N^.Left <> nil then
  begin
    Release(N^.Left);
    Release(N^.Right);
  end;
  Dispose(N);
end;

const
  MinDepth = 4;

var
  MaxDepth, Stretch, D: Integer;
  Iterations, Total, I: Int64;
  T, LongLived: PNode;

begin
  MaxDepth := 10;
  if ParamCount > 0 then MaxDepth := StrToInt(ParamStr(1));
  if MaxDepth < MinDepth + 2 then MaxDepth := MinDepth + 2;
  Stretch := MaxDepth + 1;
  T := Make(Stretch);
  WriteLn('stretch tree of depth ', Stretch, #9' check: ', Check(T));
  Release(T);
  LongLived := Make(MaxDepth);
  D := MinDepth;
  while D <= MaxDepth do
  begin
    Iterations := Int64(1) shl (MaxDepth - D + MinDepth);
    Total := 0;
    for I := 1 to Iterations do
    begin
      T := Make(D);
      Total := Total + Check(T);
      Release(T);
    end;
    WriteLn(Iterations, #9' trees of depth ', D, #9' check: ', Total);
    D := D + 2;
  end;
  WriteLn('long lived tree of depth ', MaxDepth, #9' check: ', Check(LongLived));
  Release(LongLived);
end.
