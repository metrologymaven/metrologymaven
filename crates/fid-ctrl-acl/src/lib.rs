//! Windows ACL shim for the control-channel token file. Spec node `AUTO.CTRL.AUTH`,
//! REQ-AUTO.CTRL.AUTH-001 and -002.
//!
//! This is the named `unsafe` exception of decisions/D-048: Win32 security calls for the token
//! file only, in `mod ffi` only. The rest of the crate, and every caller, is safe code. `mod ffi`
//! only copies data out of the OS (the process user's SID, a handle's owner and DACL, a file's
//! identity) and creates the file; every decision is made on those copies in safe code.
//!
//! - [`create_owner_only`] creates a new file (`CREATE_NEW`, never opening an existing one)
//!   whose security descriptor is set *at creation*: owner = the process user's SID, and a
//!   protected DACL with one ACE granting that SID full access. No other principal can open
//!   the file between its creation and the DACL being in place, because there is no such gap.
//!   The applied descriptor is then read back and checked with [`check_owner_only`]; if it is
//!   not owner-only, the file is deleted and an error returned before any secret is written.
//! - [`check_owner_only`] reads the owner and DACL back from an open handle and reports
//!   [`Access::Wider`] when another principal could get access. Wider means one of: an owner
//!   that is not the process user (BUILTIN\Administrators is accepted only when the caller
//!   says the file pre-existed), a null DACL, an allow ACE for another SID, a malformed ACE,
//!   or any ACE type other than plain allow or deny (fail closed).
//! - [`check_dir_private`] checks a directory handle: the owner and every allow ACE must be the
//!   process user, LocalSystem or BUILTIN\Administrators, except allow ACEs that grant no
//!   write, delete or permission-changing right. Inherit-only ACEs do not apply to the
//!   directory itself and are skipped.
//! - [`file_id`] is the file's identity (volume serial number and file index).
//!
//! The crate is empty on every target other than Windows.
#![cfg(windows)]

use std::fs::File;
use std::io;
use std::path::Path;

/// Result of [`check_owner_only`] and [`check_dir_private`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Only the process user has access (for a directory: no other principal can write).
    OwnerOnly,
    /// Some other principal could get access; the reason is a short fixed text.
    Wider(&'static str),
}

/// The identity of an open file: the volume serial number and the 64-bit file index, as
/// `GetFileInformationByHandle` reports them. Two handles with equal identities refer to the
/// same file while it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId {
    pub volume: u32,
    pub index: u64,
}

/// Creates `path` (it must not exist) with owner = the process user and a protected DACL that
/// grants full access to that user only, and returns it opened for reading and writing. The
/// descriptor actually applied is read back and checked; if it is not owner-only the new
/// (still empty) file is deleted and a `PermissionDenied` error returned.
pub fn create_owner_only(path: &Path) -> io::Result<File> {
    let file = ffi::create(path)?;
    let why = match check_owner_only(&file, false) {
        Ok(Access::OwnerOnly) => return Ok(file),
        Ok(Access::Wider(w)) => io::Error::new(io::ErrorKind::PermissionDenied, format!("new token file: {w}")),
        Err(e) => e,
    };
    drop(file);
    match std::fs::remove_file(path) {
        Ok(()) => Err(why),
        Err(e) => Err(io::Error::new(why.kind(), format!("{why}; removing the new file also failed: {e}"))),
    }
}

/// Checks that only the process user can access `file`. The handle needs `READ_CONTROL`
/// access, which every handle opened for reading has. With `allow_admin_owner`, an owner of
/// BUILTIN\Administrators (S-1-5-32-544) is accepted too, for files this process did not create.
pub fn check_owner_only(file: &File, allow_admin_owner: bool) -> io::Result<Access> {
    let user = ffi::user_sid()?;
    let sec = ffi::security(file)?;
    let Some(owner) = sec.owner else {
        return Ok(Access::Wider("the file has no valid owner"));
    };
    let admin_ok = allow_admin_owner && owner.bytes == ADMINISTRATORS;
    if owner.bytes != user.bytes && !admin_ok {
        return Ok(Access::Wider("the file is owned by another principal"));
    }
    let Some(aces) = sec.dacl else {
        return Ok(Access::Wider("the file has a null DACL, which grants everyone access"));
    };
    for ace in aces {
        if ace.flags & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        match ace.kind {
            ACCESS_DENIED_ACE_TYPE => continue,
            ACCESS_ALLOWED_ACE_TYPE => match ace.sid {
                Some(s) if s.bytes == user.bytes => {}
                Some(_) => return Ok(Access::Wider("the DACL grants access to another principal")),
                None => return Ok(Access::Wider("malformed allow ACE")),
            },
            _ => return Ok(Access::Wider("the DACL holds an ACE type this check does not accept")),
        }
    }
    Ok(Access::OwnerOnly)
}

