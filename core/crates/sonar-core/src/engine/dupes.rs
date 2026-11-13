//! [`Engine::duplicates`]: the duplicate finder (ADR-036) over the
//! engine's store and vector store, with its own latest-run-wins cancel
//! flag (a search never cancels it, nor it a search).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{lock, Engine};
use crate::dupes::{find_duplicates, DupesOptions, DupesReport};
use crate::index::vectors::{self, dir_for_db as vector_dir, VectorStore};
use crate::{Error, Result, Store};

impl Engine {
    /// Exact and near duplicate groups of every online, present file
    /// ([`find_duplicates`]). **Blocking**: reads (and the first time
    /// hashes) candidate files; call it off the UI thread. Opens its own
    /// store connection, so it never holds the engine's store lock, and
    /// finds exact groups only while nothing has been analyzed yet.
    /// `opts.cancel` is honoured as given.
    ///
    /// # Errors
    /// As [`find_duplicates`], plus [`Error::VectorStore`] for a damaged
    /// vector store.
    pub fn duplicates(&self, opts: &DupesOptions) -> Result<DupesReport> {
        let store = Store::open(&self.inner.db_path)?;
        let dir = vector_dir(&self.inner.db_path);
        let vs = if vectors::exists(&dir)? {
            Some(VectorStore::open_read(&dir)?)
        } else {
            None
        };
        find_duplicates(&store, vs.as_ref(), opts)
    }

    /// [`Engine::duplicates`] with the engine's duplicate cancel flag
    /// (latest wins): a newer `duplicates_latest` or
    /// [`Engine::cancel_duplicates`] makes this return
    /// [`Error::Cancelled`]. `opts.cancel` is replaced by that flag.
    ///
    /// # Errors
    /// As [`Engine::duplicates`], plus [`Error::Cancelled`].
    pub fn duplicates_latest(&self, opts: &DupesOptions) -> Result<DupesReport> {
        let token = Arc::new(AtomicBool::new(false));
        {
            let mut current = lock(&self.inner.dupes_cancel);
            current.store(true, Ordering::SeqCst);
            *current = Arc::clone(&token);
        }
        let opts = DupesOptions {
            cancel: Some(Arc::clone(&token)),
            ..opts.clone()
        };
        let report = self.duplicates(&opts)?;
        if token.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        Ok(report)
    }

    /// Cancels the newest duplicate run, if it is still running.
    pub fn cancel_duplicates(&self) {
        lock(&self.inner.dupes_cancel).store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::dupes::DupeKind;

    #[test]
    fn finds_exact_groups_before_analysis_and_cancels() {
        let tmp = tempfile::tempdir().expect("tmp");
        let lib = tmp.path().join("lib");
        fs::create_dir(&lib).expect("mkdir");
        for name in ["a.wav", "b.wav"] {
            fs::write(lib.join(name), vec![3u8; 4000]).expect("write");
        }
        let engine = Engine::open(tmp.path().join("home")).expect("open");
        engine.add_library(&lib).expect("add");
        let r = engine
            .duplicates_latest(&DupesOptions::default())
            .expect("dupes");
        assert_eq!(r.groups.len(), 1);
        assert_eq!(r.groups[0].kind, DupeKind::Exact);
        assert_eq!(r.near_files, 0, "no vectors yet");
        let stop = DupesOptions {
            cancel: Some(Arc::new(AtomicBool::new(true))),
            ..DupesOptions::default()
        };
        assert!(matches!(engine.duplicates(&stop), Err(Error::Cancelled)));
        engine.cancel_duplicates();
        assert!(engine.duplicates_latest(&DupesOptions::default()).is_ok());
    }
}
