//! Windows: AInode's data folder is the user's alone.
//!
//! A folder made in the user's profile (the default,
//! `%LOCALAPPDATA%\AInode\data`) inherits the profile's ACL: the user,
//! SYSTEM and Administrators. A folder made on another drive at the first
//! start (`D:\AInode`) inherits that drive's root instead, which on a
//! non-system NTFS drive grants "Authenticated Users" Modify and "Users"
//! Read (security audit 2026-10-18, H1: measured on the PC). Every other
//! account on the computer could then read the kept chats and change the
//! settings, the engine's catalog and the WSL distribution's disk image,
//! which runs as root inside the distribution with the user's Windows drives
//! mounted: a way into the user's account.
//!
//! So the folder gets a protected ACL (nothing inherited from the drive):
//! the user, SYSTEM and Administrators, full control, passed down to every
//! file and folder in it. Applied when the folder is made at the first
//! start, and checked at every start, so a folder made by an earlier AInode
//! is repaired.
//!
//! The second audit (2026-10-19) added what an open folder may already hold:
//! - Its owner. An owner can rewrite the ACL at any time, so a folder owned
//!   by another account (one that made `D:\AInode` before the user's first
//!   start) is taken over, or refused when that is not possible.
//! - Everything below it. While the folder was open, another account could
//!   make files there with ACLs of its own, which the folder's ACL never
//!   replaces. So once a folder is found open, every file and folder in it
//!   that another account owns or may use is reset to inherit the private
//!   ACL, or deleted when that is refused. WSL's own entries for its virtual
//!   machine (`NT VIRTUAL MACHINE\<id>`) on `ext4.vhdx` are kept. Links
//!   (reparse points) are never followed.
//! - Only what another account owns is ever deleted (the Mac's review of
//!   0.8.3): a file of the user's own that cannot be made private is kept,
//!   and the folder is reported open (the engine does not start). Paths go to
//!   Windows in their long form (`\\?\`), so a file deeper than 260
//!   characters is read and repaired like any other, not taken for a
//!   foreign one.
//! - Allow entries of any kind count: a conditional ("callback") allow entry
//!   for Everyone is as open as a plain one.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW,
    GetNamedSecurityInfoW, GetSecurityInfo, SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetAce, GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
    UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetVolumeInformationW, GetVolumePathNameW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// SYSTEM and the Administrators group, besides the user.
const SYSTEM: &str = "S-1-5-18";
const ADMINISTRATORS: &str = "S-1-5-32-544";
/// CREATOR OWNER: an inherit-only stand-in for whoever makes a file, which
/// here is the user.
const CREATOR_OWNER: &str = "S-1-3-0";
/// `NT VIRTUAL MACHINE\<id>`: WSL gives its virtual machine access to the
/// distribution's disk image.
const VIRTUAL_MACHINE: &str = "S-1-5-83-";
/// What an allow entry that names no SID we can read stands for: never ours.
const UNKNOWN: &str = "?";
/// ACE types that grant (winnt.h): plain, object, callback, callback object.
/// The others deny or audit.
const ALLOW_TYPES: [u8; 4] = [0x0, 0x5, 0x9, 0xB];
/// `FILE_PERSISTENT_ACLS` (winnt.h): the volume keeps ACLs (NTFS, ReFS).
const FILE_PERSISTENT_ACLS: u32 = 0x8;
/// `FILE_ATTRIBUTE_REPARSE_POINT`: a link of some kind.
const REPARSE_POINT: u32 = 0x400;
/// `FILE_ATTRIBUTE_DIRECTORY`.
const DIRECTORY: u32 = 0x10;

/// The ACL AInode's data folder gets, in SDDL: protected (`P`), full access
/// (`FA`) for SYSTEM, Administrators and the user, inherited by files and
/// folders (`OICI`).
pub fn private_sddl(user_sid: &str) -> String {
    format!("D:P(A;OICI;FA;;;{SYSTEM})(A;OICI;FA;;;{ADMINISTRATORS})(A;OICI;FA;;;{user_sid})")
}

