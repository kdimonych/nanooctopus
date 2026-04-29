#[macro_export]
macro_rules! cmd {
        ($($arg:tt)+) => {{
            println!(
                "cargo:{}", format!($($arg)+)
            );
        }};
    }
