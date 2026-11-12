//! Model-gated tests (feature `embed`): they print a note and pass when the
//! CLAP files are not available (`scripts/fetch-models.sh`).

use std::sync::Arc;

use super::*;

fn opts() -> Option<EmbedOptions> {
    let o = EmbedOptions {
        sessions: 2,
        max_batch: 4,
        ..EmbedOptions::default()
    };
    match o.unavailable_reason() {
        None if o.model_dir().join(TEXT_MODEL).is_file() => Some(o),
        None => {
            eprintln!("skipped: text model not found");
            None
        }
        Some(why) => {
            eprintln!("skipped: {why}");
            None
        }
    }
}

fn dot(a: &Embedding, b: &Embedding) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn tone(secs: f32) -> Vec<f32> {
    (0..(48_000.0 * secs) as usize)
        .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin())
        .collect()
}

fn noise(secs: f32) -> Vec<f32> {
    let mut s = 12345u32;
    (0..(48_000.0 * secs) as usize)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect()
}

#[test]
fn pool_embeddings_are_unit_length_deterministic_and_batch_independent() {
    let Some(o) = opts() else { return };
    let pool = Arc::new(EmbedPool::new(&o).expect("pool"));
    assert_eq!(pool.sessions(), 2);
    let mut front = FrontEnd::new();
    let feats = front.features(&tone(3.0)).expect("features");

    let alone = pool.embed(feats.clone()).expect("embed");
    let norm: f32 = alone.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-4);
    assert_eq!(pool.embed(feats.clone()).expect("again"), alone);

    // 16 concurrent callers: sessions batch what is queued (max 4).
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let (pool, feats) = (Arc::clone(&pool), feats.clone());
            std::thread::spawn(move || pool.embed(feats))
        })
        .collect();
    for h in handles {
        let e = h.join().expect("thread").expect("embed");
        assert!(
            dot(&e, &alone) > 0.9999,
            "batched run differs: {}",
            dot(&e, &alone)
        );
    }
    assert!(pool.embed(vec![0.0; 10]).is_err(), "wrong feature length");
}

#[test]
fn text_and_audio_share_one_space() {
    let Some(o) = opts() else { return };
    let pool = EmbedPool::new(&o).expect("pool");
    let mut text = TextEncoder::new(&o).expect("text");
    let mut front = FrontEnd::new();
    let a_noise = pool
        .embed(front.features(&noise(3.0)).expect("f"))
        .expect("e");
    let a_tone = pool
        .embed(front.features(&tone(3.0)).expect("f"))
        .expect("e");
    let t_noise = text.embed("white noise").expect("text");
    let t_tone = text.embed("a sine tone").expect("text");
    let norm: f32 = t_noise.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-4);
    assert!(dot(&a_noise, &t_noise) > dot(&a_noise, &t_tone));
    assert!(dot(&a_tone, &t_tone) > dot(&a_tone, &t_noise));
    assert!(text.embed("   ").is_err());
    assert_eq!(
        text.token_ids("a kick drum").expect("ids"),
        [0, 102, 3151, 12638, 2]
    );
}

#[test]
fn missing_and_corrupt_models_are_typed_errors() {
    let tmp = tempfile::tempdir().expect("tmp");
    let missing = EmbedOptions {
        models_dir: Some(tmp.path().to_path_buf()),
        sessions: 1,
        ..EmbedOptions::default()
    };
    assert!(matches!(
        EmbedPool::new(&missing),
        Err(crate::Error::ModelMissing { .. })
    ));
    assert!(matches!(
        TextEncoder::new(&missing),
        Err(crate::Error::ModelMissing { .. })
    ));
    let dir = missing.model_dir();
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join(AUDIO_MODEL), b"this is not an onnx model").expect("write");
    assert!(matches!(
        EmbedPool::new(&missing),
        Err(crate::Error::Embed { .. })
    ));
}
