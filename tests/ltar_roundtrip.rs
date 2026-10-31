//! Integration tests for packing and unpacking `.ltar` archives.
//!
//! These check that unpacking reproduces the packed files, whether the reader
//! holds the whole archive in memory (payloads are decoded in place) or not
//! (payloads are copied out first), and that a truncated archive is reported
//! as such.

use leangz::ltar::{self, UnpackError};
use std::fs;
use std::io::{BufReader, Cursor, ErrorKind};
use std::path::{Path, PathBuf};

const FILES: [&str; 4] = ["Attr.olean", "Attr.olean.server", "Attr.olean.private", "Attr.ilean"];

/// A build directory holding the test module, with an `.ilean` big enough to
/// span several zstd blocks.
fn build_dir() -> tempfile::TempDir {
  let src = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/module"));
  let dir = tempfile::tempdir().unwrap();
  for f in ["Attr.trace", "Attr.olean", "Attr.olean.server", "Attr.olean.private"] {
    fs::copy(src.join(f), dir.path().join(f)).unwrap();
  }
  let ilean: String =
    (0..40000).map(|i| format!("{{\"decl\":{i},\"pos\":[{},{}]}}", i * 7, i % 13)).collect();
  fs::write(dir.path().join("Attr.ilean"), ilean).unwrap();
  dir
}

fn pack(dir: &Path, include_hash: bool) -> Vec<u8> {
  let mut out = vec![];
  let args = FILES.iter().map(|s| s.to_string());
  ltar::pack(&[dir.to_owned()], &mut out, "Attr.trace", args, include_hash, false).unwrap();
  out
}

fn unpack(
  archive: &[u8], buffered: bool, hash: Option<u64>,
) -> Result<tempfile::TempDir, UnpackError> {
  let dir = tempfile::tempdir().unwrap();
  let base = [PathBuf::from(dir.path())];
  if buffered {
    ltar::unpack(&base, Cursor::new(archive), true, hash, false)?;
  } else {
    // A buffer smaller than any payload, so each one is copied out.
    ltar::unpack(&base, BufReader::with_capacity(16, Cursor::new(archive)), true, hash, false)?;
  }
  Ok(dir)
}

#[test]
fn test_ltar_roundtrip() {
  let src = build_dir();
  let archive = pack(src.path(), true);
  let mut traces = vec![];
  for buffered in [true, false] {
    let out = unpack(&archive, buffered, None).ok().unwrap();
    for f in FILES {
      assert!(
        fs::read(src.path().join(f)).unwrap() == fs::read(out.path().join(f)).unwrap(),
        "{f}"
      );
    }
    traces.push(fs::read(out.path().join("Attr.trace")).unwrap());
  }
  assert_eq!(traces[0], traces[1]);
}

#[test]
fn test_ltar_hash_override() {
  let src = build_dir();
  let archive = pack(src.path(), false);
  assert!(matches!(unpack(&archive, true, None), Err(UnpackError::MissingHash)));
  for buffered in [true, false] {
    let out = unpack(&archive, buffered, Some(0x0123456789abcdef)).ok().unwrap();
    let trace: serde_json::Value =
      serde_json::from_slice(&fs::read(out.path().join("Attr.trace")).unwrap()).unwrap();
    assert_eq!(trace["depHash"], "0123456789abcdef");
  }
}

#[test]
fn test_ltar_truncated() {
  let src = build_dir();
  let archive = pack(src.path(), true);
  for buffered in [true, false] {
    match unpack(&archive[..archive.len() - 100], buffered, None) {
      Err(UnpackError::IOError(e)) => assert_eq!(e.kind(), ErrorKind::UnexpectedEof),
      Err(e) => panic!("{e}"),
      Ok(_) => panic!("unpacked a truncated archive"),
    }
  }
}

#[test]
fn test_ltar_empty_private() {
  let src = build_dir();
  let archive = pack(src.path(), true);
  let dir = tempfile::tempdir().unwrap();
  let base = [PathBuf::from(dir.path())];
  ltar::unpack_with(&base, Cursor::new(&archive[..]), true, None, false, true).ok().unwrap();
  for f in ["Attr.olean", "Attr.olean.server", "Attr.ilean"] {
    assert!(fs::read(src.path().join(f)).unwrap() == fs::read(dir.path().join(f)).unwrap(), "{f}");
  }
  assert!(fs::read(dir.path().join("Attr.olean.private")).unwrap().is_empty());
}
