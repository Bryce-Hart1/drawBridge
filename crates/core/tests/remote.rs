// Claude — Date 11/03/2026
// The swiftSearch client against a real swiftSearch (see common/mod.rs for
// where the binary comes from). Each test gets its own server and store.

mod common;

use bytes::Bytes;
use common::{Server, bytes};
use drawbridge_core::{ByteRange, Conditional, Error, Key, Kind, Remote};

fn key(s: &str) -> Key {
    Key::new(s).unwrap()
}

#[tokio::test]
async fn health_needs_no_key() {
    let s = Server::start();
    s.writer().health().await.unwrap();
    Remote::new(&s.base, "ssk_not_a_real_key").unwrap().health().await.unwrap();
}

#[tokio::test]
async fn round_trips_exact_bytes() {
    let s = Server::start();
    let r = s.writer();
    // 0 B checks an empty PUT still carries Content-Length (411 otherwise).
    for (i, len) in [0usize, 1, 4096, 1 << 20, (8 << 20) + 1].into_iter().enumerate() {
        let k = key(&format!("t/{i}"));
        let data = bytes(len, i as u64 + 1);
        let stored = r.put(&k, Kind::Bin, Bytes::from(data.clone())).await.unwrap();
        assert!(stored.created);
        assert_eq!(stored.size, len as u64);

        let v = r.get(&k).await.unwrap().expect("stored value");
        assert!(v.bytes == data, "{len}-byte value came back different");
        assert_eq!(v.meta.size, len as u64);
        assert_eq!(v.meta.hash, stored.hash);
        assert_eq!(v.meta.etag, format!("\"{}\"", stored.hash));
    }
}

#[tokio::test]
async fn replace_reports_not_created() {
    let s = Server::start();
    let r = s.writer();
    let k = key("notes/a.md");
    assert!(r.put(&k, Kind::Str, Bytes::from_static(b"one")).await.unwrap().created);
    let second = r.put(&k, Kind::Str, Bytes::from_static(b"two")).await.unwrap();
    assert!(!second.created);
    assert_eq!(&r.get(&k).await.unwrap().unwrap().bytes[..], b"two");
}

#[tokio::test]
async fn head_reports_size_kind_and_tag() {
    let s = Server::start();
    let r = s.writer();
    let k = key("photos/x.jpg");
    let stored = r.put(&k, Kind::Img, Bytes::from(bytes(1234, 7))).await.unwrap();

    let m = r.head(&k).await.unwrap().expect("meta");
    assert_eq!(m.size, 1234);
    assert_eq!(m.kind, Kind::Img);
    assert_eq!(m.hash, stored.hash);
    assert_eq!(m.content_type, "image/jpeg");

    assert!(r.head(&key("photos/missing.jpg")).await.unwrap().is_none());
    assert!(r.get(&key("photos/missing.jpg")).await.unwrap().is_none());
}

#[tokio::test]
async fn ranges() {
    let s = Server::start();
    let r = s.writer();
    let k = key("v/0");
    let data = bytes(1000, 3);
    r.put(&k, Kind::Video, Bytes::from(data.clone())).await.unwrap();

    let p = r.get_range(&k, ByteRange::Span(10, 19)).await.unwrap().unwrap();
    assert_eq!((p.offset, p.total), (10, 1000));
    assert!(p.bytes == data[10..20]);

    let p = r.get_range(&k, ByteRange::From(990)).await.unwrap().unwrap();
    assert_eq!(p.offset, 990);
    assert!(p.bytes == data[990..]);

    let p = r.get_range(&k, ByteRange::Last(5)).await.unwrap().unwrap();
    assert_eq!(p.offset, 995);
    assert!(p.bytes == data[995..]);

    // A span running past the end is cut to the end, not refused.
    let p = r.get_range(&k, ByteRange::Span(998, 5000)).await.unwrap().unwrap();
    assert!(p.bytes == data[998..]);

    match r.get_range(&k, ByteRange::From(1000)).await {
        Err(Error::RangeNotSatisfiable { size }) => assert_eq!(size, 1000),
        other => panic!("expected 416, got {other:?}"),
    }

    let empty = key("v/empty");
    r.put(&empty, Kind::Bin, Bytes::new()).await.unwrap();
    assert!(matches!(
        r.get_range(&empty, ByteRange::From(0)).await,
        Err(Error::RangeNotSatisfiable { size: 0 })
    ));

    assert!(r.get_range(&key("v/missing"), ByteRange::From(0)).await.unwrap().is_none());
}

