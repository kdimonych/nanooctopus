use std::cell::RefCell;

use std::fmt;

use crate::web_socket::masker::PayloadMasker;
use mockall::Predicate;
use predicates_core::reflection::PredicateReflection;

pub struct BinaryMatcher {
    expected_binary: Vec<u8>,
}

impl BinaryMatcher {
    pub fn new<A: Into<Vec<u8>>>(expected_binary: A) -> Self {
        Self {
            expected_binary: expected_binary.into(),
        }
    }

    /// Build a predicate that matches the expected binary data byte to byte.
    pub fn build(self) -> impl Predicate<[u8]> + PredicateReflection {
        RawBinaryMatcherPredicate::new(self.expected_binary)
    }

    /// Build a predicate that matches the expected binary data byte to byte, after masking it with the given mask key.
    pub fn build_masked(mut self, mask_key: u32) -> impl Predicate<[u8]> + PredicateReflection {
        PayloadMasker::new(mask_key).mask_chank_simd(self.expected_binary.as_mut_slice());
        RawBinaryMatcherPredicate::new(self.expected_binary)
    }
}

enum RawCheckResult {
    Ok,
    Mismatch(Vec<u8>),
}

struct RawBinaryMatcherPredicate {
    expected_binary: Vec<u8>,
    check_result: RefCell<RawCheckResult>,
}

impl RawBinaryMatcherPredicate {
    fn new(expected_binary: Vec<u8>) -> Self {
        Self {
            expected_binary,
            check_result: RefCell::new(RawCheckResult::Ok),
        }
    }
}

impl Predicate<[u8]> for RawBinaryMatcherPredicate {
    fn eval(&self, buf: &[u8]) -> bool {
        *self.check_result.borrow_mut() = RawCheckResult::Ok;
        if self.expected_binary.len() != buf.len() || self.expected_binary != buf {
            *self.check_result.borrow_mut() = RawCheckResult::Mismatch(buf.to_vec());
            return false;
        }

        true
    }
}

impl PredicateReflection for RawBinaryMatcherPredicate {}

impl fmt::Display for RawBinaryMatcherPredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let check_result = self.check_result.borrow();
        match &*check_result {
            RawCheckResult::Ok => write!(f, "binary: ok"),
            RawCheckResult::Mismatch(actual_bytes) => write!(
                f,
                "binary: mismatch, expected {:?}, but got {:?}",
                self.expected_binary, actual_bytes
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_matcher_eval_ok() {
        let matcher = BinaryMatcher::new(vec![1, 2, 3]).build();
        assert!(matcher.eval(&[1, 2, 3]));
        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(str, "binary: ok");
    }

    #[test]
    fn test_binary_matcher_eval_mismatch() {
        let matcher = BinaryMatcher::new(vec![1, 2, 3]).build();
        assert!(!matcher.eval(&[1, 2, 4]));
        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(str, "binary: mismatch, expected [1, 2, 3], but got [1, 2, 4]");
    }

    #[test]
    fn test_binary_matcher_several_eval_call_not_interfere() {
        let matcher = BinaryMatcher::new(vec![1, 2, 3]).build();
        assert!(!matcher.eval(&[1, 2, 4]));
        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(str, "binary: mismatch, expected [1, 2, 3], but got [1, 2, 4]");

        assert!(matcher.eval(&[1, 2, 3]));
        let mut str = String::new();
        fmt::write(&mut str, format_args!("{matcher}")).unwrap();
        assert_eq!(str, "binary: ok");
    }
}