/// Is every access `dir` grants for one of these SIDs? `sids` are the SIDs
/// its allow entries name. A drive root or a missing folder is never judged
/// here (see `ensure_private`).
pub fn only_ours(sids: &[String], user_sid: &str) -> bool {
    sids.iter().all(|s| s == user_sid || s == SYSTEM || s == ADMINISTRATORS || s == CREATOR_OWNER)
}

/// May this SID own AInode's files? The user, or SYSTEM and Administrators,
/// which own what an elevated installer or Windows itself makes.
pub fn owner_ours(owner: &str, user_sid: &str) -> bool {
    owner == user_sid || owner == SYSTEM || owner == ADMINISTRATORS
}

/// A file or folder inside the data folder is fine as it is: owned by one
/// of ours, and every allow entry names one of ours or WSL's virtual machine.
pub fn inside_ours(owner: &str, sids: &[String], user_sid: &str) -> bool {
    owner_ours(owner, user_sid)
        && sids.iter().all(|s| s.starts_with(VIRTUAL_MACHINE) || only_ours(std::slice::from_ref(s), user_sid))
}

/// What `ensure_private` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Private already (or a drive root, or no folder yet): nothing changed.
    Private,
    /// It was open: made private, and `fixed` files or folders in it reset
    /// or deleted.
    Repaired { fixed: usize },
    /// The drive keeps no permissions at all (FAT, exFAT): nothing to set.
    NoPermissions,
}

/// Make `dir` the user's alone if anyone else owns it or has access, then
/// everything in it. Never for a drive root (`D:\`): AInode's folder is
/// always a folder of its own, and a whole drive is not AInode's to lock.
/// An error means the folder is still open to someone else.
pub fn ensure_private(dir: &Path) -> Result<Outcome, String> {
    if dir.parent().is_none_or(|p| p.as_os_str().is_empty()) || !dir.is_dir() {
        return Ok(Outcome::Private);
    }
    if !keeps_permissions(dir) {
        return Ok(Outcome::NoPermissions);
    }
    let user = current_user_sid()?;
    let (owner, sids) = security(dir)?;
    if owner_ours(&owner, &user) && only_ours(&sids, &user) {
        return Ok(Outcome::Private);
    }
    if !owner_ours(&owner, &user) {
        set_owner(dir, &user).map_err(|e| format!("{} belongs to another account on this computer and cannot be taken over ({e})", dir.display()))?;
    }
    set_sddl(dir, &private_sddl(&user))?;
    let mut fixed = 0;
    repair_tree(dir, &user, &mut fixed)?;
    Ok(Outcome::Repaired { fixed })
}

/// Every file and folder below `dir` that another account owns or may use:
/// owner taken back and ACL reset to what the folder passes down. What
/// another account owns and cannot be taken back is deleted; what the user
/// owns is never deleted: if it cannot be made private, the folder is
/// reported open. Links are never followed: one the user owns is left as it
/// is, another account's is removed (only the link).
fn repair_tree(dir: &Path, user: &str, fixed: &mut usize) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read {}: {e}", dir.display()))?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.file_attributes() & REPARSE_POINT != 0 {
            // AInode makes no links in its folder. One another account made
            // while the folder was open could lead anywhere. A junction is a
            // directory to Windows but a symlink to `is_dir`: the attribute
            // says which.
            let (owner, _) = security_of(&path, false)?;
            if !owner_ours(&owner, user) {
                remove(&path, meta.file_attributes() & DIRECTORY != 0)?;
                *fixed += 1;
            }
            continue;
        }
        let (owner, sids) = security(&path)?;
        if !inside_ours(&owner, &sids, user) {
            let reset = set_owner_if_needed(&path, user).and_then(|()| inherit_only(&path));
            let now = security(&path);
            if reset.is_err() || !matches!(&now, Ok((o, s)) if inside_ours(o, s, user)) {
                if owner_ours(&owner, user) {
                    let why = reset.err().unwrap_or_else(|| "other accounts keep access".into());
                    return Err(format!(
                        "{} is yours but other accounts on this computer can open it, and AInode could not change that ({why}). \
AInode kept it: correct its permissions or move it out of AInode's folder",
                        path.display()
                    ));
                }
                remove(&path, meta.is_dir())?;
                *fixed += 1;
                continue;
            }
            *fixed += 1;
        }
        if meta.is_dir() {
            repair_tree(&path, user, fixed)?;
        }
    }
    Ok(())
}

