//! Decides which filesystem events can matter for a library's index.
//!
//! The scanner skips dot-files/dot-dirs (including AppleDouble `._*`) and
//! indexes only audio extensions, so events on such paths cannot change the
//! index. Everything else triggers a rescan; in particular directory events
//! and renames do, because a folder rename moves every file below it.
//! Deliberately conservative: when it is unclear (the path is gone and has
//! a non-audio name) the event counts as relevant.

use std::path::{Path, PathBuf};

use notify::event::{EventKind, RemoveKind};

use crate::scan::is_audio_name;

/// What the filesystem says about a path right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Exists and is a directory.
    Dir,
    /// Exists and is not a directory.
    File,
    /// Does not exist (or cannot be stat'ed).
    Gone,
}

/// Probes `path` without following a final symlink.
pub fn probe_fs(path: &Path) -> Probe {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => Probe::Dir,
        Ok(_) => Probe::File,
        Err(_) => Probe::Gone,
    }
}

/// True if an event of `kind` on `paths` may change the index of the
/// library rooted at `root`. `probe` is injected for tests.
pub fn is_relevant(
    kind: &EventKind,
    paths: &[PathBuf],
    root: &Path,
    probe: impl Fn(&Path) -> Probe,
) -> bool {
    if matches!(kind, EventKind::Access(_)) {
        return false;
    }
    if paths.is_empty() {
        return true; // unknown: be safe
    }
    paths.iter().any(|p| path_relevant(kind, p, root, &probe))
}

fn path_relevant(
    kind: &EventKind,
    path: &Path,
    root: &Path,
    probe: &impl Fn(&Path) -> Probe,
) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return true; // outside the root (e.g. a moved root): let the scan decide
    };
    if rel.as_os_str().is_empty() {
        return true; // the root itself
    }
    let hidden = rel
        .components()
        .any(|c| c.as_os_str().to_str().is_some_and(|s| s.starts_with('.')));
    if hidden {
        return false;
    }
    let audio = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(is_audio_name);
    if audio {
        return true;
    }
    // Non-audio name: only a directory (or something unknown) can matter.
    match probe(path) {
        Probe::Dir => true,
        Probe::File => false,
        Probe::Gone => !matches!(kind, EventKind::Remove(RemoveKind::File)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, ModifyKind, RenameMode};

    fn rel(kind: EventKind, path: &str, probe: Probe) -> bool {
        is_relevant(&kind, &[PathBuf::from(path)], Path::new("/lib"), |_| probe)
    }

    const CREATE: EventKind = EventKind::Create(CreateKind::Any);

    #[test]
    fn audio_files_trigger() {
        assert!(rel(CREATE, "/lib/a/kick.WAV", Probe::File));
        assert!(rel(
            EventKind::Remove(RemoveKind::File),
            "/lib/kick.flac",
            Probe::Gone
        ));
    }

    #[test]
    fn hidden_and_appledouble_are_ignored() {
        assert!(!rel(CREATE, "/lib/.DS_Store", Probe::File));
        assert!(!rel(CREATE, "/lib/a/._kick.wav", Probe::File));
        assert!(!rel(CREATE, "/lib/.hidden/kick.wav", Probe::File));
    }

    #[test]
    fn non_audio_files_are_ignored_but_directories_trigger() {
        assert!(!rel(CREATE, "/lib/notes.txt", Probe::File));
        assert!(rel(CREATE, "/lib/Drums.v2", Probe::Dir));
        assert!(rel(
            EventKind::Modify(ModifyKind::Name(RenameMode::From)),
            "/lib/Old Folder",
            Probe::Gone
        ));
    }

    #[test]
    fn removed_non_audio_file_is_ignored_unknown_removal_triggers() {
        assert!(!rel(
            EventKind::Remove(RemoveKind::File),
            "/lib/notes.txt",
            Probe::Gone
        ));
        assert!(rel(
            EventKind::Remove(RemoveKind::Folder),
            "/lib/Drums",
            Probe::Gone
        ));
        assert!(rel(
            EventKind::Remove(RemoveKind::Any),
            "/lib/Drums",
            Probe::Gone
        ));
    }

    #[test]
    fn root_outside_paths_and_empty_paths_trigger() {
        assert!(rel(CREATE, "/lib", Probe::Dir));
        assert!(rel(EventKind::Other, "/elsewhere/x", Probe::Gone));
        assert!(is_relevant(&EventKind::Any, &[], Path::new("/lib"), |_| {
            Probe::Gone
        }));
    }

    #[test]
    fn access_never_triggers_and_any_relevant_path_suffices() {
        assert!(!rel(
            EventKind::Access(AccessKind::Any),
            "/lib/kick.wav",
            Probe::File
        ));
        let paths = vec![PathBuf::from("/lib/.x"), PathBuf::from("/lib/kick.wav")];
        let kind = EventKind::Modify(ModifyKind::Name(RenameMode::Both));
        assert!(is_relevant(&kind, &paths, Path::new("/lib"), |_| {
            Probe::File
        }));
    }
}
