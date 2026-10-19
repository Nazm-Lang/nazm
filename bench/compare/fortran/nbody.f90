program nbody
    implicit none
    real(8), parameter :: pi = 3.141592653589793d0
    real(8), parameter :: solar_mass = 4 * pi * pi
    real(8), parameter :: days_per_year = 365.24d0
    type body
        real(8) :: x, y, z, vx, vy, vz, mass
    end type body
    type(body) :: bodies(5)
    character(len=32) :: arg
    integer(8) :: n, step

    bodies(1) = body(0d0, 0d0, 0d0, 0d0, 0d0, 0d0, solar_mass)
    bodies(2) = body(4.84143144246472090d+00, -1.16032004402742839d+00, -1.03622044471123109d-01, &
        1.66007664274403694d-03 * days_per_year, 7.69901118419740425d-03 * days_per_year, &
        -6.90460016972063023d-05 * days_per_year, 9.54791938424326609d-04 * solar_mass)
    bodies(3) = body(8.34336671824457987d+00, 4.12479856412430479d+00, -4.03523417114321381d-01, &
        -2.76742510726862411d-03 * days_per_year, 4.99852801234917238d-03 * days_per_year, &
        2.30417297573763929d-05 * days_per_year, 2.85885980666130812d-04 * solar_mass)
    bodies(4) = body(1.28943695621391310d+01, -1.51111514016986312d+01, -2.23307578892655734d-01, &
        2.96460137564761618d-03 * days_per_year, 2.37847173959480950d-03 * days_per_year, &
        -2.96589568540237556d-05 * days_per_year, 4.36624404335156298d-05 * solar_mass)
    bodies(5) = body(1.53796971148509165d+01, -2.59193146099879641d+01, 1.79258772950371181d-01, &
        2.68067772490389322d-03 * days_per_year, 1.62824170038242295d-03 * days_per_year, &
        -9.51592254519715870d-05 * days_per_year, 5.15138902046611451d-05 * solar_mass)

    n = 1000
    if (command_argument_count() > 0) then
        call get_command_argument(1, arg)
        read (arg, *) n
    end if
    call offset_momentum()
    call show(energy())
    do step = 1, n
        call advance(0.01d0)
    end do
    call show(energy())

contains

    subroutine show(e)
        real(8), intent(in) :: e
        character(len=32) :: buf
        write (buf, '(f14.9)') e
        print '(a)', trim(adjustl(buf))
    end subroutine show

    subroutine offset_momentum()
        real(8) :: px, py, pz
        integer :: i
        px = 0; py = 0; pz = 0
        do i = 1, 5
            px = px + bodies(i)%vx * bodies(i)%mass
            py = py + bodies(i)%vy * bodies(i)%mass
            pz = pz + bodies(i)%vz * bodies(i)%mass
        end do
        bodies(1)%vx = -px / solar_mass
        bodies(1)%vy = -py / solar_mass
        bodies(1)%vz = -pz / solar_mass
    end subroutine offset_momentum

    function energy() result(e)
        real(8) :: e, dx, dy, dz
        integer :: i, j
        e = 0
        do i = 1, 5
            e = e + 0.5d0 * bodies(i)%mass * (bodies(i)%vx * bodies(i)%vx + &
                bodies(i)%vy * bodies(i)%vy + bodies(i)%vz * bodies(i)%vz)
            do j = i + 1, 5
                dx = bodies(i)%x - bodies(j)%x
                dy = bodies(i)%y - bodies(j)%y
                dz = bodies(i)%z - bodies(j)%z
                e = e - bodies(i)%mass * bodies(j)%mass / sqrt(dx * dx + dy * dy + dz * dz)
            end do
        end do
    end function energy

    subroutine advance(dt)
        real(8), intent(in) :: dt
        real(8) :: dx, dy, dz, d2, mag
        integer :: i, j
        do i = 1, 5
            do j = i + 1, 5
                dx = bodies(i)%x - bodies(j)%x
                dy = bodies(i)%y - bodies(j)%y
                dz = bodies(i)%z - bodies(j)%z
                d2 = dx * dx + dy * dy + dz * dz
                mag = dt / (d2 * sqrt(d2))
                bodies(i)%vx = bodies(i)%vx - dx * bodies(j)%mass * mag
                bodies(i)%vy = bodies(i)%vy - dy * bodies(j)%mass * mag
                bodies(i)%vz = bodies(i)%vz - dz * bodies(j)%mass * mag
                bodies(j)%vx = bodies(j)%vx + dx * bodies(i)%mass * mag
                bodies(j)%vy = bodies(j)%vy + dy * bodies(i)%mass * mag
                bodies(j)%vz = bodies(j)%vz + dz * bodies(i)%mass * mag
            end do
        end do
        do i = 1, 5
            bodies(i)%x = bodies(i)%x + dt * bodies(i)%vx
            bodies(i)%y = bodies(i)%y + dt * bodies(i)%vy
            bodies(i)%z = bodies(i)%z + dt * bodies(i)%vz
        end do
    end subroutine advance

end program nbody
