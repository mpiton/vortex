mod checksum;
mod download_engine;
mod reqwest_client;
mod segment_worker;
mod wait_manager;

pub use checksum::StreamingChecksumComputer;
pub use download_engine::SegmentedDownloadEngine;
pub use reqwest_client::ReqwestHttpClient;
pub use wait_manager::WaitManager;

pub(super) fn format_error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(next) = source {
        let next_message = next.to_string();
        if !next_message.is_empty() {
            message.push_str(": ");
            message.push_str(&next_message);
        }
        source = next.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    use super::format_error_chain;

    #[derive(Debug)]
    struct RootError;

    impl Display for RootError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "certificate has expired")
        }
    }

    impl Error for RootError {}

    #[derive(Debug)]
    struct WrappedError {
        message: &'static str,
        source: RootError,
    }

    impl Display for WrappedError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl Error for WrappedError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.source)
        }
    }

    #[test]
    fn test_format_error_chain_includes_nested_causes() {
        let err = WrappedError {
            message: "invalid peer certificate",
            source: RootError,
        };

        assert_eq!(
            format_error_chain(&err),
            "invalid peer certificate: certificate has expired"
        );
    }
}
