//! The audio session pool (feature `embed`, ADR-027): N independent
//! single-thread int8 sessions, one per pool thread. ADR-026 measured this
//! at 122 clips/s with 8 and 158 with 10 sessions on an M4 Pro, against 61-76
//! for one session with 10 intra-op threads: a clip is too small to spread
//! over threads, so the parallelism is across clips.
//!
//! Callers (rayon workers of the job pipeline, or any thread) hand a clip's
//! features to [`EmbedPool::embed`] and block until a session has run it.
//! A session that finds more clips queued takes up to `max_batch` of them
//! as one `[B, 1, 1001, 64]` run. The queue is bounded, so callers cannot
//! pile up features faster than the sessions consume them.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;

use ort::session::Session;
use ort::value::Tensor;

use super::session::{build, ort_err};
use super::{normalize, EmbedOptions, Embedding, EMBED_DIM, FEATURE_LEN, FRAMES, N_MELS};
use crate::{quiet_panic, Error, Result};

struct Job {
    features: Vec<f32>,
    reply: SyncSender<Result<Embedding>>,
}

/// N audio sessions on their own threads (see the module docs).
pub struct EmbedPool {
    tx: Option<SyncSender<Job>>,
    threads: Vec<JoinHandle<()>>,
    sessions: usize,
}

impl std::fmt::Debug for EmbedPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbedPool")
            .field("sessions", &self.sessions)
            .finish_non_exhaustive()
    }
}

fn pool_err(reason: impl Into<String>) -> Error {
    Error::Embed {
        reason: reason.into(),
    }
}

impl EmbedPool {
    /// Loads the audio model of `opts` into `opts.resolved_sessions()`
    /// sessions (in parallel) and starts their threads.
    ///
    /// # Errors
    /// [`Error::ModelMissing`] / [`Error::Embed`] if the model cannot be
    /// found or loaded; no thread is left running then.
    pub fn new(opts: &EmbedOptions) -> Result<Self> {
        let path = opts.model_dir().join(super::AUDIO_MODEL);
        Self::with_model(&path, opts)
    }

    /// [`EmbedPool::new`] with an explicit model file.
    ///
    /// # Errors
    /// See [`EmbedPool::new`].
    pub fn with_model(path: &Path, opts: &EmbedOptions) -> Result<Self> {
        let sessions = opts.resolved_sessions().max(1);
        let max_batch = opts.resolved_batch();
        let (tx, rx) = mpsc::sync_channel::<Job>(sessions * 2);
        let rx = std::sync::Arc::new(Mutex::new(rx));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let mut threads = Vec::with_capacity(sessions);
        for i in 0..sessions {
            let (rx, ready, path, ep) = (
                std::sync::Arc::clone(&rx),
                ready_tx.clone(),
                path.to_path_buf(),
                opts.ep,
            );
            let spawned = std::thread::Builder::new()
                .name(format!("sonar-embed-{i}"))
                .spawn(move || match build(&path, ep, 1) {
                    Ok(session) => {
                        let _ = ready.send(Ok(()));
                        drop(ready);
                        worker(session, &rx, max_batch);
                    }
                    Err(e) => {
                        let _ = ready.send(Err(e));
                    }
                });
            match spawned {
                Ok(h) => threads.push(h),
                Err(e) => {
                    drop(tx);
                    join_all(threads);
                    return Err(pool_err(format!("cannot start an embedding thread: {e}")));
                }
            }
        }
        drop(ready_tx);
        let mut failure = None;
        for _ in 0..sessions {
            match ready_rx.recv() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    failure.get_or_insert(e);
                }
                Err(_) => {
                    failure
                        .get_or_insert_with(|| pool_err("an embedding thread died while loading"));
                    break;
                }
            }
        }
        if let Some(e) = failure {
            drop(tx);
            join_all(threads);
            return Err(e);
        }
        Ok(Self {
            tx: Some(tx),
            threads,
            sessions,
        })
    }

    /// Number of sessions.
    pub fn sessions(&self) -> usize {
        self.sessions
    }

    /// Embeds one clip's [`FEATURE_LEN`] features; blocks until a session
    /// has run it. Callable from any number of threads at once.
    ///
    /// # Errors
    /// [`Error::Embed`] for features of the wrong length, a failing model
    /// run or a stopped pool.
    pub fn embed(&self, features: Vec<f32>) -> Result<Embedding> {
        if features.len() != FEATURE_LEN {
            return Err(pool_err(format!(
                "expected {FEATURE_LEN} feature values, got {}",
                features.len()
            )));
        }
        let (reply, answer) = mpsc::sync_channel(1);
        let tx = self.tx.as_ref().ok_or_else(|| pool_err("pool stopped"))?;
        tx.send(Job { features, reply })
            .map_err(|_| pool_err("embedding pool stopped"))?;
        answer
            .recv()
            .map_err(|_| pool_err("an embedding session stopped"))?
    }
}

impl Drop for EmbedPool {
    fn drop(&mut self) {
        self.tx = None;
        join_all(std::mem::take(&mut self.threads));
    }
}

fn join_all(threads: Vec<JoinHandle<()>>) {
    for t in threads {
        let _ = t.join();
    }
}

/// One session's loop: take a job (plus whatever else is queued, up to
/// `max_batch`), run it, answer each caller.
fn worker(mut session: Session, rx: &Mutex<Receiver<Job>>, max_batch: usize) {
    loop {
        let mut jobs = Vec::with_capacity(max_batch);
        {
            let queue = rx.lock().unwrap_or_else(PoisonError::into_inner);
            let Ok(first) = queue.recv() else { return };
            jobs.push(first);
            while jobs.len() < max_batch {
                match queue.try_recv() {
                    Ok(j) => jobs.push(j),
                    Err(_) => break,
                }
            }
        }
        let outcome = quiet_panic::guarded(|| run_batch(&mut session, &jobs))
            .unwrap_or_else(|msg| Err(pool_err(format!("model run panicked: {msg}"))));
        match outcome {
            Ok(embeddings) => {
                for (job, e) in jobs.into_iter().zip(embeddings) {
                    let _ = job.reply.send(Ok(e));
                }
            }
            Err(e) => {
                let reason = e.to_string();
                for job in jobs {
                    let _ = job.reply.send(Err(pool_err(reason.clone())));
                }
            }
        }
    }
}

/// Runs `jobs` as one batch and returns one normalised embedding per job.
fn run_batch(session: &mut Session, jobs: &[Job]) -> Result<Vec<Embedding>> {
    let b = jobs.len();
    let data: Box<[f32]> = match jobs {
        [one] => one.features.clone().into_boxed_slice(),
        many => {
            let mut v = Vec::with_capacity(b * FEATURE_LEN);
            for j in many {
                v.extend_from_slice(&j.features);
            }
            v.into_boxed_slice()
        }
    };
    let input = Tensor::from_array(([b, 1, FRAMES, N_MELS], data))
        .map_err(|e| ort_err("building the input tensor", e))?;
    let outputs = session
        .run(ort::inputs!["input_features" => input])
        .map_err(|e| ort_err("running the audio model", e))?;
    let (_, raw) = outputs
        .get("audio_embeds")
        .ok_or_else(|| pool_err("audio model has no `audio_embeds` output"))?
        .try_extract_tensor::<f32>()
        .map_err(|e| ort_err("reading the audio embeddings", e))?;
    if raw.len() != b * EMBED_DIM {
        return Err(pool_err(format!(
            "audio model returned {} values for {b} clips",
            raw.len()
        )));
    }
    raw.as_chunks::<EMBED_DIM>()
        .0
        .iter()
        .map(|c| normalize(c))
        .collect()
}
