//! Tests of display rows, waveform peaks and preview PCM through the engine.

use super::*;
use crate::fixtures::wav::{encode, SampleFormat, WavSpec};

#[test]
fn rows_peaks_and_preview_for_a_scanned_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let lib = tmp.path().join("lib");
    std::fs::create_dir(&lib).expect("mkdir");
    let tone: Vec<f32> = (0..800).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect();
    let bytes = encode(&WavSpec::new(16_000, 1, SampleFormat::Int16), &tone).bytes;
    std::fs::write(lib.join("kick_one.wav"), bytes).expect("write");

    let engine = Engine::open(tmp.path().join("home")).expect("open");
    engine.add_library(&lib).expect("add");
    let path = lib.join("kick_one.wav").canonicalize().expect("canon");
    let id = engine
        .file_id_by_path(&path)
        .expect("lookup")
        .expect("known");

    let row = engine.row(id).expect("row").expect("exists");
    assert_eq!((row.file_id, row.name.as_str()), (id, "kick_one"));
    assert_eq!(row.path, path);
    assert!(!row.favorite && row.tags.is_empty());
    assert!(engine.row(id + 99).expect("row").is_none());
    assert_eq!(engine.rows(&[id + 99, id]).expect("rows").len(), 1);

    let peaks = engine.waveform_peaks(id, 50).expect("peaks");
    assert_eq!(peaks.len(), 50);
    assert!(peaks.iter().all(|&(lo, hi)| lo <= hi && hi.abs() <= 1.0));
    assert!(peaks.iter().any(|&(_, hi)| hi > 0.2));
    assert!(matches!(
        engine.waveform_peaks(id, 0),
        Err(Error::InvalidInput { .. })
    ));
    assert!(matches!(
        engine.waveform_peaks(id + 99, 10),
        Err(Error::FileNotFound { .. })
    ));

    let pcm = engine.preview_pcm(id).expect("pcm");
    assert_eq!((pcm.sample_rate, pcm.channels), (16_000, 1));
    assert_eq!(pcm.samples.len(), 800);
    assert!(!pcm.truncated);
}