fn remove(path: &Path, dir: bool) -> Result<(), String> {
    let r = if dir {
        // A junction or a directory link: remove_dir removes the link
        // itself; remove_dir_all never follows one either.
        std::fs::remove_dir(path).or_else(|_| std::fs::remove_dir_all(path))
    } else {
        std::fs::remove_file(path)
    };
    r.map_err(|e| format!("{} is another account's and could not be removed: {e}", path.display()))
}

fn set_owner_if_needed(path: &Path, user: &str) -> Result<(), String> {
    match security(path) {
        Ok((owner, _)) if owner_ours(&owner, user) => Ok(()),
        _ => set_owner(path, user),
    }
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// `path` as Windows' security calls take it past 260 characters: `C:\x` as
/// `\\?\C:\x`, `\\server\share\x` as `\\?\UNC\server\share\x`; a path
/// already in that form, or a relative one, as it is.
pub fn long_form(path: &Path) -> std::ffi::OsString {
    let Some(s) = path.to_str() else { return path.as_os_str().to_owned() };
    if s.starts_with(r"\\?\") || s.starts_with(r"\\.\") {
        return path.as_os_str().to_owned();
    }
    let s = s.replace('/', "\\");
    if let Some(unc) = s.strip_prefix(r"\\") {
        return format!(r"\\?\UNC\{unc}").into();
    }
    let b = s.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
        return format!(r"\\?\{s}").into();
    }
    path.as_os_str().to_owned()
}

/// Does the volume `path` is on keep ACLs?
fn keeps_permissions(path: &Path) -> bool {
    let name = wide(path.as_os_str());
    let mut root = [0u16; 1024];
    let mut flags = 0u32;
    // SAFETY: both buffers outlive the calls and their sizes are passed.
    unsafe {
        if GetVolumePathNameW(name.as_ptr(), root.as_mut_ptr(), root.len() as u32) == 0 {
            return true;
        }
        if GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut flags,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            return true;
        }
    }
    flags & FILE_PERSISTENT_ACLS != 0
}

