// in nanooctopus_server/src/log.rs
#[cfg(feature = "defmt")]
pub use defmt::{debug, error, info, trace, warn};

#[cfg(all(not(feature = "defmt"), feature = "log"))]
pub use log::{debug, error, info, trace, warn};

#[cfg(not(any(feature = "defmt", feature = "log")))]
pub mod noop {
    #[macro_export]
    macro_rules! debug {
        ($($t:tt)*) => {};
    }
    #[macro_export]
    macro_rules! info {
        ($($t:tt)*) => {};
    }
    #[macro_export]
    macro_rules! warn {
        ($($t:tt)*) => {};
    }
    #[macro_export]
    macro_rules! error {
        ($($t:tt)*) => {};
    }
    #[macro_export]
    macro_rules! trace {
        ($($t:tt)*) => {};
    }
}