/// Checks that no principal other than the process user, LocalSystem (S-1-5-18) and
/// BUILTIN\Administrators (S-1-5-32-544) can create, delete, rename or re-permission entries in
/// the directory `dir`, or change the directory itself. `dir` must be a handle to the directory
/// opened with `READ_CONTROL` (any handle opened for reading).
pub fn check_dir_private(dir: &File) -> io::Result<Access> {
    let user = ffi::user_sid()?;
    let sec = ffi::security(dir)?;
    let trusted = |b: &[u8]| b == user.bytes.as_slice() || b == ADMINISTRATORS || b == LOCAL_SYSTEM;
    match &sec.owner {
        Some(o) if trusted(&o.bytes) => {}
        Some(_) => return Ok(Access::Wider("the directory is owned by another principal")),
        None => return Ok(Access::Wider("the directory has no valid owner")),
    }
    let Some(aces) = sec.dacl else {
        return Ok(Access::Wider("the directory has a null DACL, which grants everyone access"));
    };
    for ace in aces {
        if ace.flags & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        match ace.kind {
            ACCESS_DENIED_ACE_TYPE => continue,
            ACCESS_ALLOWED_ACE_TYPE => match ace.sid {
                Some(s) if trusted(&s.bytes) => {}
                Some(_) if ace.mask & DIR_WRITE_RIGHTS == 0 => {}
                Some(_) => return Ok(Access::Wider("the directory DACL grants another principal write access")),
                None => return Ok(Access::Wider("malformed allow ACE on the directory")),
            },
            _ => return Ok(Access::Wider("the directory DACL holds an ACE type this check does not accept")),
        }
    }
    Ok(Access::OwnerOnly)
}

/// The identity of the open file `file` (see [`FileId`]).
pub fn file_id(file: &File) -> io::Result<FileId> {
    ffi::file_id(file)
}

/// The SID string (`S-1-5-21-...`) of the process token user. Never secret. It is the OS user
/// of every session on Windows (AUTO.SURF.COMPOSE REQ-007, `fid_compose::process_os_user`), so
/// it must come from the process token and nothing else.
pub fn current_user_sid_string() -> io::Result<String> {
    ffi::user_sid().map(|s| s.to_string_form())
}

/// BUILTIN\Administrators, S-1-5-32-544, in binary form.
const ADMINISTRATORS: [u8; 16] = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 0x02, 0, 0];
/// NT AUTHORITY\SYSTEM (LocalSystem), S-1-5-18, in binary form.
const LOCAL_SYSTEM: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];

/// ACE types and flags from winnt.h.
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;
const INHERIT_ONLY_ACE: u8 = 0x08;

/// Access rights that let a principal add, remove, rename or re-permission directory entries
/// or change the directory itself (winnt.h): FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY,
/// FILE_WRITE_EA, FILE_DELETE_CHILD, FILE_WRITE_ATTRIBUTES, DELETE, WRITE_DAC, WRITE_OWNER,
/// MAXIMUM_ALLOWED, GENERIC_ALL and GENERIC_WRITE.
const DIR_WRITE_RIGHTS: u32 = 0x0002
    | 0x0004
    | 0x0010
    | 0x0040
    | 0x0100
    | 0x0001_0000
    | 0x0004_0000
    | 0x0008_0000
    | 0x0200_0000
    | 0x1000_0000
    | 0x4000_0000;

/// Bytes a SID of `sub_count` subauthorities occupies: revision, count, 6-byte authority, then
/// 4 bytes per subauthority.
fn sid_len(sub_count: u8) -> usize {
    8 + 4 * usize::from(sub_count)
}

/// An owned copy of a SID: its bytes (for comparison, which is what `EqualSid` does) and a
/// 4-byte aligned buffer with the same bytes (to pass as a `PSID`).
struct Sid {
    bytes: Vec<u8>,
    aligned: Vec<u32>,
}

impl Sid {
    fn from_bytes(bytes: Vec<u8>) -> Sid {
        let aligned = bytes
            .chunks(4)
            .map(|c| {
                let mut w = [0u8; 4];
                w[..c.len()].copy_from_slice(c);
                u32::from_ne_bytes(w)
            })
            .collect();
        Sid { bytes, aligned }
    }

