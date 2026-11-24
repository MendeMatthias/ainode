//! Free and total space of the filesystem that holds $DATA, and on-disk sizes
//! of the app's own directories.
//!
//! Before consent $DATA does not exist yet (nothing may be written), so the
//! nearest existing ancestor stands in for it: the same filesystem in every
//! normal setup.

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct DiskSpace {
    /// Bytes available to an unprivileged process (f_bavail), what Finder shows.
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// Windows: GetDiskFreeSpaceExW, what Explorer shows (the caller's quota).
#[cfg(windows)]
pub fn space_for(path: &Path) -> Result<DiskSpace, String> {
    use std::os::windows::ffi::OsStrExt;
    let mut p = path;
    while !p.exists() {
        p = p.parent().ok_or_else(|| format!("no existing ancestor of {}", path.display()))?;
    }
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: a NUL-terminated wide path; the out pointers are valid u64s.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut())
    };
    if ok == 0 {
        return Err(format!("GetDiskFreeSpaceExW {}: {}", p.display(), std::io::Error::last_os_error()));
    }
    Ok(DiskSpace { free_bytes: free, total_bytes: total })
}

#[cfg(unix)]
pub fn space_for(path: &Path) -> Result<DiskSpace, String> {
    let mut p = path;
    while !p.exists() {
        p = p.parent().ok_or_else(|| format!("no existing ancestor of {}", path.display()))?;
    }
    let c = CString::new(p.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // SAFETY: statvfs fills the zeroed struct we pass and reads a valid C string.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if rc != 0 {
        return Err(format!("statvfs {}: {}", p.display(), std::io::Error::last_os_error()));
    }
    let frsize = st.f_frsize as u64;
    Ok(DiskSpace {
        free_bytes: (st.f_bavail as u64).saturating_mul(frsize),
        total_bytes: (st.f_blocks as u64).saturating_mul(frsize),
    })
}

/// Sum of regular-file sizes under `root` (apparent size, no symlink
/// following). Missing root is 0. Used for the runnable copies, which are a
/// handful of files per model.
pub fn tree_bytes(root: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
            } else if ft.is_file() {
                total = total.saturating_add(e.metadata().map(|m| m.len()).unwrap_or(0));
            }
        }
    }
    total
}

/// Lowercase hex sha256 of a file, streamed. Blocking: call it from
/// `spawn_blocking`. About 0.75 s for the 2.2 GB 3B on an M5 (release).
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// This Mac's memory in bytes (sysctl hw.memsize), 0 if it cannot be read.
#[cfg(target_os = "macos")]
pub fn ram_bytes() -> u64 {
    let mut v: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: hw.memsize is a u64; v and len are valid for the call.
    let rc = unsafe {
        libc::sysctlbyname(c"hw.memsize".as_ptr(), (&mut v as *mut u64).cast(), &mut len, std::ptr::null_mut(), 0)
    };
    if rc == 0 && len == std::mem::size_of::<u64>() { v } else { 0 }
}

/// This computer's memory in bytes, comparable with a catalog's `ram_gb`
/// (the size printed on the box): macOS's hw.memsize is exactly 16 GiB on a
/// 16 GB Mac, but Linux's MemTotal leaves out what the kernel and firmware
/// keep (15.5 GiB on 16 GB), which would hide a model the computer can run.
/// So: the memory blocks the kernel has online (sysfs), else MemTotal
/// (sysinfo), rounded up to a whole GiB. 0 if neither can be read.
#[cfg(target_os = "linux")]
pub fn ram_bytes() -> u64 {
    let gib = 1u64 << 30;
    let online = online_memory_bytes(Path::new("/sys/devices/system/memory")).unwrap_or(0);
    // SAFETY: sysinfo fills the zeroed struct we pass.
    let mut si: libc::sysinfo = unsafe { std::mem::zeroed() };
    let total = if unsafe { libc::sysinfo(&mut si) } == 0 {
        (si.totalram as u64).saturating_mul(u64::from(si.mem_unit.max(1)))
    } else {
        0
    };
    online.max(total).div_ceil(gib).saturating_mul(gib)
}

/// Windows: the physical memory (GlobalMemoryStatusEx), rounded up to a whole
/// GiB for the same reason as on Linux (32 GB reads 31.8).
#[cfg(windows)]
pub fn ram_bytes() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let gib = 1u64 << 30;
    // SAFETY: dwLength is set as the API requires; the struct is ours.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
        return 0;
    }
    m.ullTotalPhys.div_ceil(gib).saturating_mul(gib)
}

/// Sum of the online memory blocks under `/sys/devices/system/memory`
/// (`block_size_bytes` is hex), None when the kernel does not expose them.
#[cfg(target_os = "linux")]
fn online_memory_bytes(root: &Path) -> Option<u64> {
    let block = std::fs::read_to_string(root.join("block_size_bytes")).ok()?;
    let block = u64::from_str_radix(block.trim().trim_start_matches("0x"), 16).ok().filter(|&b| b > 0)?;
    let mut n = 0u64;
    for e in std::fs::read_dir(root).ok()?.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().starts_with("memory") {
            continue;
        }
        if std::fs::read_to_string(e.path().join("online")).is_ok_and(|v| v.trim() == "1") {
            n += 1;
        }
    }
    (n > 0).then(|| n.saturating_mul(block))
}

/// A drive or volume models can live on, for the first-run choice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Place {
    /// `C:\` on Windows, a mount point elsewhere.
    pub root: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// The fixed drives (Windows), the real filesystems mounted (Linux), the
