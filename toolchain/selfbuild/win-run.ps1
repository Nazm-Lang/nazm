# R8 slice 17 (S17.3): run Nazm-built programs on the Windows host, one at a time, each inside a
# Windows job object (`openspec/changes/r8-cut-over/design.md`, *Slice 17*). The Windows
# counterpart of `cargo xtask host-run` (`xtask/src/host_run.rs`), whose rules it keeps:
#
#   - the executable links the counted heap (`nz.heap_malloc`, or a stripped image's whole N0406
#     message) and names exactly one heap limit, at most 2 GiB, in that message;
#   - only one runs at a time (an exclusive lock file), in the foreground, under a deadline;
#   - its output is bounded (files polled; past the cap the job is ended, status 126);
#   - its peak memory is recorded: the job's peak committed memory of any one process and of the
#     whole job (Windows counts commit, which is stricter than macOS's resident memory);
#   - the withdrawal conditions: a process's peak commit past the limit by more than 256 MiB, or an
#     N0406 that did not end the program with status 2. Either writes the marker, and every later
#     run refuses until a person has investigated and removed it.
#
# Unlike macOS, Windows can cap a process tree's memory: the job also holds a hard ceiling (each
# process at limit + 512 MiB, the job at 2 x limit + 512 MiB, at most 4.5 GiB), at most 64 live
# processes, kills every descendant when it closes, and no error dialog waits for a person.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File win-run.ps1 -Steps STEPS -Report REPORT
#
# STEPS holds one step a line, fields separated by a TAB:
#   LABEL DEADLINE-SECONDS WORKDIR STDIN STDOUT STDERR ENV EXE ARGS
# STDIN is a file or NUL; ENV is `NAME=VALUE` pairs separated by `|`, or `-`; ARGS is the rest of the
# command line as cmd.exe reads it. Each step's status is written to STDOUT.status; a line per step
# is appended to REPORT. The run stops at the first anomaly: a refusal, a withdrawal condition, the
# deadline, the output cap, the memory ceiling reached, or a process left behind. Exit: 0 when every
# step ran (whatever the programs' own statuses), 1 refused, 70 withdrawn, 3 another anomaly.
param(
    [Parameter(Mandatory = $true)][string]$Steps,
    [Parameter(Mandatory = $true)][string]$Report,
    [string]$StateDir = "$env:USERPROFILE\nazm-w17\state",
    [int]$MaxOutMiB = 192,
    # Lowers the job's ceiling (never raises it): the probe that the ceiling holds on this host.
    [int]$CeilingMiB = 0
)
$ErrorActionPreference = 'Stop'
trap { [Console]::Out.WriteLine("win-run: ANOMALY: the runner failed: $_"); exit 3 }

Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.Text;
using System.Threading;
using System.Runtime.InteropServices;

