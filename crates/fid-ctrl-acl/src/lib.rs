//! Windows ACL shim for the control-channel token file. Spec node `AUTO.CTRL.AUTH`,
//! REQ-AUTO.CTRL.AUTH-001 and -002.
//!
//! This is the named `unsafe` exception the node's build prompt allows ("no `unsafe` beyond
//! the OS ACL shim"). It does two things, both on an open file handle so there is no
//! check-then-use window on the path:
//!
//! - [`restrict_to_current_user`] replaces the file's DACL with a *protected* DACL holding one
//!   ACE: full access for the process user's SID. Inherited ACEs are removed.
//! - [`check_owner_only`] reads the owner and DACL back and reports [`Access::Wider`] if any
//!   principal other than the process user could get access: an owner that is neither the
//!   process user nor the token's default owner, a null DACL, an allow ACE for any other SID,
//!   or any ACE type other than plain allow or deny (fail closed).
//!
//! The crate is empty on every target other than Windows. Unix permissions need no shim
//! (`std::os::unix` exposes mode and owner).
#![cfg(windows)]

use std::fs::File;
use std::io;

/// Result of [`check_owner_only`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Only the process user (and the owner, which is that user or the token's default owner)
    /// has access.
    OwnerOnly,
    /// Some other principal could get access; the reason is a short fixed text.
    Wider(&'static str),
}

/// Replaces `file`'s DACL with a protected DACL that grants full access to the process
/// user's SID only. The handle must have been opened with `WRITE_DAC` access.
pub fn restrict_to_current_user(file: &File) -> io::Result<()> {
    ffi::restrict(file)
}

/// Checks that only the process user can access `file`. The handle needs `READ_CONTROL`
/// access, which every handle opened for reading has.
pub fn check_owner_only(file: &File) -> io::Result<Access> {
    ffi::check(file)
}

/// The SID string (`S-1-5-21-...`) of the process user, for diagnostics and tests. Never
/// secret.
pub fn current_user_sid_string() -> io::Result<String> {
    ffi::token_sid(ffi::Which::User).map(|s| s.to_string_form())
}

