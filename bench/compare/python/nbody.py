import sys
from math import sqrt

PI = 3.141592653589793
SOLAR_MASS = 4 * PI * PI
DAYS_PER_YEAR = 365.24

# [x, y, z, vx, vy, vz, mass]
bodies = [
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


def offset_momentum():
    px = py = pz = 0.0
    for b in bodies:
        px += b[3] * b[6]
        py += b[4] * b[6]
        pz += b[5] * b[6]
    bodies[0][3] = -px / SOLAR_MASS
    bodies[0][4] = -py / SOLAR_MASS
    bodies[0][5] = -pz / SOLAR_MASS


def energy():
    e = 0.0
    for i, a in enumerate(bodies):
        e += 0.5 * a[6] * (a[3] * a[3] + a[4] * a[4] + a[5] * a[5])
        for b in bodies[i + 1:]:
            dx, dy, dz = a[0] - b[0], a[1] - b[1], a[2] - b[2]
            e -= a[6] * b[6] / sqrt(dx * dx + dy * dy + dz * dz)
    return e


def advance(dt):
    for i, a in enumerate(bodies):
        for b in bodies[i + 1:]:
            dx, dy, dz = a[0] - b[0], a[1] - b[1], a[2] - b[2]
            d2 = dx * dx + dy * dy + dz * dz
            mag = dt / (d2 * sqrt(d2))
            a[3] -= dx * b[6] * mag
            a[4] -= dy * b[6] * mag
            a[5] -= dz * b[6] * mag
            b[3] += dx * a[6] * mag
            b[4] += dy * a[6] * mag
            b[5] += dz * a[6] * mag
    for b in bodies:
        b[0] += dt * b[3]
        b[1] += dt * b[4]
        b[2] += dt * b[5]


n = int(sys.argv[1]) if len(sys.argv) > 1 else 1000
offset_momentum()
print(f"{energy():.9f}")
for _ in range(n):
    advance(0.01)
print(f"{energy():.9f}")