public static class NzJob {
    [StructLayout(LayoutKind.Sequential)] struct BASIC { public long a, b; public uint Flags; public UIntPtr MinWs, MaxWs; public uint ActiveProcessLimit; public UIntPtr Affinity; public uint Priority, Sched; }
    [StructLayout(LayoutKind.Sequential)] struct IOC { public ulong a, b, c, d, e, f; }
    [StructLayout(LayoutKind.Sequential)] struct EXT { public BASIC Basic; public IOC Io; public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed; }
    [StructLayout(LayoutKind.Sequential)] struct ACCT { public long a, b, c, d; public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses, TotalTerminatedProcesses; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct SI { public int cb; public string r, desk, title; public int x, y, xs, ys, xc, yc, fill, flags; public short show, r2; public IntPtr r3, i, o, e; }
    [StructLayout(LayoutKind.Sequential)] struct PI { public IntPtr hProcess, hThread; public int pid, tid; }
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr CreateJobObject(IntPtr a, string n);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr j, int c, ref EXT i, int l);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr j, int c, ref EXT i, int l, IntPtr r);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr j, int c, ref ACCT i, int l, IntPtr r);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr j, IntPtr p);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateJobObject(IntPtr j, uint c);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool CreateProcess(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref SI si, out PI pi);
    [DllImport("kernel32.dll")] static extern uint ResumeThread(IntPtr t);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr h, out uint c);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] static extern uint SetErrorMode(uint m);

    public static long PeakProcess, PeakJob, Active, Total;
    public static string Anomaly;

    static long Size(string p) { try { return new FileInfo(p).Length; } catch { return 0; } }

    // Runs `cmd /d /s /c "LINE"` suspended inside a new job, then resumes it. Returns the exit status;
    // 124 past the deadline, 126 past the output cap (Anomaly says which, and when the ceiling was met).
    public static int Run(string line, string dir, int deadline, long procLimit, long jobLimit, string outFile, string errFile, long maxOut) {
        Anomaly = null; PeakProcess = PeakJob = Active = Total = 0;
        SetErrorMode(0x0001 | 0x0002 | 0x8000);
        IntPtr job = CreateJobObject(IntPtr.Zero, null);
        if (job == IntPtr.Zero) throw new Exception("CreateJobObject failed: " + Marshal.GetLastWin32Error());
        EXT e = new EXT();
        // PROCESS_MEMORY 0x100, JOB_MEMORY 0x200, DIE_ON_UNHANDLED_EXCEPTION 0x400, KILL_ON_JOB_CLOSE 0x2000, ACTIVE_PROCESS 0x8
        e.Basic.Flags = 0x100 | 0x200 | 0x400 | 0x2000 | 0x8;
        e.Basic.ActiveProcessLimit = 64;
        e.ProcessMemoryLimit = new UIntPtr((ulong)procLimit);
        e.JobMemoryLimit = new UIntPtr((ulong)jobLimit);
        if (!SetInformationJobObject(job, 9, ref e, Marshal.SizeOf(typeof(EXT)))) throw new Exception("SetInformationJobObject failed: " + Marshal.GetLastWin32Error());
        SI si = new SI(); si.cb = Marshal.SizeOf(typeof(SI));
        PI pi;
        StringBuilder cmd = new StringBuilder("cmd.exe /d /s /c \"" + line + "\"");
        // CREATE_SUSPENDED 0x4, CREATE_NO_WINDOW 0x08000000, CREATE_UNICODE_ENVIRONMENT 0x400
        if (!CreateProcess(null, cmd, IntPtr.Zero, IntPtr.Zero, false, 0x4 | 0x08000000, IntPtr.Zero, dir, ref si, out pi)) {
            CloseHandle(job); throw new Exception("CreateProcess failed: " + Marshal.GetLastWin32Error());
        }
        if (!AssignProcessToJobObject(job, pi.hProcess)) {
            int err = Marshal.GetLastWin32Error();
            TerminateJobObject(job, 1); CloseHandle(pi.hThread); CloseHandle(pi.hProcess); CloseHandle(job);
            throw new Exception("AssignProcessToJobObject failed: " + err);
        }
        ResumeThread(pi.hThread);
        DateTime until = DateTime.UtcNow.AddSeconds(deadline);
        int status = -1;
        while (true) {
            if (WaitForSingleObject(pi.hProcess, 100) == 0) { uint c; GetExitCodeProcess(pi.hProcess, out c); status = (int)c; break; }
            if (DateTime.UtcNow > until) { TerminateJobObject(job, 124); WaitForSingleObject(pi.hProcess, 5000); status = 124; Anomaly = "deadline: past " + deadline + " s, the job was ended"; break; }
            if (Size(outFile) > maxOut || Size(errFile) > maxOut) { TerminateJobObject(job, 126); WaitForSingleObject(pi.hProcess, 5000); status = 126; Anomaly = "output: past " + (maxOut >> 20) + " MiB, the job was ended"; break; }
        }
        // The console host (conhost.exe) of cmd's console leaves the job a moment after cmd: up to 3 s.
        ACCT a = new ACCT();
        for (int w = 0; w < 30; w++) {
            QueryInformationJobObject(job, 1, ref a, Marshal.SizeOf(typeof(ACCT)), IntPtr.Zero);
            if (a.ActiveProcesses == 0 || Anomaly != null) break;
            Thread.Sleep(100);
        }
        Active = a.ActiveProcesses; Total = a.TotalProcesses;
        if (Active > 0 && Anomaly == null) Anomaly = "descendants: " + Active + " processes were still running when the program ended; the job ended them";
        TerminateJobObject(job, 1);
        EXT q = new EXT();
        QueryInformationJobObject(job, 9, ref q, Marshal.SizeOf(typeof(EXT)), IntPtr.Zero);
        PeakProcess = (long)q.PeakProcessMemoryUsed.ToUInt64(); PeakJob = (long)q.PeakJobMemoryUsed.ToUInt64();
        if (Anomaly == null && (PeakProcess >= procLimit - (16L << 20) || PeakJob >= jobLimit - (16L << 20)))
            Anomaly = "ceiling: the job's memory ceiling was reached (peak process " + PeakProcess + ", job " + PeakJob + " bytes)";
        CloseHandle(pi.hThread); CloseHandle(pi.hProcess); CloseHandle(job);
        return status;
    }

    static int Find(byte[] h, byte[] n, int from) {
        for (int i = from; i + n.Length <= h.Length; i++) {
            int k = 0; while (k < n.Length && h[i + k] == n[k]) k++;
            if (k == n.Length) return i;
        }
        return -1;
    }

    // host-run's heap_limit_of: the limit, or -1 and why.
    public static long HeapLimit(string path, out string why) {
        why = null;
        byte[] img = File.ReadAllBytes(path);
        byte[] sym = Encoding.ASCII.GetBytes("nz.heap_malloc");
        byte[] before = Encoding.ASCII.GetBytes("nazm: error[N0406]: out of memory: the heap limit of ");
        byte[] after = Encoding.ASCII.GetBytes(" bytes is reached");
        bool symbol = Find(img, sym, 0) >= 0;
        System.Collections.Generic.List<long> found = new System.Collections.Generic.List<long>();
        int from = 0, at;
        while ((at = Find(img, before, from)) >= 0) {
            int s = at + before.Length, d = 0;
            while (s + d < img.Length && img[s + d] >= 48 && img[s + d] <= 57) d++;
            bool tail = d > 0 && s + d + after.Length <= img.Length;
            for (int k = 0; tail && k < after.Length; k++) if (img[s + d + k] != after[k]) tail = false;
            if (tail && d <= 18) { long n = long.Parse(Encoding.ASCII.GetString(img, s, d)); if (!found.Contains(n)) found.Add(n); }
            from = s;
        }
        if (!symbol && found.Count == 0) { why = "no-counted-heap: the executable does not link the counted heap (nz.heap_malloc is not in it): build it with --heap-limit"; return -1; }
        if (found.Count == 0) { why = "no-heap-limit: the executable carries no heap limit (its N0406 message is not in it)"; return -1; }
        if (found.Count > 1) { why = "ambiguous-heap-limit: the executable names several heap limits"; return -1; }
        if (found[0] > (2L << 30)) { why = "heap-limit-too-large: its heap limit is " + found[0] + " bytes, over the 2 GiB (2147483648) a host stage may have"; return -1; }
        return found[0];
    }
}
"@

