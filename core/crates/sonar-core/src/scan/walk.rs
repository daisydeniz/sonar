//! Directory walk: lists audio files under a library root with their size
//! and mtime (one `stat` each, no reads), plus the parts of the tree that
//! could not be listed.
//!
//! Rules (see the module docs of [`crate::scan`]): symlinks are never
//! followed, names starting with `.` (dot-files, dot-dirs, AppleDouble
//! `._*`) are skipped, only [`AUDIO_EXTENSIONS`](super::AUDIO_EXTENSIONS)
//! files are kept, non-UTF-8 names are reported and skipped.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{self, DirEntry, ReadDir};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::identity::{is_audio_extension, is_audio_name, mtime_ns};
use super::{IssueKind, ScanIssue};

/// An audio file found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskFile {
    /// Path relative to the library root, `/`-separated.
    pub rel_path: String,
    /// Size in bytes.
    pub size: u64,
    /// Modification time, ns since the unix epoch.
    pub mtime_ns: i64,
}

/// Relative paths whose state on disk is unknown this scan (unlistable
/// directories, entries that could not be stat'ed). Rows there must be
/// left alone, not marked missing.
#[derive(Debug, Default)]
pub(crate) struct Unknown {
    /// The root itself could not be fully listed: everything is unknown.
    whole_library: bool,
    /// Relative directories whose subtree is unknown.
    dirs: HashSet<String>,
    files: HashSet<String>,
}

impl Unknown {
    /// Marks the subtree under relative directory `rel` ("" = root).
    pub(crate) fn add_dir(&mut self, rel: &str) {
        if rel.is_empty() {
            self.whole_library = true;
        } else {
            self.dirs.insert(rel.to_owned());
        }
    }

    /// Marks one relative path (file, or entry of unknown type).
    pub(crate) fn add_file(&mut self, rel: &str) {
        self.files.insert(rel.to_owned());
    }

    /// True if the on-disk state of `rel` is unknown. One hash lookup per
    /// ancestor directory of `rel` (path depth), independent of how many
    /// directories were unreadable.
    pub(crate) fn contains(&self, rel: &str) -> bool {
        if self.whole_library || self.files.contains(rel) {
            return true;
        }
        !self.dirs.is_empty()
            && rel
                .match_indices('/')
                .any(|(i, _)| self.dirs.contains(&rel[..i]))
    }
}

/// Result of walking one library root.
#[derive(Debug, Default)]
pub(crate) struct Walk {
    /// Audio files, sorted by `rel_path`.
    pub files: Vec<DiskFile>,
    /// Paths whose state could not be determined.
    pub unknown: Unknown,
    /// Problems met on the way (never fatal).
    pub issues: Vec<ScanIssue>,
}

/// Why a walk produced no listing.
#[derive(Debug)]
pub(crate) enum WalkError {
    /// The root is missing, not a directory, or cannot be listed.
    Offline(io::Error),
    /// The cancel flag was set.
    Cancelled,
}

/// Walks the tree under `root` (iteratively, so depth is not limited by
/// the stack). Checks `cancel` once per directory entry.
pub(crate) fn walk(root: &Path, cancel: &AtomicBool) -> Result<Walk, WalkError> {
    let meta = fs::metadata(root).map_err(WalkError::Offline)?;
    if !meta.is_dir() {
        let e = io::Error::new(io::ErrorKind::NotADirectory, "not a directory");
        return Err(WalkError::Offline(e));
    }
    let first = fs::read_dir(root).map_err(WalkError::Offline)?;

    let mut out = Walk::default();
    let mut stack: Vec<(PathBuf, String)> = Vec::new();
    visit(root, "", first, cancel, &mut out, &mut stack)?;
    while let Some((dir, rel)) = stack.pop() {
        match fs::read_dir(&dir) {
            Ok(entries) => visit(&dir, &rel, entries, cancel, &mut out, &mut stack)?,
            Err(e) => {
                out.issue(IssueKind::UnreadableDir, dir, &e);
                out.unknown.add_dir(&rel);
            }
        }
    }
    out.files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(out)
}

impl Walk {
    fn issue(&mut self, kind: IssueKind, path: PathBuf, err: &io::Error) {
        self.issues.push(ScanIssue {
            path,
            kind,
            message: err.to_string(),
        });
    }
}