#[tokio::test]
async fn conditional_reads() {
    let s = Server::start();
    let r = s.writer();
    let k = key("dbx/v1/dev/d1/head.json");
    r.put(&k, Kind::Str, Bytes::from_static(br#"{"seq":1}"#)).await.unwrap();
    let tag = r.head(&k).await.unwrap().unwrap().etag;

    assert!(matches!(r.get_if_changed(&k, &tag).await.unwrap(), Conditional::Unchanged));

    r.put(&k, Kind::Str, Bytes::from_static(br#"{"seq":2}"#)).await.unwrap();
    match r.get_if_changed(&k, &tag).await.unwrap() {
        Conditional::Changed(v) => {
            assert_eq!(&v.bytes[..], br#"{"seq":2}"#);
            assert_ne!(v.meta.etag, tag);
        }
        other => panic!("expected Changed, got {other:?}"),
    }

    assert!(matches!(
        r.get_if_changed(&key("dbx/v1/dev/d2/head.json"), &tag).await.unwrap(),
        Conditional::Missing
    ));
}

#[tokio::test]
async fn delete() {
    let s = Server::start();
    let r = s.writer();
    let k = key("tmp/x");
    r.put(&k, Kind::Bin, Bytes::from_static(b"x")).await.unwrap();
    assert!(r.delete(&k).await.unwrap());
    assert!(!r.delete(&k).await.unwrap());
    assert!(r.get(&k).await.unwrap().is_none());
}

#[tokio::test]
async fn access_levels() {
    let s = Server::start();
    let k = key("a/b");
    s.writer().put(&k, Kind::Bin, Bytes::from_static(b"hi")).await.unwrap();

    let reader = s.reader();
    assert_eq!(&reader.get(&k).await.unwrap().unwrap().bytes[..], b"hi");
    assert!(matches!(reader.put(&k, Kind::Bin, Bytes::from_static(b"no")).await, Err(Error::Forbidden)));
    assert!(matches!(reader.delete(&k).await, Err(Error::Forbidden)));

    let stranger = Remote::new(
        &s.base,
        "ssk_00000000_0000000000000000000000000000000000000000000000000000000000000000",
    )
    .unwrap();
    assert!(matches!(stranger.get(&k).await, Err(Error::Unauthorized)));
    assert!(matches!(stranger.head(&k).await, Err(Error::Unauthorized)));
    // A missing key looks the same as a present one without a valid key.
    assert!(matches!(stranger.get(&key("a/missing")).await, Err(Error::Unauthorized)));
    assert!(matches!(
        stranger.put(&k, Kind::Bin, Bytes::from_static(b"no")).await,
        Err(Error::Unauthorized)
    ));
}

// castle+ refuses a request with two Content-Length headers (400), so check the
// raw request head: exactly one, for an empty body and a full one, and never
// a Transfer-Encoding (501 from both castle+ and swiftSearch).
#[tokio::test]
async fn put_frames_with_one_content_length() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let capture = std::thread::spawn(move || {
        let mut heads = Vec::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                head.push(line.trim_end().to_ascii_lowercase());
            }
            let mut stream = stream;
            stream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            heads.push(head);
        }
        heads
    });

    let r = Remote::new(&base, "ssk_x").unwrap();
    let _ = r.put(&key("a"), Kind::Bin, Bytes::new()).await;
    let _ = r.put(&key("b"), Kind::Bin, Bytes::from(bytes(70_000, 1))).await;

    for (head, want) in capture.join().unwrap().iter().zip(["0", "70000"]) {
        let lengths: Vec<_> = head.iter().filter(|l| l.starts_with("content-length:")).collect();
        assert_eq!(lengths, [&format!("content-length: {want}")], "{head:#?}");
        assert!(!head.iter().any(|l| l.starts_with("transfer-encoding:")), "{head:#?}");
        assert!(head.contains(&"x-swiftsearch-type: bin".to_owned()), "{head:#?}");
    }
}

#[tokio::test]
async fn too_large() {
    let s = Server::start();
    let r = s.writer();
    let k = key("big");
    let result = r.put(&k, Kind::Bin, Bytes::from(vec![0u8; (64 << 20) + 1])).await;
    assert!(matches!(result, Err(Error::TooLarge)), "got {result:?}");
}