function Say([string]$m) { [Console]::Out.WriteLine($m) }
function Append([string]$m) { [IO.File]::AppendAllText($Report, $m + "`r`n") }

New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
$marker = Join-Path $StateDir 'withdrawn'
$margin = 256MB
# One at a time: the lock is held open, exclusively, for the whole run.
try { $lock = [IO.File]::Open((Join-Path $StateDir 'lock'), 'OpenOrCreate', 'ReadWrite', 'None') }
catch { Say "win-run: refused [another-stage-running]: another win-run holds $StateDir\lock"; exit 1 }

$n = 0
foreach ($raw in [IO.File]::ReadAllLines($Steps)) {
    if ($raw.Trim() -eq '' -or $raw.StartsWith('#')) { continue }
    $f = $raw.Split("`t", 9)
    if ($f.Count -lt 8) { Say "win-run: refused [usage]: a step has $($f.Count) fields: $raw"; exit 1 }
    $label, $deadline, $dir, $in, $out, $err, $envs, $exe = $f[0..7]
    $argline = if ($f.Count -ge 9) { $f[8] } else { '' }
    $deadline = [int]$deadline
    if ($deadline -le 0 -or $deadline -gt 3600) { Say "win-run: refused [usage]: deadline $deadline s (1..3600) at '$label'"; exit 1 }
    if (Test-Path $marker) { Say "win-run: refused [withdrawn]: the rule is withdrawn ($([IO.File]::ReadAllText($marker))); marker: $marker"; exit 70 }
    $exePath = if ([IO.Path]::IsPathRooted($exe)) { $exe } else { Join-Path $dir $exe }
    if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) { Say "win-run: refused [no-executable]: $exePath at '$label'"; exit 1 }
    $why = $null
    $limit = [NzJob]::HeapLimit($exePath, [ref]$why)
    if ($limit -lt 0) { Say "win-run: refused [$($why.Split(':')[0])]: $($why.Substring($why.IndexOf(':') + 2)) ($exePath, '$label')"; exit 1 }
    $procLimit = $limit + 512MB
    $jobLimit = [Math]::Min(2 * $limit + 512MB, 4608MB)
    if ($CeilingMiB -gt 0) { $procLimit = [Math]::Min($procLimit, [long]$CeilingMiB * 1MB); $jobLimit = [Math]::Min($jobLimit, [long]$CeilingMiB * 1MB) }
    $saved = @{}
    if ($envs -ne '-') {
        foreach ($kv in $envs.Split('|')) {
            $i = $kv.IndexOf('='); $k = $kv.Substring(0, $i)
            $saved[$k] = [Environment]::GetEnvironmentVariable($k)
            [Environment]::SetEnvironmentVariable($k, $kv.Substring($i + 1))
        }
    }
    $outPath = if ([IO.Path]::IsPathRooted($out)) { $out } else { Join-Path $dir $out }
    $errPath = if ([IO.Path]::IsPathRooted($err)) { $err } else { Join-Path $dir $err }
    $line = "`"$exePath`" $argline <`"$in`" >`"$outPath`" 2>`"$errPath`""
    $status = [NzJob]::Run($line, $dir, $deadline, $procLimit, $jobLimit, $outPath, $errPath, [long]$MaxOutMiB * 1MB)
    foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
    [IO.File]::WriteAllText("$outPath.status", "$status`n")
    $n++
    Append ("# $label`r`nwin-run: heap limit $limit bytes; peak commit $([NzJob]::PeakProcess) bytes (job $([NzJob]::PeakJob)); processes $([NzJob]::Total); exit $status")
    $errText = ''
    try { $errText = [IO.File]::ReadAllText($errPath) } catch { }
    $withdraw = $null
    if ([NzJob]::PeakProcess -gt $limit + $margin) { $withdraw = "peak commit $([NzJob]::PeakProcess) bytes exceeds the heap limit $limit by more than $margin" }
    elseif ($errText.Contains('error[N0406]') -and $status -ne 2) { $withdraw = "an N0406 was written but the program did not stop with status 2 (it ended $status)" }
    if ($withdraw) {
        [IO.File]::WriteAllText($marker, "$withdraw (running $exePath, '$label')")
        Say "win-run: WITHDRAWAL CONDITION at '$label': $withdraw; marker $marker"
        Append "WITHDRAWAL CONDITION: $withdraw"
        exit 70
    }
    if ([NzJob]::Anomaly) { Say "win-run: ANOMALY at '$label': $([NzJob]::Anomaly)"; Append "ANOMALY: $([NzJob]::Anomaly)"; exit 3 }
}
Say "win-run: $n steps run"
exit 0