#[allow(unsafe_code)]
mod ffi {
    use super::Access;
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::ptr::{self, null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        AddAccessAllowedAce, GetAce, GetLengthSid, GetTokenInformation, InitializeAcl, IsValidSid, TokenOwner,
        TokenUser, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// `FILE_ALL_ACCESS` (winnt.h).
    const FILE_ALL_ACCESS: u32 = 0x001F_01FF;
    /// ACE types and flags (winnt.h).
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;
    const INHERIT_ONLY_ACE: u8 = 0x08;
    /// Largest token information buffer accepted (a SID is at most 68 bytes).
    const MAX_TOKEN_INFO: u32 = 64 * 1024;
    /// `sizeof(ACL)` and `sizeof(ACCESS_ALLOWED_ACE) - sizeof(DWORD)` (the SID starts at `SidStart`).
    const ACL_HEADER_BYTES: usize = std::mem::size_of::<ACL>();
    const ACE_FIXED_BYTES: usize = std::mem::size_of::<ACCESS_ALLOWED_ACE>() - std::mem::size_of::<u32>();

    /// An owned copy of a SID: its bytes (for comparison, which is what `EqualSid` does) and a
    /// 4-byte aligned buffer with the same bytes (to pass as a `PSID`).
    pub(super) struct Sid {
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

        fn as_psid(&self) -> PSID {
            self.aligned.as_ptr().cast_mut().cast()
        }

        /// `S-R-I-S1-S2-...` from the binary layout (revision, count, 6-byte authority, subauthorities).
        pub(super) fn to_string_form(&self) -> String {
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

    /// Copies a SID out of memory owned by the OS.
    ///
    /// # Safety
    /// `psid` must be null or point to a SID that stays valid for the duration of the call.
    unsafe fn copy_sid(psid: PSID) -> io::Result<Sid> {
        // SAFETY: the caller guarantees `psid` is null or a live SID; IsValidSid checks the
        // structure before GetLengthSid reads its length.
        unsafe {
            if psid.is_null() || IsValidSid(psid) == 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid SID"));
            }
            let n = GetLengthSid(psid) as usize;
            Ok(Sid::from_bytes(std::slice::from_raw_parts(psid.cast::<u8>(), n).to_vec()))
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

    #[derive(Clone, Copy)]
    pub(super) enum Which {
        User,
        DefaultOwner,
    }

    /// The process token's user SID or default owner SID.
    pub(super) fn token_sid(which: Which) -> io::Result<Sid> {
        let class = match which {
            Which::User => TokenUser,
            Which::DefaultOwner => TokenOwner,
        };
        // SAFETY: every pointer passed is either null with a zero length or points into a
        // live, large enough local buffer; the token handle is closed by the guard. Both
        // TOKEN_USER and TOKEN_OWNER start with a PSID that points into the same buffer,
        // which outlives the copy.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let _guard = HandleGuard(token);
            let mut len = 0u32;
            GetTokenInformation(token, class, null_mut(), 0, &mut len);
            if len == 0 || len > MAX_TOKEN_INFO {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "token information size"));
            }
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let cap = (buf.len() * 8) as u32;
            if GetTokenInformation(token, class, buf.as_mut_ptr().cast::<c_void>(), cap, &mut len) == 0 {
                return Err(io::Error::last_os_error());
            }
            let psid: PSID = ptr::read(buf.as_ptr().cast::<PSID>());
            copy_sid(psid)
        }
    }

    pub(super) fn restrict(file: &File) -> io::Result<()> {
        let user = token_sid(Which::User)?;
        let acl_bytes = (ACL_HEADER_BYTES + ACE_FIXED_BYTES + user.bytes.len()).div_ceil(4) * 4;
        let mut acl = vec![0u32; acl_bytes / 4];
        let handle = file.as_raw_handle();
        // SAFETY: `acl` is a 4-byte aligned buffer of `acl_bytes` bytes, enough for the ACL
        // header and one allow ACE with this SID; `user` outlives the calls; the file handle is
        // live for the borrow of `file`. SetSecurityInfo copies the ACL.
        unsafe {
            let pacl = acl.as_mut_ptr().cast::<ACL>();
            if InitializeAcl(pacl, acl_bytes as u32, ACL_REVISION) == 0 {
                return Err(io::Error::last_os_error());
            }
            if AddAccessAllowedAce(pacl, ACL_REVISION, FILE_ALL_ACCESS, user.as_psid()) == 0 {
                return Err(io::Error::last_os_error());
            }
            let rc = SetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                pacl,
                null(),
            );
            if rc != 0 {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
        }
        Ok(())
    }

    pub(super) fn check(file: &File) -> io::Result<Access> {
        let user = token_sid(Which::User)?;
        let default_owner = token_sid(Which::DefaultOwner)?;
        let handle = file.as_raw_handle();
        let mut owner: PSID = null_mut();
        let mut dacl: *mut ACL = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: the out-pointers are live locals; on success `owner` and `dacl` point into
        // the descriptor `sd`, which the guard frees only after the last use below. Each ACE
        // pointer comes from GetAce on that DACL with an index below its AceCount, and is read
        // with unaligned reads of the fixed-size header and allow-ACE prefix that GetAce
        // guarantees are present for those types.
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
            if owner.is_null() {
                return Ok(Access::Wider("the file has no owner"));
            }
            let owner = copy_sid(owner)?;
            if owner.bytes != user.bytes && owner.bytes != default_owner.bytes {
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
                        if usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
                            return Ok(Access::Wider("malformed allow ACE"));
                        }
                        let sid = ace.cast::<u8>().add(ACE_FIXED_BYTES).cast::<c_void>();
                        if copy_sid(sid)?.bytes != user.bytes {
                            return Ok(Access::Wider("the DACL grants access to another principal"));
                        }
                    }
                    _ => return Ok(Access::Wider("the DACL holds an ACE type this check does not accept")),
                }
            }
        }
        Ok(Access::OwnerOnly)
    }
}
