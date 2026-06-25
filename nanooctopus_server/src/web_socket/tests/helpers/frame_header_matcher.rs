use edge_ws::{Error as EdgeWsError, FrameHeader, FrameType};
use std::cell::RefCell;

use std::fmt;

use mockall::Predicate;
use predicates_core::reflection::PredicateReflection;

pub struct FrameHeaderMatcher {
    expected_frame_header: FrameHeader,
}

impl FrameHeaderMatcher {
    pub const fn new(expected_type: FrameType) -> Self {
        let expected_frame_header = FrameHeader {
            frame_type: expected_type,
            payload_len: 0,
            mask_key: None,
        };
        Self { expected_frame_header }
    }

    pub const fn with_payload_len(mut self, payload_len: u64) -> Self {
        self.expected_frame_header.payload_len = payload_len;
        self
    }

    pub const fn with_mask_key(mut self, mask_key: Option<u32>) -> Self {
        self.expected_frame_header.mask_key = mask_key;
        self
    }

    pub fn build(self) -> impl Predicate<[u8]> + PredicateReflection {
        FrameHeaderPredicate::new(self.expected_frame_header)
    }
}

enum CheckResult {
    Ok,
    BufferNeExpected {
        expected_size: usize,
        actual_size: usize,
    },
    Mismatch {
        actual_bytes: Vec<u8>,
        actual_frame_header: FrameHeader,
        actual_parsed_size: usize,
    },
    Unparsed {
        actual_bytes: Vec<u8>,
        parse_error: EdgeWsError<()>,
    },
}

struct FrameHeaderPredicate {
    expected_frame_header: FrameHeader,
    expected_header_bytes: Vec<u8>,
    check_result: RefCell<CheckResult>,
}

impl FrameHeaderPredicate {
    fn new(expected_frame_header: FrameHeader) -> Self {
        let mut header_buf = [0u8; FrameHeader::MAX_LEN];

        let expected_header_len = expected_frame_header
            .serialize(&mut header_buf)
            .expect("header_buf len must be sufficient to store the header");
        let expected_header_bytes = &header_buf[..expected_header_len];

        Self {
            expected_frame_header,
            expected_header_bytes: expected_header_bytes.to_vec(),
            check_result: RefCell::new(CheckResult::Ok),
        }
    }
}

impl Predicate<[u8]> for FrameHeaderPredicate {
    fn eval(&self, buf: &[u8]) -> bool {
        // Reset check result to Ok before evaluation
        *self.check_result.borrow_mut() = CheckResult::Ok;

        if self.expected_header_bytes.len() != buf.len() {
            *self.check_result.borrow_mut() = CheckResult::BufferNeExpected {
                expected_size: self.expected_header_bytes.len(),
                actual_size: buf.len(),
            };
            return false;
        }

        let (actual_frame_header, actual_size) = match FrameHeader::deserialize(buf) {
            Ok(res) => res,
            Err(err) => {
                *self.check_result.borrow_mut() = CheckResult::Unparsed {
                    actual_bytes: buf.to_vec(),
                    parse_error: err,
                };
                return false;
            }
        };

        if self.expected_header_bytes.len() != actual_size {
            *self.check_result.borrow_mut() = CheckResult::Mismatch {
                actual_bytes: buf.to_vec(),
                actual_frame_header: actual_frame_header,
                actual_parsed_size: actual_size,
            };
            return false;
        }

        if self.expected_header_bytes != &buf[..actual_size] {
            *self.check_result.borrow_mut() = CheckResult::Mismatch {
                actual_bytes: buf.to_vec(),
                actual_frame_header: actual_frame_header,
                actual_parsed_size: actual_size,
            };
            return false;
        }

        true
    }
}

impl PredicateReflection for FrameHeaderPredicate {}

