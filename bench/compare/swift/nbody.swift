import Foundation

let solarMass = 4 * Double.pi * Double.pi
let daysPerYear = 365.24

struct Body { var x, y, z, vx, vy, vz, mass: Double }

var bodies = [
    Body(x: 0, y: 0, z: 0, vx: 0, vy: 0, vz: 0, mass: solarMass),
    Body(x: 4.84143144246472090e+00, y: -1.16032004402742839e+00, z: -1.03622044471123109e-01,
         vx: 1.66007664274403694e-03 * daysPerYear, vy: 7.69901118419740425e-03 * daysPerYear,
         vz: -6.90460016972063023e-05 * daysPerYear, mass: 9.54791938424326609e-04 * solarMass),
    Body(x: 8.34336671824457987e+00, y: 4.12479856412430479e+00, z: -4.03523417114321381e-01,
         vx: -2.76742510726862411e-03 * daysPerYear, vy: 4.99852801234917238e-03 * daysPerYear,
         vz: 2.30417297573763929e-05 * daysPerYear, mass: 2.85885980666130812e-04 * solarMass),
    Body(x: 1.28943695621391310e+01, y: -1.51111514016986312e+01, z: -2.23307578892655734e-01,
         vx: 2.96460137564761618e-03 * daysPerYear, vy: 2.37847173959480950e-03 * daysPerYear,
         vz: -2.96589568540237556e-05 * daysPerYear, mass: 4.36624404335156298e-05 * solarMass),
    Body(x: 1.53796971148509165e+01, y: -2.59193146099879641e+01, z: 1.79258772950371181e-01,
         vx: 2.68067772490389322e-03 * daysPerYear, vy: 1.62824170038242295e-03 * daysPerYear,
         vz: -9.51592254519715870e-05 * daysPerYear, mass: 5.15138902046611451e-05 * solarMass),
]

func offsetMomentum() {
    var px = 0.0, py = 0.0, pz = 0.0
    for b in bodies {
        px += b.vx * b.mass
        py += b.vy * b.mass
        pz += b.vz * b.mass
    }
    bodies[0].vx = -px / solarMass
    bodies[0].vy = -py / solarMass
    bodies[0].vz = -pz / solarMass
}

func energy() -> Double {
    var e = 0.0
    for i in 0..<bodies.count {
        let a = bodies[i]
        e += 0.5 * a.mass * (a.vx * a.vx + a.vy * a.vy + a.vz * a.vz)
        for j in (i + 1)..<bodies.count {
            let b = bodies[j]
            let dx = a.x - b.x, dy = a.y - b.y, dz = a.z - b.z
            e -= a.mass * b.mass / (dx * dx + dy * dy + dz * dz).squareRoot()
        }
    }
    return e
}

func advance(_ dt: Double) {
    for i in 0..<bodies.count {
        for j in (i + 1)..<bodies.count {
            let dx = bodies[i].x - bodies[j].x
            let dy = bodies[i].y - bodies[j].y
            let dz = bodies[i].z - bodies[j].z
            let d2 = dx * dx + dy * dy + dz * dz
            let mag = dt / (d2 * d2.squareRoot())
            bodies[i].vx -= dx * bodies[j].mass * mag
            bodies[i].vy -= dy * bodies[j].mass * mag
            bodies[i].vz -= dz * bodies[j].mass * mag
            bodies[j].vx += dx * bodies[i].mass * mag
            bodies[j].vy += dy * bodies[i].mass * mag
            bodies[j].vz += dz * bodies[i].mass * mag
        }
    }
    for i in 0..<bodies.count {
        bodies[i].x += dt * bodies[i].vx
        bodies[i].y += dt * bodies[i].vy
        bodies[i].z += dt * bodies[i].vz
    }
}

let n = CommandLine.arguments.count > 1 ? Int(CommandLine.arguments[1])! : 1000
offsetMomentum()
print(String(format: "%.9f", energy()))
for _ in 0..<n { advance(0.01) }
print(String(format: "%.9f", energy()))
