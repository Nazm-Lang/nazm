//! What the interpreter's `os_` built-ins need of the host that differs between POSIX and Windows
//! (W1, `docs/user/spec.md`, *Windows x86_64*): the family of error numbers and the statuses in it, the
//! key a handle's object is filed under, names as bytes, entropy, and error text. Everything else
//! in `eval::os`, `eval::net` and `eval::process` is portable `std`.
//!
//! On Windows a status is a negative **Linux** errno number, as the compiled runtime's Windows
//! layer answers it (`nazm-runtime`, `windows_os`): a Win32 error is translated as the C runtime
//! translates it (`_dosmaperr`) and a Winsock error by the runtime's own table, so `@std/ioerror`'s
//! one mapping serves every host.

/// The host's family of error numbers, as `nz_os_family` answers it: `0` macOS, `1` Linux, `2`
/// Windows (whose statuses are in Linux's numbering).
pub(super) const fn family() -> i64 {
    if cfg!(windows) {
        2
    } else if cfg!(target_os = "linux") {
        1
    } else {
        0
    }
}

/// Whether the host's numbers are Linux's: Linux itself, and Windows, translated.
const LINUX_NUMBERS: bool = cfg!(any(target_os = "linux", windows));

/// `EWOULDBLOCK` in the host's family.
pub(super) const EAGAIN: i64 = if LINUX_NUMBERS { 11 } else { 35 };
/// `ETIMEDOUT` in the host's family.
pub(super) const ETIMEDOUT: i64 = if LINUX_NUMBERS { 110 } else { 60 };
/// `ENOSYS` in the host's family.
pub(super) const ENOSYS: i64 = if LINUX_NUMBERS { 38 } else { 78 };

/// The positive error number of a failure the system reported, in the host's family; `EINVAL` for
/// one it did not number.
pub(super) fn errno(e: &std::io::Error) -> i64 {
    match e.raw_os_error() {
        #[cfg(windows)]
        Some(code) => windows_errno(code),
        #[cfg(not(windows))]
        Some(code) => i64::from(code),
        // A failure the library raised itself, with no system number: a program that is not found
        // (`Command` on Windows looks for it before it asks the system).
        None if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound => 2,
        None => 22,
    }
}

/// A Win32 error (`GetLastError`) or a Winsock one (`WSAGetLastError`) in Linux's numbering.
#[cfg(any(windows, test))]
fn windows_errno(code: i32) -> i64 {
    match code {
        // Winsock's (the runtime's `nz.win.wsa_map`).
        10004 => 4,
        10009 => 9,
        10013 => 13,
        10014 => 14,
        10022 => 22,
        10024 => 24,
        10035 => 11,
        10036 => 115,
        10037 => 114,
        10038 => 88,
        10039 => 89,
        10040 => 90,
        10041 => 91,
        10042 => 92,
        10043 => 93,
        10045 => 95,
        10047 => 97,
        10048 => 98,
        10049 => 99,
        10050 => 100,
        10051 => 101,
        10052 => 102,
        10053 => 103,
        10054 => 104,
        10055 => 105,
        10056 => 106,
        10057 => 107,
        10058 => 32,
        10060 => 110,
        10061 => 111,
        10064 => 112,
        10065 => 113,
        10000..=11999 => 5,
        // Win32's, as the C runtime's `_dosmaperr` maps them (its numbers, which agree with
        // Linux's below 35; `ENOTEMPTY` is Linux's 39).
        2 | 3 | 15 | 18 | 53 | 67 | 161 | 206 => 2,
        4 => 24,
        5 | 19..=36 | 65 | 82 | 83 | 108 | 158 | 180 => 13,
        6 | 114 | 130 => 9,
        7..=9 | 1816 => 12,
        10 => 7,
        11 | 188..=202 => 8,
        17 => 18,
        80 | 183 => 17,
        89 | 164 | 215 => 11,
        109 => 32,
        112 => 28,
        128 | 129 => 10,
        145 => 39,
        _ => 22,
    }
}

/// The key a handle's object is filed under in the table: its descriptor on POSIX; on Windows the
/// low 32 bits of its handle or socket, which the system keeps distinct while it is open and never
/// makes 0, 1 or 2 (standard input's, output's and errors' keys).
#[cfg(unix)]
pub(super) fn key(o: &impl std::os::fd::AsRawFd) -> i64 {
    i64::from(o.as_raw_fd())
}

#[cfg(windows)]
pub(super) fn key(o: &impl std::os::windows::io::AsRawHandle) -> i64 {
    i64::try_from(o.as_raw_handle().addr() & 0xFFFF_FFFF).unwrap_or(0)
}

#[cfg(unix)]
pub(super) fn socket_key(o: &impl std::os::fd::AsRawFd) -> i64 {
    i64::from(o.as_raw_fd())
}

#[cfg(windows)]
pub(super) fn socket_key(o: &impl std::os::windows::io::AsRawSocket) -> i64 {
    i64::try_from(o.as_raw_socket() & 0xFFFF_FFFF).unwrap_or(0)
}

/// A file or directory held open only to give a handle a key no other open object has: the null
/// device, as the native runtime keys a child or (on Windows) a directory.
pub(super) fn null_device() -> std::io::Result<std::fs::File> {
    std::fs::File::open(if cfg!(windows) { "NUL" } else { "/dev/null" })
}