impl fmt::Display for FrameHeaderPredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let check_result = self.check_result.borrow();
        match &*check_result {
            CheckResult::Ok => write!(f, "frame header: ok"),
            CheckResult::BufferNeExpected {
                expected_size,
                actual_size,
            } => write!(
                f,
                "frame header: buffer size mismatch, expected {} bytes, but got {} bytes",
                expected_size, actual_size
            ),
            CheckResult::Mismatch {
                actual_bytes,
                actual_frame_header,
                actual_parsed_size,
            } => write!(
                f,
                "frame header: mismatch,  expected header {:?}, got header {:?}, expected header size {}, got header size {}, expected bytes {:x?}, got bytes {:x?}",
                self.expected_frame_header,
                actual_frame_header,
                self.expected_header_bytes.len(),
                actual_parsed_size,
                self.expected_header_bytes,
                actual_bytes
            ),
            CheckResult::Unparsed {
                actual_bytes,
                parse_error,
                ..
            } => write!(
                f,
                "frame header: parse error {:?}, actual bytes {:x?}",
                parse_error, actual_bytes
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialize_frame_header<'a>(buf: &'a mut [u8], frame_header: &FrameHeader) -> &'a [u8] {
        let header_len = frame_header
            .serialize(buf)
            .expect("buf len must be sufficient to store the header");
        &buf[..header_len]
    }

    #[test]
    fn test_frame_header_matcher_eval_ok() {
        let actual_frame_header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: 0,
            mask_key: None,
        };
        let mut actual_buf = [0u8; FrameHeader::MAX_LEN];
        let actual_header_bytes = serialize_frame_header(&mut actual_buf, &actual_frame_header);

        let matcher = FrameHeaderMatcher::new(actual_frame_header.frame_type)
            .with_payload_len(actual_frame_header.payload_len)
            .with_mask_key(actual_frame_header.mask_key)
            .build();

        assert!(matcher.eval(actual_header_bytes));

        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(str, "frame header: ok");
    }

    #[test]
    fn test_frame_header_matcher_eval_size_mismatch() {
        let expected_header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: 0,
            mask_key: None,
        };
        let actual_header = expected_header.clone();

        let mut actual_buf = [0u8; FrameHeader::MAX_LEN];
        let actual_header_bytes = serialize_frame_header(&mut actual_buf, &actual_header);

        let matcher = FrameHeaderMatcher::new(actual_header.frame_type)
            .with_payload_len(actual_header.payload_len)
            .with_mask_key(actual_header.mask_key)
            .build();

        assert!(!matcher.eval(&actual_header_bytes[..actual_header_bytes.len() - 1]));

        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(
            str,
            format!(
                "frame header: buffer size mismatch, expected {} bytes, but got {} bytes",
                actual_header_bytes.len(),
                actual_header_bytes.len() - 1
            )
        );
    }

    #[test]
    fn test_frame_header_matcher_eval_mismatch() {
        let expected_header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: 0,
            mask_key: None,
        };
        let actual_header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: 0,
            mask_key: None,
        };
        let mut actual_buf = [0u8; FrameHeader::MAX_LEN];
        let actual_header_bytes = serialize_frame_header(&mut actual_buf, &actual_header);

        let mut expected_buf = [0u8; FrameHeader::MAX_LEN];
        let expected_header_bytes = serialize_frame_header(&mut expected_buf, &expected_header);

        let matcher = FrameHeaderMatcher::new(expected_header.frame_type)
            .with_payload_len(expected_header.payload_len)
            .with_mask_key(expected_header.mask_key)
            .build();

        assert!(!matcher.eval(actual_header_bytes));

        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(
            str,
            format!(
                "frame header: mismatch,  expected header {:?}, got header {:?}, expected header size {}, got header size {}, expected bytes {:x?}, got bytes {:x?}",
                expected_header,
                actual_header,
                expected_header_bytes.len(),
                actual_header_bytes.len(),
                expected_header_bytes,
                actual_header_bytes
            )
        );
    }

    #[test]
    fn test_frame_header_matcher_eval_parsed_with_size_mismatch() {
        let expected_header = FrameHeader {
            frame_type: FrameType::Binary(false),
            payload_len: 65232,
            mask_key: None,
        };
        let actual_header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: 0,
            mask_key: None,
        };

        let mut expected_buf = [0u8; FrameHeader::MAX_LEN];
        let mut actual_buf: [u8; 14] = [0u8; FrameHeader::MAX_LEN];

        let expected_header_bytes = serialize_frame_header(&mut expected_buf, &expected_header);
        let actual_header_len = serialize_frame_header(&mut actual_buf, &actual_header).len();

        let matcher = FrameHeaderMatcher::new(expected_header.frame_type)
            .with_payload_len(expected_header.payload_len)
            .with_mask_key(expected_header.mask_key)
            .build();

        assert!(!matcher.eval(&actual_buf[..expected_header_bytes.len()]));

        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(
            str,
            format!(
                "frame header: mismatch,  expected header {:?}, got header {:?}, expected header size {}, got header size {}, expected bytes {:x?}, got bytes {:x?}",
                expected_header,
                actual_header,
                expected_header_bytes.len(),
                actual_header_len,
                expected_header_bytes,
                &actual_buf[..expected_header_bytes.len()]
            )
        );
    }

    #[test]
    fn test_frame_header_matcher_eval_not_parsed() {
        let actual_frame_header = FrameHeader {
            frame_type: FrameType::Text(false),
            payload_len: 0,
            mask_key: None,
        };
        let invalid_message = [5u8, 0u8];

        let matcher = FrameHeaderMatcher::new(actual_frame_header.frame_type)
            .with_payload_len(actual_frame_header.payload_len)
            .with_mask_key(actual_frame_header.mask_key)
            .build();

        assert!(!matcher.eval(&invalid_message));

        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(
            str,
            format!(
                "frame header: parse error {:?}, actual bytes {:x?}",
                EdgeWsError::<()>::Invalid,
                &invalid_message
            )
        );
    }
}
