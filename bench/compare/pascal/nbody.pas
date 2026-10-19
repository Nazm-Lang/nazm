program nbody;
{$mode objfpc}
uses SysUtils, Math;

const
  Pi_ = 3.141592653589793;
  DaysPerYear = 365.24;

type
  TBody = record
    X, Y, Z, VX, VY, VZ, Mass: Double;
  end;

var
  SolarMass: Double;
  Bodies: array[0..4] of TBody;
  N, Step: Int64;

function MakeBody(X, Y, Z, VX, VY, VZ, M: Double): TBody;
begin
  Result.X := X; Result.Y := Y; Result.Z := Z;
  Result.VX := VX * DaysPerYear; Result.VY := VY * DaysPerYear; Result.VZ := VZ * DaysPerYear;
  Result.Mass := M * SolarMass;
end;

procedure OffsetMomentum;
var
  I: Integer;
  PX, PY, PZ: Double;
begin
  PX := 0; PY := 0; PZ := 0;
  for I := 0 to 4 do
  begin
    PX := PX + Bodies[I].VX * Bodies[I].Mass;
    PY := PY + Bodies[I].VY * Bodies[I].Mass;
    PZ := PZ + Bodies[I].VZ * Bodies[I].Mass;
  end;
  Bodies[0].VX := -PX / SolarMass;
  Bodies[0].VY := -PY / SolarMass;
  Bodies[0].VZ := -PZ / SolarMass;
end;

function Energy: Double;
var
  I, J: Integer;
  DX, DY, DZ: Double;
begin
  Result := 0;
  for I := 0 to 4 do
  begin
    Result := Result + 0.5 * Bodies[I].Mass * (Bodies[I].VX * Bodies[I].VX +
      Bodies[I].VY * Bodies[I].VY + Bodies[I].VZ * Bodies[I].VZ);
    for J := I + 1 to 4 do
    begin
      DX := Bodies[I].X - Bodies[J].X;
      DY := Bodies[I].Y - Bodies[J].Y;
      DZ := Bodies[I].Z - Bodies[J].Z;
      Result := Result - Bodies[I].Mass * Bodies[J].Mass / Sqrt(DX * DX + DY * DY + DZ * DZ);
    end;
  end;
end;

procedure Advance(DT: Double);
var
  I, J: Integer;
  DX, DY, DZ, D2, Mag: Double;
begin
  for I := 0 to 4 do
    for J := I + 1 to 4 do
    begin
      DX := Bodies[I].X - Bodies[J].X;
      DY := Bodies[I].Y - Bodies[J].Y;
      DZ := Bodies[I].Z - Bodies[J].Z;
      D2 := DX * DX + DY * DY + DZ * DZ;
      Mag := DT / (D2 * Sqrt(D2));
      Bodies[I].VX := Bodies[I].VX - DX * Bodies[J].Mass * Mag;
      Bodies[I].VY := Bodies[I].VY - DY * Bodies[J].Mass * Mag;
      Bodies[I].VZ := Bodies[I].VZ - DZ * Bodies[J].Mass * Mag;
      Bodies[J].VX := Bodies[J].VX + DX * Bodies[I].Mass * Mag;
      Bodies[J].VY := Bodies[J].VY + DY * Bodies[I].Mass * Mag;
      Bodies[J].VZ := Bodies[J].VZ + DZ * Bodies[I].Mass * Mag;
    end;
  for I := 0 to 4 do
  begin
    Bodies[I].X := Bodies[I].X + DT * Bodies[I].VX;
    Bodies[I].Y := Bodies[I].Y + DT * Bodies[I].VY;
    Bodies[I].Z := Bodies[I].Z + DT * Bodies[I].VZ;
  end;
end;

begin
  SolarMass := 4 * Pi_ * Pi_;
  FillChar(Bodies[0], SizeOf(TBody), 0);
  Bodies[0].Mass := SolarMass;
  Bodies[1] := MakeBody(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,
    1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05, 9.54791938424326609e-04);
  Bodies[2] := MakeBody(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,
    -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04);
  Bodies[3] := MakeBody(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01,
    2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05, 4.36624404335156298e-05);
  Bodies[4] := MakeBody(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,
    2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05);
  N := 1000;
  if ParamCount > 0 then N := StrToInt64(ParamStr(1));
  OffsetMomentum;
  WriteLn(Energy:0:9);
  for Step := 1 to N do Advance(0.01);
  WriteLn(Energy:0:9);
end.