/// A name from the system as the bytes a program sees: exactly on POSIX; UTF-8 on Windows, `None`
/// for one that is not valid UTF-16 (`docs/user/spec.md`, *Windows x86_64*, **Text and paths**).
pub(super) fn bytes_of(s: &std::ffi::OsStr) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        Some(std::os::unix::ffi::OsStrExt::as_bytes(s).to_vec())
    }
    #[cfg(not(unix))]
    {
        s.to_str().map(|s| s.as_bytes().to_vec())
    }
}

/// A program's bytes as a name for the system: exactly on POSIX; on Windows, `None` for bytes that
/// are not UTF-8 (`EILSEQ`, as the runtime answers).
pub(super) fn os_string(b: &[u8]) -> Option<std::ffi::OsString> {
    #[cfg(unix)]
    {
        Some(<std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(b.to_vec()))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(b).ok().map(std::ffi::OsString::from)
    }
}

/// `EILSEQ` in the host's family: a name that is not text the system can take.
pub(super) const EILSEQ: i64 = if LINUX_NUMBERS { 84 } else { 92 };

/// `buf` filled from the system's generator: its random device on POSIX. On Windows, which has no
/// device and whose generator a crate that forbids `unsafe` cannot call, 128 bits of the system's
/// generator are taken fresh for each call (the keys `std` seeds a new thread's hashers with, from
/// `ProcessPrng`) and expanded by SipHash in counter mode.
pub(super) fn entropy(buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Read as _;
        std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf))
    }
    #[cfg(not(unix))]
    {
        use std::hash::{BuildHasher as _, Hasher as _};
        let n = buf.len();
        let words = std::thread::spawn(move || {
            let keys = std::collections::hash_map::RandomState::new();
            (0..n.div_ceil(8) as u64)
                .map(|i| {
                    let mut h = keys.build_hasher();
                    h.write_u64(i);
                    h.finish()
                })
                .collect::<Vec<u64>>()
        })
        .join()
        .map_err(|_| std::io::Error::other("the entropy thread failed"))?;
        for (chunk, w) in buf.chunks_mut(8).zip(words) {
            let bytes = w.to_le_bytes();
            chunk.copy_from_slice(bytes.get(..chunk.len()).unwrap_or_default());
        }
        Ok(())
    }
}

/// The system's sentence for the positive error number `e` of the host's family: `std`'s on POSIX
/// (the C library's `strerror`); on Windows the C runtime's `strerror` for the Linux number taken
/// back to the C runtime's, as the native runtime's `nz_os_error_text` answers.
pub(super) fn error_text(e: i32) -> String {
    #[cfg(windows)]
    {
        crt_strerror(e).to_owned()
    }
    #[cfg(not(windows))]
    {
        let full = std::io::Error::from_raw_os_error(e).to_string();
        let suffix = format!(" (os error {e})");
        full.strip_suffix(&suffix).unwrap_or(&full).to_owned()
    }
}

/// MinGW-w64's C runtime (`msvcrt.dll`) `strerror`, read on a Windows 10 host, for a Linux
/// number.
#[cfg(any(windows, test))]
fn crt_strerror(linux: i32) -> &'static str {
    const CRT: [&str; 43] = [
        "No error",
        "Operation not permitted",
        "No such file or directory",
        "No such process",
        "Interrupted function call",
        "Input/output error",
        "No such device or address",
        "Arg list too long",
        "Exec format error",
        "Bad file descriptor",
        "No child processes",
        "Resource temporarily unavailable",
        "Not enough space",
        "Permission denied",
        "Bad address",
        "Unknown error",
        "Resource device",
        "File exists",
        "Improper link",
        "No such device",
        "Not a directory",
        "Is a directory",
        "Invalid argument",
        "Too many open files in system",
        "Too many open files",
        "Inappropriate I/O control operation",
        "Unknown error",
        "File too large",
        "No space left on device",
        "Invalid seek",
        "Read-only file system",
        "Too many links",
        "Broken pipe",
        "Domain error",
        "Result too large",
        "Unknown error",
        "Resource deadlock avoided",
        "Unknown error",
        "Filename too long",
        "No locks available",
        "Function not implemented",
        "Directory not empty",
        "Illegal byte sequence",
    ];
    // The six numbers on which the two numberings differ.
    let crt = match linux {
        35 => 36,
        36 => 38,
        37 => 39,
        38 => 40,
        39 => 41,
        84 => 42,
        n => n,
    };
    usize::try_from(crt)
        .ok()
        .and_then(|i| CRT.get(i))
        .copied()
        .unwrap_or("Unknown error")
}

#[cfg(test)]
mod tests {
    use super::{crt_strerror, windows_errno};

    #[test]
    fn windows_errors_are_linux_numbers() {
        assert_eq!(windows_errno(2), 2); // ERROR_FILE_NOT_FOUND
        assert_eq!(windows_errno(32), 13); // a sharing violation is permission denied
        assert_eq!(windows_errno(183), 17); // ERROR_ALREADY_EXISTS
        assert_eq!(windows_errno(145), 39); // ERROR_DIR_NOT_EMPTY: Linux's ENOTEMPTY
        assert_eq!(windows_errno(10061), 111); // WSAECONNREFUSED
        assert_eq!(windows_errno(10060), 110); // WSAETIMEDOUT
        assert_eq!(windows_errno(10101), 5);
        assert_eq!(windows_errno(9999), 22);
    }

    #[test]
    fn the_c_runtimes_sentences_by_linux_number() {
        assert_eq!(crt_strerror(2), "No such file or directory");
        assert_eq!(crt_strerror(39), "Directory not empty");
        assert_eq!(crt_strerror(84), "Illegal byte sequence");
        assert_eq!(crt_strerror(110), "Unknown error");
    }
}
