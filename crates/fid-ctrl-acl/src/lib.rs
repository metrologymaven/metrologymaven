//! Windows ACL shim for the control-channel token file. Spec node `AUTO.CTRL.AUTH`,
//! REQ-AUTO.CTRL.AUTH-001 and -002.
//!
//! This is the named `unsafe` exception of decisions/D-048: Win32 security calls for the token
//! file only, in `mod ffi` only. The rest of the crate, and every caller, is safe code.
//!
//! - [`create_owner_only`] creates a new file (`CREATE_NEW`, never opening an existing one)
//!   whose security descriptor is set *at creation*: owner = the process user's SID, and a
//!   protected DACL with one ACE granting that SID full access. No other principal can open
//!   the file between its creation and the DACL being in place, because there is no such gap.
//! - [`check_owner_only`] reads the owner and DACL back from an open handle and reports
//!   [`Access::Wider`] when another principal could get access. Wider means one of: an owner
//!   that is not the process user (BUILTIN\Administrators is accepted only when the caller
//!   says the file pre-existed), a null DACL, an allow ACE for another SID, a malformed ACE,
//!   or any ACE type other than plain allow or deny (fail closed).
//!
//! The crate is empty on every target other than Windows.
#![cfg(windows)]

use std::fs::File;
use std::io;
use std::path::Path;

/// Result of [`check_owner_only`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Only the process user has access.
    OwnerOnly,
    /// Some other principal could get access; the reason is a short fixed text.
    Wider(&'static str),
}

/// Creates `path` (it must not exist) with owner = the process user and a protected DACL that
/// grants full access to that user only, and returns it opened for reading and writing.
pub fn create_owner_only(path: &Path) -> io::Result<File> {
    ffi::create(path)
}

/// Checks that only the process user can access `file`. The handle needs `READ_CONTROL`
/// access, which every handle opened for reading has. With `allow_admin_owner`, an owner of
/// BUILTIN\Administrators (S-1-5-32-544) is accepted too, for files this process did not create.
pub fn check_owner_only(file: &File, allow_admin_owner: bool) -> io::Result<Access> {
    ffi::check(file, allow_admin_owner)
}

/// The SID string (`S-1-5-21-...`) of the process user, for diagnostics and tests. Never secret.
pub fn current_user_sid_string() -> io::Result<String> {
    ffi::user_sid().map(|s| s.to_string_form())
}

/// BUILTIN\Administrators, S-1-5-32-544, in binary form.
const ADMINISTRATORS: [u8; 16] = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 0x02, 0, 0];

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

#[allow(unsafe_code)]
mod ffi {
    use super::{sid_len, Access, Sid, ADMINISTRATORS};
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
    use windows_sys::Win32::Storage::FileSystem::CreateFileW;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// Constants from winnt.h / fileapi.h.
    const FILE_ALL_ACCESS: u32 = 0x001F_01FF;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const CREATE_NEW: u32 = 1;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
    const SE_DACL_PROTECTED: u16 = 0x1000;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;
    const INHERIT_ONLY_ACE: u8 = 0x08;
    /// Largest token information buffer accepted (a SID is at most 68 bytes).
    const MAX_TOKEN_INFO: u32 = 64 * 1024;
    /// `sizeof(ACL)`, and the offset of the SID in an allow ACE (`SidStart`).
    const ACL_HEADER_BYTES: usize = std::mem::size_of::<ACL>();
    const ACE_FIXED_BYTES: usize = std::mem::size_of::<ACCESS_ALLOWED_ACE>() - std::mem::size_of::<u32>();

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

    pub(super) fn check(file: &File, allow_admin_owner: bool) -> io::Result<Access> {
        let user = user_sid()?;
        let handle = file.as_raw_handle();
        let mut owner: PSID = null_mut();
        let mut dacl: *mut ACL = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: the out-pointers are live locals; on success `owner` and `dacl` point into
        // the descriptor `sd`, which the guard frees only after the last use below. The owner
        // SID is validated by IsValidSid before its length is trusted. Each ACE pointer comes
        // from GetAce on that DACL with an index below its AceCount; the header is read first,
        // and an allow ACE's SID is read only within the ACE's own AceSize.
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
            if owner.is_null() || IsValidSid(owner) == 0 {
                return Ok(Access::Wider("the file has no valid owner"));
            }
            let owner = copy_sid(owner, GetLengthSid(owner) as usize)?;
            let admin_ok = allow_admin_owner && owner.bytes == ADMINISTRATORS;
            if owner.bytes != user.bytes && !admin_ok {
                return Ok(Access::Wider("the file is owned by another principal"));
            }
            if dacl.is_null() {
                return Ok(Access::Wider("the file has a null DACL, which grants everyone access"));
            }
            let count = ptr::read_unaligned(dacl).AceCount;
            for i in 0..count {
                let mut ace: *mut c_void = null_mut();
                if GetAce(dacl, u32::from(i), &mut ace) == 0 || ace.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let header = ptr::read_unaligned(ace.cast::<ACE_HEADER>());
                if header.AceFlags & INHERIT_ONLY_ACE != 0 {
                    continue;
                }
                match header.AceType {
                    ACCESS_DENIED_ACE_TYPE => continue,
                    ACCESS_ALLOWED_ACE_TYPE => {
                        let size = usize::from(header.AceSize);
                        if size < ACE_FIXED_BYTES + sid_len(0) {
                            return Ok(Access::Wider("malformed allow ACE"));
                        }
                        let sid = ace.cast::<u8>().add(ACE_FIXED_BYTES).cast::<c_void>();
                        match copy_sid(sid, size - ACE_FIXED_BYTES) {
                            Ok(s) if s.bytes == user.bytes => {}
                            Ok(_) => return Ok(Access::Wider("the DACL grants access to another principal")),
                            Err(_) => return Ok(Access::Wider("malformed allow ACE")),
                        }
                    }
                    _ => return Ok(Access::Wider("the DACL holds an ACE type this check does not accept")),
                }
            }
        }
        Ok(Access::OwnerOnly)
    }
}