/// A SID as text (`S-1-5-21-...`).
fn sid_string(sid: PSID) -> Result<String, String> {
    let mut out: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sid` points into a buffer the caller keeps alive; `out` is
    // allocated by the call and freed with LocalFree below.
    unsafe {
        if ConvertSidToStringSidW(sid, &mut out) == 0 {
            return Err(format!("ConvertSidToStringSidW: {}", std::io::Error::last_os_error()));
        }
        let len = (0..).take_while(|&i| *out.add(i) != 0).count();
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(out, len));
        LocalFree(out.cast());
        Ok(s)
    }
}

/// The SID of the user this process runs as.
pub fn current_user_sid() -> Result<String, String> {
    // SAFETY: the token handle is closed before returning; the buffer is
    // sized by the first GetTokenInformation call and outlives sid_string.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(format!("OpenProcessToken: {}", std::io::Error::last_os_error()));
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
        CloseHandle(token);
        if ok == 0 {
            return Err(format!("GetTokenInformation: {}", std::io::Error::last_os_error()));
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        sid_string(user.User.Sid)
    }
}

/// The SIDs `path`'s allow entries name (deny entries only take away).
pub fn allowed_sids(path: &Path) -> Result<Vec<String>, String> {
    security(path).map(|(_, sids)| sids)
}

/// `path`'s owner and the SIDs its allow entries name, of any allow kind; an
/// allow entry whose SID cannot be read counts as `UNKNOWN`. A link is
/// followed (the data folder itself may be one).
pub fn security(path: &Path) -> Result<(String, Vec<String>), String> {
    security_of(path, true)
}

/// As `security`; with `follow` false a link's own owner and ACL, never its
/// target's.
fn security_of(path: &Path, follow: bool) -> Result<(String, Vec<String>), String> {
    let name = wide(&long_form(path));
    let mut owner: PSID = std::ptr::null_mut();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `owner` and `dacl` point into `sd`, which the call allocates
    // and we free with LocalFree after the last use of either; the handle is
    // closed right after the call that reads through it.
    unsafe {
        let what = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let err = if follow {
            GetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, what, &mut owner, std::ptr::null_mut(), &mut dacl, std::ptr::null_mut(), &mut sd)
        } else {
            let h = CreateFileW(
                name.as_ptr(),
                READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            );
            if h == INVALID_HANDLE_VALUE {
                return Err(format!("open {}: {}", path.display(), std::io::Error::last_os_error()));
            }
            let e = GetSecurityInfo(h, SE_FILE_OBJECT, what, &mut owner, std::ptr::null_mut(), &mut dacl, std::ptr::null_mut(), &mut sd);
            CloseHandle(h);
            e
        };
        if err != 0 {
            return Err(format!("read the permissions of {}: {}", path.display(), std::io::Error::from_raw_os_error(err as i32)));
        }
        let result = (|| {
            let owner = if owner.is_null() { UNKNOWN.to_string() } else { sid_string(owner)? };
            let mut sids = Vec::new();
            // No DACL at all means everyone has full access.
            if dacl.is_null() {
                sids.push("S-1-1-0".to_string());
                return Ok((owner, sids));
            }
            for i in 0..u32::from((*dacl).AceCount) {
                let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
                if GetAce(dacl, i, &mut ace) == 0 {
                    sids.push(UNKNOWN.to_string());
                    continue;
                }
                let header = &*(ace as *const ACE_HEADER);
                if !ALLOW_TYPES.contains(&header.AceType) {
                    continue;
                }
                // The plain and callback kinds keep the SID right after the
                // header and the mask; the object kinds carry GUIDs first,
                // which are never on files: counted as not ours.
                if header.AceType == 0x0 || header.AceType == 0x9 {
                    let sid = (ace as *const u8).add(std::mem::size_of::<ACE_HEADER>() + 4) as PSID;
                    sids.push(sid_string(sid).unwrap_or_else(|_| UNKNOWN.to_string()));
                } else {
                    sids.push(UNKNOWN.to_string());
                }
            }
            Ok((owner, sids))
        })();
        LocalFree(sd.cast());
        result
    }
}

/// Make `user` the owner of `path`. Allowed without privileges when the
/// ACL grants the user WRITE_OWNER (the private folder's full control does).
fn set_owner(path: &Path, user: &str) -> Result<(), String> {
    let name = wide(&long_form(path));
    let text = wide(OsStr::new(user));
    let mut sid: PSID = std::ptr::null_mut();
    // SAFETY: `sid` is allocated by the conversion and freed below.
    unsafe {
        if ConvertStringSidToSidW(text.as_ptr(), &mut sid) == 0 {
            return Err(format!("the SID {user}: {}", std::io::Error::last_os_error()));
        }
        let err = SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            sid,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        );
        LocalFree(sid.cast());
        if err != 0 {
            return Err(format!("take over {}: {}", path.display(), std::io::Error::from_raw_os_error(err as i32)));
        }
    }
    Ok(())
}

/// Set `path`'s DACL from SDDL, protected, so it inherits nothing and its
/// files and folders inherit it.
fn set_sddl(path: &Path, sddl: &str) -> Result<(), String> {
    set_dacl(path, sddl, PROTECTED_DACL_SECURITY_INFORMATION)
}

/// Drop `path`'s own entries: it keeps only what its folder passes down.
fn inherit_only(path: &Path) -> Result<(), String> {
    set_dacl(path, "D:", UNPROTECTED_DACL_SECURITY_INFORMATION)
}

fn set_dacl(path: &Path, sddl: &str, how: u32) -> Result<(), String> {
    let name = wide(&long_form(path));
    let text = wide(OsStr::new(sddl));
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `sd` is allocated by the conversion and freed below; `dacl`
    // points into it and is only used before that.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut()) == 0 {
            return Err(format!("the permissions {sddl}: {}", std::io::Error::last_os_error()));
        }
        let (mut present, mut defaulted) = (0, 0);
        let mut dacl: *mut ACL = std::ptr::null_mut();
        if GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) == 0 || present == 0 {
            LocalFree(sd.cast());
            return Err("no DACL in the permissions".into());
        }
        let err = SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | how,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl,
            std::ptr::null(),
        );
        LocalFree(sd.cast());
        if err != 0 {
            return Err(format!("set the permissions of {}: {}", path.display(), std::io::Error::from_raw_os_error(err as i32)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sddl_names_three_and_is_protected() {
        assert_eq!(
            private_sddl("S-1-5-21-1-2-3-1001"),
            "D:P(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;S-1-5-32-544)(A;OICI;FA;;;S-1-5-21-1-2-3-1001)"
        );
    }

    #[test]
    fn only_the_user_system_and_administrators_count_as_ours() {
        let me = "S-1-5-21-1-2-3-1001";
        let ours: Vec<String> = [me, SYSTEM, ADMINISTRATORS, CREATOR_OWNER].iter().map(|s| s.to_string()).collect();
        assert!(only_ours(&ours, me));
        for other in ["S-1-5-11", "S-1-5-32-545", "S-1-1-0", "S-1-5-21-1-2-3-1002", UNKNOWN] {
            let mut v = ours.clone();
            v.push(other.into());
            assert!(!only_ours(&v, me), "{other} is not ours");
        }
        assert!(owner_ours(me, me) && owner_ours(SYSTEM, me) && owner_ours(ADMINISTRATORS, me));
        assert!(!owner_ours("S-1-5-21-1-2-3-1002", me) && !owner_ours(UNKNOWN, me) && !owner_ours(CREATOR_OWNER, me));
        let vm = "S-1-5-83-1-111-222-333-444".to_string();
        assert!(inside_ours(me, &[me.to_string(), vm.clone()], me), "WSL's virtual machine on ext4.vhdx");
        assert!(!only_ours(&[vm], me), "but never on the folder itself");
    }

    /// A folder that grants Authenticated Users Modify (as `D:\AInode`
    /// inherits from `D:\`) is made private, files in it follow, and a
    /// second pass changes nothing.
    #[test]
    fn a_shared_folder_is_made_private_with_its_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("AInode");
        std::fs::create_dir(&dir).unwrap();
        let me = current_user_sid().unwrap();
        set_sddl(&dir, &format!("D:P(A;OICI;FA;;;{me})(A;OICI;0x1301bf;;;S-1-5-11)(A;OICI;0x1200a9;;;S-1-5-32-545)")).unwrap();
        std::fs::write(dir.join("settings.json"), b"{}").unwrap();
        assert!(allowed_sids(&dir.join("settings.json")).unwrap().iter().any(|s| s == "S-1-5-11"));

        assert!(matches!(ensure_private(&dir).unwrap(), Outcome::Repaired { .. }), "repaired");
        for p in [dir.clone(), dir.join("settings.json")] {
            let sids = allowed_sids(&p).unwrap();
            assert!(only_ours(&sids, &me), "{}: {sids:?}", p.display());
            assert!(sids.contains(&me), "{}: the user keeps access", p.display());
        }
        std::fs::write(dir.join("new.json"), b"{}").unwrap();
        assert!(only_ours(&allowed_sids(&dir.join("new.json")).unwrap(), &me), "a new file inherits it");
        assert_eq!(ensure_private(&dir).unwrap(), Outcome::Private, "nothing to do the second time");
    }

    /// What an open folder may hold: a file and a folder with protected ACLs
    /// of their own that keep Authenticated Users in, a conditional allow
    /// entry for Everyone, and a junction out of the folder. The repair
    /// resets each; the junction is the user's own (a test cannot make one
    /// another account owns), so it stays, and it is never followed: an open
    /// file behind it is not touched.
    #[test]
    fn what_an_open_folder_holds_is_repaired_too() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("AInode");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(dir.join("modelnet").join("deep")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let me = current_user_sid().unwrap();
        set_sddl(&dir, &format!("D:P(A;OICI;FA;;;{me})(A;OICI;0x1301bf;;;S-1-5-11)")).unwrap();
        let open = format!("D:P(A;OICI;FA;;;{me})(A;OICI;FA;;;S-1-5-11)");
        std::fs::write(dir.join("settings.json"), b"{}").unwrap();
        set_sddl(&dir.join("settings.json"), &open).unwrap();
        set_sddl(&dir.join("modelnet"), &open).unwrap();
        std::fs::write(dir.join("modelnet").join("deep").join("catalog.json"), b"{}").unwrap();
        set_sddl(&dir.join("modelnet").join("deep").join("catalog.json"), &open).unwrap();
        // A conditional allow entry: Everyone, full control, on a condition that always holds.
        std::fs::write(dir.join("consent.json"), b"{}").unwrap();
        let conditional = format!("D:P(A;;FA;;;{me})(XA;;FA;;;WD;(Member_of {{SID(WD)}}))");
        set_sddl(&dir.join("consent.json"), &conditional).unwrap();
        assert!(allowed_sids(&dir.join("consent.json")).unwrap().iter().any(|s| s == "S-1-1-0"), "the callback entry is read");
        let link = dir.join("link");
        let made = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(&link).arg(&outside).output().unwrap();
        assert!(made.status.success(), "mklink /J");
        std::fs::write(outside.join("theirs.txt"), b"x").unwrap();
        set_sddl(&outside.join("theirs.txt"), &open).unwrap();
        let outside_before = allowed_sids(&outside.join("theirs.txt")).unwrap();

        let r = ensure_private(&dir).unwrap();
        assert!(matches!(r, Outcome::Repaired { fixed } if fixed >= 4), "{r:?}");
        for p in [
            dir.join("settings.json"),
            dir.join("modelnet"),
            dir.join("modelnet").join("deep").join("catalog.json"),
            dir.join("consent.json"),
        ] {
            let (o, sids) = security(&p).unwrap();
            assert!(inside_ours(&o, &sids, &me), "{}: {o} {sids:?}", p.display());
        }
        assert!(std::fs::symlink_metadata(&link).is_ok(), "the user's own junction stays");
        assert_eq!(allowed_sids(&outside.join("theirs.txt")).unwrap(), outside_before, "nothing behind it was touched");
        assert_eq!(ensure_private(&dir).unwrap(), Outcome::Private);
    }

    /// A file deeper than 260 characters in an open folder (a deep folder the
    /// user chose): read and repaired in its long form, never taken for a
    /// foreign file and deleted (the Mac's review of 0.8.3).
    #[test]
    fn a_file_past_260_characters_is_repaired_not_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("AInode");
        let me = current_user_sid().unwrap();
        std::fs::create_dir(&dir).unwrap();
        set_sddl(&dir, &format!("D:P(A;OICI;FA;;;{me})(A;OICI;0x1301bf;;;S-1-5-11)")).unwrap();
        let mut deep = dir.join("modelnet");
        while deep.as_os_str().len() < 300 {
            deep = deep.join("a-folder-name-of-forty-characters-long-x");
        }
        std::fs::create_dir_all(&deep).unwrap();
        let file = deep.join("piece.bin");
        std::fs::write(&file, b"model bytes").unwrap();
        assert!(file.as_os_str().len() > 260, "{}", file.as_os_str().len());
        set_sddl(&file, &format!("D:P(A;;FA;;;{me})(A;;FA;;;S-1-5-11)")).unwrap();
        assert!(matches!(ensure_private(&dir).unwrap(), Outcome::Repaired { .. }));
        assert_eq!(std::fs::read(&file).unwrap(), b"model bytes", "kept, not deleted");
        let (o, sids) = security(&file).unwrap();
        assert!(inside_ours(&o, &sids, &me), "{o} {sids:?}");
    }

    /// A file of the user's own that stays open to others and cannot be
    /// changed (an entry denies even its owner WRITE_DAC) is kept, and the
    /// folder is reported open instead of the file being deleted.
    #[test]
    fn an_own_file_that_cannot_be_fixed_is_kept_and_the_folder_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("AInode");
        let me = current_user_sid().unwrap();
        std::fs::create_dir(&dir).unwrap();
        set_sddl(&dir, &format!("D:P(A;OICI;FA;;;{me})(A;OICI;0x1301bf;;;S-1-5-11)")).unwrap();
        let file = dir.join("chat.json");
        std::fs::write(&file, b"{}").unwrap();
        // OWNER RIGHTS (OW) denied WRITE_DAC: not even the owner may change it.
        set_sddl(&file, &format!("D:P(D;;WD;;;OW)(A;;FA;;;{me})(A;;FR;;;S-1-5-11)")).unwrap();
        let err = ensure_private(&dir).unwrap_err();
        assert!(err.contains("chat.json") && err.contains("kept it"), "{err}");
        assert_eq!(std::fs::read(&file).unwrap(), b"{}", "the user's file is still there");
        // The test's own clean-up: give the owner its rights back.
        let _ = std::process::Command::new("icacls").arg(&file).arg("/reset").output();
    }

    /// A file held open with no sharing at all (as WSL holds `ext4.vhdx`
    /// while its virtual machine runs) still gets the private ACL: setting
    /// permissions is not a read, write or delete.
    #[test]
    fn a_file_in_use_is_repaired_too() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("AInode");
        std::fs::create_dir_all(dir.join("wsl")).unwrap();
        let me = current_user_sid().unwrap();
        set_sddl(&dir, &format!("D:P(A;OICI;FA;;;{me})(A;OICI;0x1301bf;;;S-1-5-11)")).unwrap();
        let disk = dir.join("wsl").join("ext4.vhdx");
        std::fs::write(&disk, b"disk").unwrap();
        let _held = std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(&disk).unwrap();
        assert!(matches!(ensure_private(&dir).unwrap(), Outcome::Repaired { .. }));
        let (o, sids) = security(&disk).unwrap();
        assert!(inside_ours(&o, &sids, &me), "{o} {sids:?}");
        assert!(disk.is_file(), "kept, not deleted");
    }

    /// The real case: a folder made at the root of a second NTFS drive
    /// inherits that root's ACL. Runs where there is a `D:\` (the PC); a
    /// throwaway folder, removed again.
    #[test]
    fn a_folder_on_a_second_drive_is_made_private() {
        let root = Path::new(r"D:\");
        if !root.is_dir() || !allowed_sids(root).is_ok_and(|s| s.iter().any(|x| x == "S-1-5-11")) {
            eprintln!("no D: drive that grants Authenticated Users; skipped");
            return;
        }
        let tmp = tempfile::Builder::new().prefix("ainode-acl-test").tempdir_in(root).unwrap();
        let dir = tmp.path().join("AInode");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), b"{}").unwrap();
        let me = current_user_sid().unwrap();
        assert!(!only_ours(&allowed_sids(&dir).unwrap(), &me), "inherited from D:\\ as on the PC");
        assert!(matches!(ensure_private(&dir).unwrap(), Outcome::Repaired { .. }));
        assert!(only_ours(&allowed_sids(&dir.join("settings.json")).unwrap(), &me));
        let (owner, _) = security(&dir).unwrap();
        assert!(owner_ours(&owner, &me), "owner {owner}");
    }

    #[test]
    fn a_drive_root_or_a_missing_folder_is_left_alone() {
        assert_eq!(ensure_private(Path::new(r"C:\")).unwrap(), Outcome::Private);
        assert_eq!(ensure_private(Path::new(r"C:\no\such\AInode\folder")).unwrap(), Outcome::Private);
    }
}