/// startup disk and what is under /Volumes (macOS), each with its space; one
/// whose space cannot be read is left out. Never empty: at least the one
/// `fallback` is on.
pub fn places(fallback: &Path) -> Vec<Place> {
    let mut out: Vec<Place> = place_roots()
        .into_iter()
        .filter_map(|r| space_for(&r).ok().map(|s| Place { root: r.to_string_lossy().into_owned(), free_bytes: s.free_bytes, total_bytes: s.total_bytes }))
        .collect();
    if out.is_empty() {
        if let Ok(s) = space_for(fallback) {
            out.push(Place { root: root_of(fallback), free_bytes: s.free_bytes, total_bytes: s.total_bytes });
        }
    }
    out
}

/// The drive or mount point `p` is on, as `places` names it: the longest
/// root that is a prefix of the path, else the path's own root.
pub fn root_of(p: &Path) -> String {
    let full = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let full = PathBuf::from(full.to_string_lossy().trim_start_matches(r"\\?\"));
    let mut best: Option<PathBuf> = None;
    for r in place_roots() {
        if full.starts_with(&r) && best.as_ref().is_none_or(|b| r.as_os_str().len() > b.as_os_str().len()) {
            best = Some(r);
        }
    }
    best.or_else(|| full.ancestors().last().map(Path::to_path_buf)).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(windows)]
fn place_roots() -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    // winbase.h DRIVE_FIXED: a hard disk, not removable, remote, a CD or a RAM disk.
    const DRIVE_FIXED: u32 = 3;
    // SAFETY: plain queries with no pointers but a NUL-terminated wide string.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i as u8) as char))
        .filter(|root| {
            let wide: Vec<u16> = std::ffi::OsStr::new(root).encode_wide().chain(std::iter::once(0)).collect();
            let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
            kind == DRIVE_FIXED
        })
        .map(PathBuf::from)
        .collect()
}

#[cfg(target_os = "linux")]
fn place_roots() -> Vec<PathBuf> {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else { return vec![PathBuf::from("/")] };
    let real = ["ext2", "ext3", "ext4", "xfs", "btrfs", "zfs", "f2fs", "jfs", "ntfs", "ntfs3", "exfat", "vfat", "fuseblk"];
    let hidden = ["/boot", "/snap", "/proc", "/sys", "/dev", "/var/lib", "/usr", "/etc"];
    let mut seen_dev = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in mounts.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 || !real.contains(&f[2]) {
            continue;
        }
        let mount = f[1].replace("\\040", " ");
        if mount != "/" && hidden.iter().any(|h| mount == *h || mount.starts_with(&format!("{h}/"))) {
            continue;
        }
        if !seen_dev.insert(f[0].to_string()) {
            continue;
        }
        out.push(PathBuf::from(mount));
    }
    if !out.iter().any(|p| p == Path::new("/")) {
        out.insert(0, PathBuf::from("/"));
    }
    out
}

#[cfg(target_os = "macos")]
fn place_roots() -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from("/")];
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            // The startup disk appears here too, as a link to /: once is enough.
            if e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()) {
                out.push(e.path());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_name_real_roots_with_their_space() {
        let here = std::env::temp_dir();
        let ps = places(&here);
        assert!(!ps.is_empty());
        for p in &ps {
            assert!(!p.root.is_empty() && Path::new(&p.root).is_absolute(), "{p:?}");
            assert!(p.total_bytes >= p.free_bytes, "{p:?}");
        }
        let mine = root_of(&here);
        assert!(ps.iter().any(|p| p.root == mine), "{mine} among {ps:?}");
    }

    #[test]
    fn reads_this_computers_memory() {
        // Any computer that builds this has more than 1 GB, in whole GiB on
        // Linux and Windows. Not "at least 8 GB on a Mac": a virtual Mac can
        // have less (GitHub's macOS runner has 7, and hw.memsize says so).
        let min = 1;
        assert!(super::ram_bytes() >= min * 1024 * 1024 * 1024, "{}", super::ram_bytes());
        if !cfg!(target_os = "macos") {
            assert_eq!(super::ram_bytes() % (1 << 30), 0);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn online_memory_blocks_are_summed() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        std::fs::write(r.join("block_size_bytes"), "8000000\n").unwrap(); // 128 MiB, hex
        for (i, on) in [(0, "1"), (1, "1"), (2, "0"), (3, "1")] {
            std::fs::create_dir(r.join(format!("memory{i}"))).unwrap();
            std::fs::write(r.join(format!("memory{i}/online")), on).unwrap();
        }
        std::fs::create_dir(r.join("power")).unwrap();
        assert_eq!(super::online_memory_bytes(r), Some(3 * 128 * 1024 * 1024));
        assert_eq!(super::online_memory_bytes(&r.join("missing")), None);
    }

    #[test]
    fn sha256_file_matches_the_nist_vector() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("abc");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(sha256_file(&tmp.path().join("missing")).is_err());
    }

    #[test]
    fn space_of_a_missing_path_is_its_ancestors() {
        let tmp = tempfile::tempdir().unwrap();
        let a = space_for(tmp.path()).unwrap();
        let b = space_for(&tmp.path().join("not/yet/created")).unwrap();
        assert!(a.total_bytes > 0 && a.free_bytes > 0 && a.free_bytes <= a.total_bytes);
        assert_eq!(a.total_bytes, b.total_bytes);
    }

    #[test]
    fn tree_bytes_counts_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        std::fs::write(tmp.path().join("a/x"), vec![0u8; 10]).unwrap();
        std::fs::write(tmp.path().join("a/b/y"), vec![0u8; 5]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path().join("a/x"), tmp.path().join("a/link")).unwrap();
        assert_eq!(tree_bytes(tmp.path()), 15);
        assert_eq!(tree_bytes(&tmp.path().join("missing")), 0);
    }
}
