PI = 3.141592653589793
SOLAR_MASS = 4 * PI * PI
DAYS_PER_YEAR = 365.24

# [x, y, z, vx, vy, vz, mass]
BODIES = [
  [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR_MASS],
  [4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,
   1.66007664274403694e-03 * DAYS_PER_YEAR, 7.69901118419740425e-03 * DAYS_PER_YEAR,
   -6.90460016972063023e-05 * DAYS_PER_YEAR, 9.54791938424326609e-04 * SOLAR_MASS],
  [8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,
   -2.76742510726862411e-03 * DAYS_PER_YEAR, 4.99852801234917238e-03 * DAYS_PER_YEAR,
   2.30417297573763929e-05 * DAYS_PER_YEAR, 2.85885980666130812e-04 * SOLAR_MASS],
  [1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01,
   2.96460137564761618e-03 * DAYS_PER_YEAR, 2.37847173959480950e-03 * DAYS_PER_YEAR,
   -2.96589568540237556e-05 * DAYS_PER_YEAR, 4.36624404335156298e-05 * SOLAR_MASS],
  [1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,
   2.68067772490389322e-03 * DAYS_PER_YEAR, 1.62824170038242295e-03 * DAYS_PER_YEAR,
   -9.51592254519715870e-05 * DAYS_PER_YEAR, 5.15138902046611451e-05 * SOLAR_MASS],
]

def offset_momentum
  px = py = pz = 0.0
  BODIES.each do |b|
    px += b[3] * b[6]
    py += b[4] * b[6]
    pz += b[5] * b[6]
  end
  BODIES[0][3] = -px / SOLAR_MASS
  BODIES[0][4] = -py / SOLAR_MASS
  BODIES[0][5] = -pz / SOLAR_MASS
end

def energy
  e = 0.0
  BODIES.each_with_index do |a, i|
    e += 0.5 * a[6] * (a[3] * a[3] + a[4] * a[4] + a[5] * a[5])
    BODIES[(i + 1)..].each do |b|
      dx = a[0] - b[0]
      dy = a[1] - b[1]
      dz = a[2] - b[2]
      e -= a[6] * b[6] / Math.sqrt(dx * dx + dy * dy + dz * dz)
    end
  end
  e
end

def advance(dt)
  BODIES.each_with_index do |a, i|
    BODIES[(i + 1)..].each do |b|
      dx = a[0] - b[0]
      dy = a[1] - b[1]
      dz = a[2] - b[2]
      d2 = dx * dx + dy * dy + dz * dz
      mag = dt / (d2 * Math.sqrt(d2))
      a[3] -= dx * b[6] * mag
      a[4] -= dy * b[6] * mag
      a[5] -= dz * b[6] * mag
      b[3] += dx * a[6] * mag
      b[4] += dy * a[6] * mag
      b[5] += dz * a[6] * mag
    end
  end
  BODIES.each do |b|
    b[0] += dt * b[3]
    b[1] += dt * b[4]
    b[2] += dt * b[5]
  end
end

n = (ARGV[0] || 1000).to_i
offset_momentum
puts format("%.9f", energy)
n.times { advance(0.01) }
puts format("%.9f", energy)