    /// `S-R-I-S1-S2-...` from the binary layout.
    fn to_string_form(&self) -> String {
        let b = &self.bytes;
        if b.len() < 8 {
            return String::from("S-?");
        }
        let authority = b[2..8].iter().fold(0u64, |acc, &x| (acc << 8) | u64::from(x));
        let mut s = format!("S-{}-{}", b[0], authority);
        for c in b[8..].chunks_exact(4) {
            s.push_str(&format!("-{}", u32::from_le_bytes([c[0], c[1], c[2], c[3]])));
        }
        s
    }
}

/// One ACE copied out of a DACL.
struct Ace {
    kind: u8,
    flags: u8,
    /// The access mask; 0 when the ACE is too short to hold one.
    mask: u32,
    /// The SID of an allow or deny ACE; `None` for other types or a malformed ACE.
    sid: Option<Sid>,
}

/// Owner and DACL copied out of a security descriptor.
struct Security {
    /// `None` when the descriptor has no owner or an invalid one.
    owner: Option<Sid>,
    /// `None` for a null DACL (which grants everyone access).
    dacl: Option<Vec<Ace>>,
}

#[allow(unsafe_code)]
mod ffi {
    use super::{sid_len, Ace, FileId, Security, Sid, ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::Path;
    use std::ptr::{self, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        AddAccessAllowedAce, GetAce, GetLengthSid, GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor,
        IsValidSid, SetSecurityDescriptorControl, SetSecurityDescriptorDacl, SetSecurityDescriptorOwner, TokenUser,
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR, TOKEN_QUERY,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// Constants from winnt.h / fileapi.h.
    const FILE_ALL_ACCESS: u32 = 0x001F_01FF;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const CREATE_NEW: u32 = 1;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
    const SE_DACL_PROTECTED: u16 = 0x1000;
    /// Largest token information buffer accepted (a SID is at most 68 bytes).
    const MAX_TOKEN_INFO: u32 = 64 * 1024;
    /// `sizeof(ACL)`, and the offset of the SID in an allow or deny ACE (`SidStart`).
    const ACL_HEADER_BYTES: usize = std::mem::size_of::<ACL>();
    const ACE_FIXED_BYTES: usize = std::mem::size_of::<ACCESS_ALLOWED_ACE>() - std::mem::size_of::<u32>();
    /// The ACE header is followed by the 4-byte access mask.
    const ACE_HEADER_BYTES: usize = std::mem::size_of::<ACE_HEADER>();
    const ACE_MASK_END: usize = ACE_HEADER_BYTES + std::mem::size_of::<u32>();

    impl Sid {
        fn as_psid(&self) -> PSID {
            self.aligned.as_ptr().cast_mut().cast()
        }
    }

    /// Copies a SID out of OS memory, reading at most `avail` bytes from `psid`.
    ///
    /// # Safety
    /// `psid` must be null or point to at least `avail` readable bytes that stay valid for the
    /// duration of the call.
    unsafe fn copy_sid(psid: PSID, avail: usize) -> io::Result<Sid> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid SID");
        if psid.is_null() || avail < sid_len(0) {
            return Err(invalid());
        }
        // SAFETY: `avail >= 8` bytes are readable (caller), so the subauthority count at byte 1
        // can be read; the full SID length is then checked against `avail` before IsValidSid
        // and GetLengthSid read the rest, and the copy reads exactly that length.
        unsafe {
            let need = sid_len(ptr::read_unaligned(psid.cast::<u8>().add(1)));
            if need > avail || IsValidSid(psid) == 0 || GetLengthSid(psid) as usize != need {
                return Err(invalid());
            }
            Ok(Sid::from_bytes(std::slice::from_raw_parts(psid.cast::<u8>(), need).to_vec()))
        }
    }

    struct HandleGuard(HANDLE);

    impl Drop for HandleGuard {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcessToken and is closed exactly once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    struct LocalGuard(PSECURITY_DESCRIPTOR);

    impl Drop for LocalGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the descriptor was allocated by GetSecurityInfo, which documents
                // LocalFree as its release, and is freed exactly once.
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
    }

    /// The process token's user SID.
    pub(super) fn user_sid() -> io::Result<Sid> {
        // SAFETY: every pointer passed is either null with a zero length or points into a
        // live, large enough local buffer; the token handle is closed by the guard. TOKEN_USER
        // starts with a PSID that points into the same buffer, which outlives the copy; the
        // pointer is checked to lie inside the buffer, and at most the buffer's remaining bytes
        // are read.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let _guard = HandleGuard(token);
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
            if len == 0 || len > MAX_TOKEN_INFO {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "token information size"));
            }
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let cap = buf.len() * 8;
            if GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast::<c_void>(), cap as u32, &mut len) == 0 {
                return Err(io::Error::last_os_error());
            }
            let psid: PSID = ptr::read(buf.as_ptr().cast::<PSID>());
            let start = buf.as_ptr() as usize;
            let at = psid as usize;
            if at < start || at >= start + cap {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "token SID outside its buffer"));
            }
            copy_sid(psid, start + cap - at)
        }
    }

    pub(super) fn create(path: &Path) -> io::Result<File> {
        let user = user_sid()?;
        let acl_bytes = (ACL_HEADER_BYTES + ACE_FIXED_BYTES + user.bytes.len()).div_ceil(4) * 4;
        let mut acl = vec![0u32; acl_bytes / 4];
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"));
        }
        wide.push(0);
        let mut sd = SECURITY_DESCRIPTOR::default();
        // SAFETY: `acl` is a 4-byte aligned buffer of `acl_bytes` bytes, enough for the ACL
        // header and one allow ACE with this SID. `sd` is an absolute security descriptor on the
        // stack whose owner and DACL pointers refer to `user` and `acl`, which outlive the
        // CreateFileW call that copies them into the new file. `wide` is NUL-terminated. The
        // returned handle is valid and owned by exactly one File.
        unsafe {
            let pacl = acl.as_mut_ptr().cast::<ACL>();
            if InitializeAcl(pacl, acl_bytes as u32, ACL_REVISION) == 0
                || AddAccessAllowedAce(pacl, ACL_REVISION, FILE_ALL_ACCESS, user.as_psid()) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let psd: PSECURITY_DESCRIPTOR = ptr::addr_of_mut!(sd).cast();
            if InitializeSecurityDescriptor(psd, SECURITY_DESCRIPTOR_REVISION) == 0
                || SetSecurityDescriptorOwner(psd, user.as_psid(), 0) == 0
                || SetSecurityDescriptorDacl(psd, 1, pacl, 0) == 0
                || SetSecurityDescriptorControl(psd, SE_DACL_PROTECTED, SE_DACL_PROTECTED) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: psd,
                bInheritHandle: 0,
            };
            let h = CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                &sa,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            );
            if h == INVALID_HANDLE_VALUE || h.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(File::from_raw_handle(h))
        }
    }

    /// Copies the owner and DACL of the object behind `file`.
    pub(super) fn security(file: &File) -> io::Result<Security> {
        let handle = file.as_raw_handle();
        let mut owner: PSID = null_mut();
        let mut dacl: *mut ACL = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: the out-pointers are live locals; on success `owner` and `dacl` point into
        // the descriptor `sd`, which the guard frees only after the last use below. The owner
        // SID is validated by IsValidSid before its length is trusted. Each ACE pointer comes
        // from GetAce on that DACL with an index below its AceCount; the header is read first,
        // the access mask only when AceSize covers it, and an allow or deny ACE's SID only
        // within the ACE's own AceSize.
        unsafe {
            let rc = GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut sd,
            );
            if rc != 0 {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
            let _sd = LocalGuard(sd);
            let owner = if owner.is_null() || IsValidSid(owner) == 0 {
                None
            } else {
                copy_sid(owner, GetLengthSid(owner) as usize).ok()
            };
            if dacl.is_null() {
                return Ok(Security { owner, dacl: None });
            }
            let count = ptr::read_unaligned(dacl).AceCount;
            let mut aces = Vec::with_capacity(usize::from(count));
            for i in 0..count {
                let mut ace: *mut c_void = null_mut();
                if GetAce(dacl, u32::from(i), &mut ace) == 0 || ace.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let header = ptr::read_unaligned(ace.cast::<ACE_HEADER>());
                let size = usize::from(header.AceSize);
                let mask = if size >= ACE_MASK_END {
                    ptr::read_unaligned(ace.cast::<u8>().add(ACE_HEADER_BYTES).cast::<u32>())
                } else {
                    0
                };
                let sid = match header.AceType {
                    ACCESS_ALLOWED_ACE_TYPE | ACCESS_DENIED_ACE_TYPE if size >= ACE_FIXED_BYTES + sid_len(0) => {
                        let sid = ace.cast::<u8>().add(ACE_FIXED_BYTES).cast::<c_void>();
                        copy_sid(sid, size - ACE_FIXED_BYTES).ok()
                    }
                    _ => None,
                };
                aces.push(Ace { kind: header.AceType, flags: header.AceFlags, mask, sid });
            }
            Ok(Security { owner, dacl: Some(aces) })
        }
    }

    /// The volume serial number and file index of the file behind `file`.
    pub(super) fn file_id(file: &File) -> io::Result<FileId> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is valid for the lifetime of `file`, and `info` is a live local of
        // the type the call writes.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(FileId {
            volume: info.dwVolumeSerialNumber,
            index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        })
    }
}