/// Handles the entries of one directory: files are recorded, subdirectories
/// pushed on `stack`.
fn visit(
    dir: &Path,
    rel_dir: &str,
    entries: ReadDir,
    cancel: &AtomicBool,
    out: &mut Walk,
    stack: &mut Vec<(PathBuf, String)>,
) -> Result<(), WalkError> {
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err(WalkError::Cancelled);
        }
        match entry {
            Ok(entry) => visit_entry(&entry, rel_dir, out, stack),
            Err(e) => {
                // The listing is incomplete: we cannot tell what is gone.
                out.issue(IssueKind::UnreadableDir, dir.to_path_buf(), &e);
                out.unknown.add_dir(rel_dir);
                break;
            }
        }
    }
    Ok(())
}

fn visit_entry(
    entry: &DirEntry,
    rel_dir: &str,
    out: &mut Walk,
    stack: &mut Vec<(PathBuf, String)>,
) {
    let name = entry.file_name();
    if name.as_encoded_bytes().starts_with(b".") {
        return; // dot-files, dot-dirs and AppleDouble `._*`
    }
    let path = entry.path();
    // `DirEntry::file_type` does not follow symlinks.
    let file_type = match entry.file_type() {
        Ok(t) => t,
        Err(e) => {
            if let Some(rel) = name.to_str().map(|n| join_rel(rel_dir, n)) {
                out.unknown.add_file(&rel);
                out.unknown.add_dir(&rel);
            }
            out.issue(IssueKind::UnreadableFile, path, &e);
            return;
        }
    };
    if file_type.is_symlink() {
        return;
    }
    let Some(name) = name.to_str() else {
        if file_type.is_dir() || (file_type.is_file() && has_audio_extension(&name)) {
            let e = io::Error::new(io::ErrorKind::InvalidData, "file name is not valid UTF-8");
            out.issue(IssueKind::NonUtf8Name, path, &e);
        }
        return;
    };
    let rel = join_rel(rel_dir, name);
    if file_type.is_dir() {
        stack.push((path, rel));
        return;
    }
    if !file_type.is_file() || !is_audio_name(name) {
        return;
    }
    match entry.metadata().and_then(|m| Ok((m.len(), mtime_ns(&m)?))) {
        Ok((size, mtime_ns)) => out.files.push(DiskFile {
            rel_path: rel,
            size,
            mtime_ns,
        }),
        Err(e) => {
            out.unknown.add_file(&rel);
            out.issue(IssueKind::UnreadableFile, path, &e);
        }
    }
}

fn has_audio_extension(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(is_audio_extension)
}

/// `rel_dir/name`, or `name` at the root.
fn join_rel(rel_dir: &str, name: &str) -> String {
    if rel_dir.is_empty() {
        name.to_owned()
    } else {
        format!("{rel_dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_matches_files_and_subtrees_only() {
        let mut u = Unknown::default();
        u.add_dir("a/b");
        u.add_file("c.wav");
        assert!(u.contains("a/b/x.wav"));
        assert!(u.contains("a/b/deep/x.wav"));
        assert!(u.contains("c.wav"));
        assert!(!u.contains("a/bc/x.wav"), "prefix must end at a separator");
        assert!(!u.contains("a/x.wav"));
        assert!(!u.contains("c.wav2"));
        u.add_dir("");
        assert!(u.contains("anything.wav"));
    }

    #[test]
    fn join_rel_uses_forward_slashes() {
        assert_eq!(join_rel("", "a.wav"), "a.wav");
        assert_eq!(join_rel("x/y", "a.wav"), "x/y/a.wav");
    }

    #[test]
    fn non_utf8_extension_check() {
        use std::os::unix::ffi::OsStrExt;
        assert!(has_audio_extension(OsStr::from_bytes(b"bad\xff.WAV")));
        assert!(!has_audio_extension(OsStr::from_bytes(b"bad\xff.txt")));
    }

    #[test]
    fn missing_or_file_root_is_offline() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let never = AtomicBool::new(false);
        let missing = walk(&tmp.path().join("nope"), &never);
        assert!(matches!(missing, Err(WalkError::Offline(_))));
        let file = tmp.path().join("f");
        fs::write(&file, b"x").expect("write");
        assert!(matches!(walk(&file, &never), Err(WalkError::Offline(_))));
    }

    #[test]
    fn cancel_flag_stops_the_walk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("a.wav"), b"x").expect("write");
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            walk(tmp.path(), &cancel),
            Err(WalkError::Cancelled)
        ));
    }
}
