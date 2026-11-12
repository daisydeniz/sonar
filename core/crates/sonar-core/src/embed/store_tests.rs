//! Tests of the embedding store (`mod tests` of `store`).

use super::*;

fn emb(seed: f32) -> Embedding {
    let mut e = [0.0f32; EMBED_DIM];
    for (i, v) in e.iter_mut().enumerate() {
        *v = seed + i as f32 * 1e-3;
    }
    e
}

#[test]
fn append_survives_reopen_and_reads_back() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = tmp.path().join("clap");
    let (mut w, state) = EmbedStore::open(&dir).expect("open");
    assert_eq!(state, OpenState::New);
    let (a, b, c) = (emb(1.0), emb(2.0), emb(3.0));
    w.append(&[(7, &a), (9, &b)]).expect("append");
    drop(w);
    let (mut w, state) = EmbedStore::open(&dir).expect("reopen");
    assert_eq!((state, w.rows(), w.live_rows()), (OpenState::Ready, 2, 2));
    w.append(&[(11, &c)]).expect("append");
    drop(w);

    let r = EmbedStore::open_read(&dir).expect("read");
    assert_eq!((r.rows(), r.live_rows()), (3, 3));
    assert_eq!(r.get(9), Some(&b));
    assert_eq!(r.get(11), Some(&c));
    assert_eq!(r.get(8), None);
    let all: Vec<(i64, Embedding)> = r.iter().map(|(id, e)| (id, *e)).collect();
    assert_eq!(all, vec![(7, a), (9, b), (11, c)]);
    assert!(r.matrix().is_some_and(|m| m.len() == 3 * EMBED_DIM));
}

#[test]
fn re_appending_a_file_makes_the_new_row_win() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = tmp.path().join("clap");
    let (mut w, _) = EmbedStore::open(&dir).expect("open");
    let (old, new) = (emb(1.0), emb(5.0));
    w.append(&[(3, &old)]).expect("append");
    w.append(&[(3, &new)]).expect("append again");
    let s = w.stats();
    assert_eq!((s.rows, s.live, s.dead), (2, 1, 1));
    drop(w);
    let r = EmbedStore::open_read(&dir).expect("read");
    assert_eq!(r.get(3), Some(&new));
    assert_eq!(r.iter().count(), 1);
}

#[test]
fn a_torn_append_is_cut_off() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = tmp.path().join("clap");
    let (mut w, _) = EmbedStore::open(&dir).expect("open");
    w.append(&[(1, &emb(1.0)), (2, &emb(2.0))]).expect("append");
    drop(w);
    // Embedding rows written, ids not (crash between the two writes),
    // plus a half row of garbage in the ids file.
    let mut f = OpenOptions::new()
        .append(true)
        .open(dir.join(ROWS_FILE))
        .expect("rows");
    f.write_all(&vec![0xAB; ROW_BYTES as usize]).expect("write");
    let mut f = OpenOptions::new()
        .append(true)
        .open(dir.join(IDS_FILE))
        .expect("ids");
    f.write_all(&[1, 2, 3]).expect("write");
    assert_eq!(EmbedStore::open_read(&dir).expect("read").rows(), 2);
    let (mut w, state) = EmbedStore::open(&dir).expect("reopen");
    assert_eq!((state, w.rows()), (OpenState::Ready, 2));
    w.append(&[(3, &emb(3.0))]).expect("append");
    drop(w);
    let r = EmbedStore::open_read(&dir).expect("read");
    assert_eq!(r.rows(), 3);
    assert_eq!(r.get(3), Some(&emb(3.0)));
    assert_eq!(r.get(2), Some(&emb(2.0)));
}

#[test]
fn one_writer_at_a_time_and_readers_cannot_write() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = tmp.path().join("clap");
    let (w, _) = EmbedStore::open(&dir).expect("open");
    assert!(matches!(
        EmbedStore::open(&dir),
        Err(Error::VectorStore { .. })
    ));
    drop(w);
    let (mut w, _) = EmbedStore::open(&dir).expect("open");
    w.append(&[(1, &emb(1.0))]).expect("append");
    drop(w);
    let mut r = EmbedStore::open_read(&dir).expect("read");
    assert!(r.append(&[(2, &emb(2.0))]).is_err());
}

#[test]
fn other_format_is_reset_by_writers_and_refused_by_readers() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = tmp.path().join("clap");
    assert!(EmbedStore::open_read(&dir).is_err(), "no store yet");
    let (mut w, _) = EmbedStore::open(&dir).expect("open");
    w.append(&[(1, &emb(1.0))]).expect("append");
    drop(w);
    fs::write(dir.join(META_FILE), "{\"format\":99,\"dim\":512}").expect("meta");
    assert!(EmbedStore::open_read(&dir).is_err());
    let (w, state) = EmbedStore::open(&dir).expect("reset");
    assert_eq!((state, w.rows()), (OpenState::Reset, 0));
}

#[test]
fn bad_ids_are_rejected() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (mut w, _) = EmbedStore::open(tmp.path().join("clap")).expect("open");
    assert!(w.append(&[(0, &emb(1.0))]).is_err());
    assert!(w.append(&[(-4, &emb(1.0))]).is_err());
    assert_eq!(w.rows(), 0);
}
