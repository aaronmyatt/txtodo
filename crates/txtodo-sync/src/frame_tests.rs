//! Frame: the frozen header round-trips, a foreign stream fails on magic, an unknown version is a
//! typed error that consumes nothing, and a hostile length is refused before any allocation.

use crate::frame::{Frame, FrameError, HEADER_BYTES, MAGIC, MAX_FRAME_BYTES, PROTOCOL_VERSION};

#[test]
fn header_layout_is_magic_version_le_len_le_then_body() {
    let frame = Frame::new(vec![0xAA, 0xBB, 0xCC]).unwrap();
    let bytes = frame.encode().unwrap();
    // Frozen forever: any change here is a wire break, not a refactor.
    assert_eq!(
        bytes,
        [b'T', b'X', b'T', b'O', 1, 0, 3, 0, 0, 0, 0xAA, 0xBB, 0xCC]
    );
    assert_eq!(bytes.len(), HEADER_BYTES + 3);
    let (back, used) = Frame::decode(&bytes).unwrap();
    assert_eq!(back, frame);
    assert_eq!(used, bytes.len());
}

#[test]
fn decode_takes_one_frame_and_reports_how_much_it_used() {
    let a = Frame::new(b"first".to_vec()).unwrap().encode().unwrap();
    let b = Frame::new(Vec::new()).unwrap().encode().unwrap();
    let mut stream = a.clone();
    stream.extend_from_slice(&b);
    let (first, used) = Frame::decode(&stream).unwrap();
    assert_eq!(first.body, b"first");
    assert_eq!(used, a.len());
    let (second, used2) = Frame::decode(&stream[used..]).unwrap();
    assert!(second.body.is_empty(), "an empty body is a legal frame");
    assert_eq!(used + used2, stream.len());
}

#[test]
fn a_v1_decoder_reading_a_v2_frame_returns_unknown_version_and_consumes_nothing() {
    let mut v2 = Frame::new(vec![1, 2, 3]).unwrap();
    v2.version = PROTOCOL_VERSION + 1;
    let bytes = v2.encode().unwrap();
    assert_eq!(
        Frame::decode(&bytes),
        Err(FrameError::UnknownVersion {
            got: 2,
            supported: 1
        })
    );
    // The header still parses, so a caller can skip exactly this frame and carry on.
    assert_eq!(Frame::peek(&bytes), Ok((2, bytes.len())));
}

#[test]
fn foreign_or_truncated_streams_fail_on_magic_or_length_never_on_the_body() {
    assert_eq!(
        Frame::decode(b"HTTP/1.1 200 OK"),
        Err(FrameError::BadMagic { got: *b"HTTP" })
    );
    assert_eq!(
        Frame::decode(b"TX"),
        Err(FrameError::Truncated {
            needed: HEADER_BYTES,
            got: 2
        })
    );
    let mut short = Frame::new(vec![9; 8]).unwrap().encode().unwrap();
    short.truncate(HEADER_BYTES + 5);
    assert_eq!(
        Frame::decode(&short),
        Err(FrameError::Truncated {
            needed: HEADER_BYTES + 8,
            got: HEADER_BYTES + 5
        })
    );
}

#[test]
fn a_hostile_length_prefix_is_rejected_before_allocating() {
    let mut hostile = Vec::new();
    hostile.extend_from_slice(&MAGIC);
    hostile.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    hostile.extend_from_slice(&u32::MAX.to_le_bytes());
    hostile.extend_from_slice(&[0; 16]);
    let err = Frame::decode(&hostile).unwrap_err();
    assert_eq!(
        err,
        FrameError::TooLarge {
            len: u32::MAX as usize,
            max: MAX_FRAME_BYTES
        }
    );
    // TooLarge, not Truncated: the cap is checked before the available-bytes check, so the length
    // never reaches `to_vec`.
    assert_eq!(Frame::peek(&hostile).unwrap_err(), err);
    let over = vec![0; MAX_FRAME_BYTES + 1];
    assert!(matches!(Frame::new(over), Err(FrameError::TooLarge { .. })));
}

#[test]
fn errors_name_what_was_seen_and_what_was_expected() {
    let text = FrameError::UnknownVersion {
        got: 7,
        supported: 1,
    }
    .to_string();
    assert!(text.contains('7') && text.contains('1'), "{text}");
    let text = FrameError::TooLarge { len: 99, max: 10 }.to_string();
    assert!(text.contains("99") && text.contains("10"), "{text}");
}
