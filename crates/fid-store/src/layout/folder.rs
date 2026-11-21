//! Projects opened from verified folders (spec node `CORE.STO.LAYOUT` REQ-011).
//!
//! `CORE.BUS.POL.FS` resolves a folder target (`resolve_folder`) and hands the store a folder it
//! has checked: the canonical absolute path it re-derived after its own open, and the folder's
//! identity at that moment. The store opens that path itself and refuses the folder
//! (`outside_root`) if what it opened is not the same folder, so a folder swapped for another
//! (or for a link to one) between the bus check and the store open is never used.
//! `fid-bus` depends on `fid-store`, so the traits live here and `fid-bus` implements them.

use super::{absolute_checked, sys, Locked, OpenMode, ProjectLocation, StoreError};
use std::fs::File;
use std::io;
use std::path::Path;

/// A folder's identity: device and inode on Unix, volume serial number and file index on
/// Windows. Stable across a rename on the same volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FolderId {
    volume: u64,
    index: u64,
}

impl FolderId {
    /// The identity of an open file or directory handle.
    pub fn of(handle: &File) -> io::Result<FolderId> {
        file_id(handle)
    }

    /// The identity of the folder at `path`, following links on the way (the caller checks
    /// the result against a recorded identity).
    pub fn of_path(path: &Path) -> io::Result<FolderId> {
        sys::Dir::open(&absolute_checked(path).map_err(io::Error::other)?, false)?.identity()
    }

    pub(crate) fn from_parts(volume: u64, index: u64) -> FolderId {
        FolderId { volume, index }
    }
}

#[cfg(unix)]
fn file_id(f: &File) -> io::Result<FolderId> {
    sys::stat_id(f)
}

#[cfg(windows)]
fn file_id(f: &File) -> io::Result<FolderId> {
    sys::file_id(f)
}

#[cfg(not(any(unix, windows)))]
fn file_id(_f: &File) -> io::Result<FolderId> {
    Err(io::Error::other("folder identities are not supported on this platform"))
}

/// A folder `CORE.BUS.POL.FS` resolved and opened: an existing project folder
/// (`FolderTarget::open_project`) or a folder it has just created (`FolderTarget::create_new`).
pub trait VerifiedFolder {
    /// The canonical absolute path re-derived after the bus opened the folder. Never shown
    /// to a caller.
    fn folder_path(&self) -> &Path;
    /// The identity the bus recorded when it opened the folder.
    fn folder_id(&self) -> FolderId;
    /// Resolved in an import root: the store opens it with [`OpenMode::Read`] only and writes
    /// nothing (`CORE.BUS.POL.FS` REQ-011).
    fn read_only(&self) -> bool;
}

/// Where a copy goes (`FolderTarget::into_copy_target`): an existing parent folder and a
/// name that was absent when checked. The store creates the target only by renaming its
/// temporary folder there, without replacing anything (REQ-012).
pub trait CopyTarget {
    /// The canonical absolute path of the parent folder.
    fn parent_path(&self) -> &Path;
    /// The parent folder's identity when the bus checked it.
    fn parent_id(&self) -> FolderId;
    /// The target's name in the parent folder: one plain segment.
    fn target_name(&self) -> &str;
}

impl ProjectLocation {
    /// [`ProjectLocation::create`] in a folder the bus just created: it must still be that
    /// folder (`outside_root` otherwise) and empty.
    pub fn create_at(folder: &dyn VerifiedFolder) -> Result<(ProjectLocation, Locked), StoreError> {
        if folder.read_only() {
            return Err(StoreError::ReadOnlyTarget);
        }
        let root = absolute_checked(folder.folder_path())?;
        let dir = sys::Dir::open(&root, false)?;
        if dir.identity()? != folder.folder_id() {
            return Err(StoreError::FolderSwapped);
        }
        ProjectLocation::create_in(root, dir)
    }

    /// [`ProjectLocation::open`] of a folder the bus resolved: it must still be that folder
    /// (`outside_root` otherwise). A read-only folder opens with [`OpenMode::Read`] only.
    pub fn open_at(
        folder: &dyn VerifiedFolder,
        mode: OpenMode,
    ) -> Result<(ProjectLocation, Option<Locked>), StoreError> {
        if folder.read_only() && mode == OpenMode::Write {
            return Err(StoreError::ReadOnlyTarget);
        }
        let (loc, lock) = ProjectLocation::open(folder.folder_path(), mode)?;
        if loc.dir.identity()? != folder.folder_id() {
            return Err(StoreError::FolderSwapped);
        }
        Ok((loc, lock))
    }

    /// The identity of the folder this location holds open.
    pub fn folder_id(&self) -> Result<FolderId, StoreError> {
        Ok(self.dir.identity()?)
    }

    /// Checks that the held folder and the folder now at [`ProjectLocation::root`] are both the
    /// folder with identity `want` (REQ-011's second check, after the database is opened).
    pub fn check_identity(&self, want: FolderId) -> Result<(), StoreError> {
        let held = self.dir.identity()?;
        let now = FolderId::of_path(&self.root).map_err(|_| StoreError::FolderSwapped)?;
        if held == want && now == want {
            Ok(())
        } else {
            Err(StoreError::FolderSwapped)
        }
    }
}
