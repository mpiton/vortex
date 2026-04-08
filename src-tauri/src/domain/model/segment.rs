use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::DownloadId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentState {
    Pending,
    Downloading,
    Completed,
    Error,
}

impl std::str::FromStr for SegmentState {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Pending" => Ok(SegmentState::Pending),
            "Downloading" => Ok(SegmentState::Downloading),
            "Completed" => Ok(SegmentState::Completed),
            "Error" => Ok(SegmentState::Error),
            _ => Err(DomainError::ValidationError(format!(
                "Unknown segment state: {s}"
            ))),
        }
    }
}

impl std::fmt::Display for SegmentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SegmentState::Pending => write!(f, "Pending"),
            SegmentState::Downloading => write!(f, "Downloading"),
            SegmentState::Completed => write!(f, "Completed"),
            SegmentState::Error => write!(f, "Error"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    id: u32,
    download_id: DownloadId,
    start_byte: u64,
    end_byte: u64,
    downloaded_bytes: u64,
    state: SegmentState,
    retry_count: u32,
}

impl Segment {
    pub fn new(id: u32, download_id: DownloadId, start_byte: u64, end_byte: u64) -> Self {
        let (lo, hi) = if start_byte <= end_byte {
            (start_byte, end_byte)
        } else {
            (end_byte, start_byte)
        };
        Segment {
            id,
            download_id,
            start_byte: lo,
            end_byte: hi,
            downloaded_bytes: 0,
            state: SegmentState::Pending,
            retry_count: 0,
        }
    }

    pub fn start(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != SegmentState::Pending {
            return Err(DomainError::InvalidSegmentTransition {
                from: self.state,
                to: SegmentState::Downloading,
            });
        }
        self.state = SegmentState::Downloading;
        Ok(DomainEvent::SegmentStarted {
            download_id: self.download_id,
            segment_id: self.id,
        })
    }

    pub fn complete(&mut self) -> Result<DomainEvent, DomainError> {
        if self.state != SegmentState::Downloading {
            return Err(DomainError::InvalidSegmentTransition {
                from: self.state,
                to: SegmentState::Completed,
            });
        }
        self.downloaded_bytes = self.end_byte - self.start_byte;
        self.state = SegmentState::Completed;
        Ok(DomainEvent::SegmentCompleted {
            download_id: self.download_id,
            segment_id: self.id,
        })
    }

    pub fn fail(&mut self, error: String) -> Result<DomainEvent, DomainError> {
        if self.state != SegmentState::Downloading {
            return Err(DomainError::InvalidSegmentTransition {
                from: self.state,
                to: SegmentState::Error,
            });
        }
        self.state = SegmentState::Error;
        Ok(DomainEvent::SegmentFailed {
            download_id: self.download_id,
            segment_id: self.id,
            error,
        })
    }

    pub fn reset(&mut self) -> Result<(), DomainError> {
        if self.state != SegmentState::Error {
            return Err(DomainError::InvalidSegmentTransition {
                from: self.state,
                to: SegmentState::Pending,
            });
        }
        self.state = SegmentState::Pending;
        self.downloaded_bytes = 0;
        self.retry_count += 1;
        Ok(())
    }

    pub fn update_progress(&mut self, bytes: u64) {
        self.downloaded_bytes = bytes;
    }

    // --- Getters ---

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn download_id(&self) -> DownloadId {
        self.download_id
    }

    pub fn start_byte(&self) -> u64 {
        self.start_byte
    }

    pub fn end_byte(&self) -> u64 {
        self.end_byte
    }

    pub fn downloaded_bytes(&self) -> u64 {
        self.downloaded_bytes
    }

    pub fn state(&self) -> SegmentState {
        self.state
    }

    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    pub fn total_bytes(&self) -> u64 {
        self.end_byte - self.start_byte
    }

    pub fn is_complete(&self) -> bool {
        self.state == SegmentState::Completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_segment() -> Segment {
        Segment::new(1, DownloadId(10), 0, 1024)
    }

    #[test]
    fn test_segment_new_starts_pending() {
        let s = make_segment();
        assert_eq!(s.state(), SegmentState::Pending);
        assert_eq!(s.downloaded_bytes(), 0);
        assert_eq!(s.retry_count(), 0);
    }

    #[test]
    fn test_segment_start_from_pending_succeeds() {
        let mut s = make_segment();
        let result = s.start();
        assert!(result.is_ok());
        assert_eq!(s.state(), SegmentState::Downloading);
        assert_eq!(
            result.unwrap(),
            DomainEvent::SegmentStarted {
                download_id: DownloadId(10),
                segment_id: 1
            }
        );
    }

    #[test]
    fn test_segment_start_from_non_pending_fails() {
        let mut s = make_segment();
        s.start().unwrap();
        let result = s.start();
        assert!(result.is_err());
    }

    #[test]
    fn test_segment_complete_updates_downloaded_bytes() {
        let mut s = make_segment();
        s.start().unwrap();
        let event = s.complete().unwrap();
        assert_eq!(s.state(), SegmentState::Completed);
        assert_eq!(s.downloaded_bytes(), 1024); // end_byte - start_byte = 1024 - 0
        assert!(s.is_complete());
        assert_eq!(
            event,
            DomainEvent::SegmentCompleted {
                download_id: DownloadId(10),
                segment_id: 1
            }
        );
    }

    #[test]
    fn test_segment_fail_from_downloading() {
        let mut s = make_segment();
        s.start().unwrap();
        assert!(s.fail("test error".to_string()).is_ok());
        assert_eq!(s.state(), SegmentState::Error);
    }

    #[test]
    fn test_segment_fail_from_pending_fails() {
        let mut s = make_segment();
        assert!(s.fail("test error".to_string()).is_err());
    }

    #[test]
    fn test_segment_reset_from_error() {
        let mut s = make_segment();
        s.start().unwrap();
        s.fail("test error".to_string()).unwrap();
        assert!(s.reset().is_ok());
        assert_eq!(s.state(), SegmentState::Pending);
        assert_eq!(s.downloaded_bytes(), 0);
        assert_eq!(s.retry_count(), 1);
    }

    #[test]
    fn test_segment_reset_from_non_error_fails() {
        let mut s = make_segment();
        assert!(s.reset().is_err());
    }

    #[test]
    fn test_segment_total_bytes() {
        let s = Segment::new(1, DownloadId(1), 512, 2048);
        assert_eq!(s.total_bytes(), 1536);
    }

    #[test]
    fn test_segment_is_complete_false_when_pending() {
        let s = make_segment();
        assert!(!s.is_complete());
    }

    #[test]
    fn test_segment_update_progress() {
        let mut s = make_segment();
        s.start().unwrap();
        s.update_progress(256);
        assert_eq!(s.downloaded_bytes(), 256);
    }

    #[test]
    fn test_segment_inverted_byte_range_is_normalized() {
        let s = Segment::new(1, DownloadId(1), 2048, 512);
        assert_eq!(s.start_byte(), 512);
        assert_eq!(s.end_byte(), 2048);
        assert_eq!(s.total_bytes(), 1536);
    }

    // Required test names from spec
    #[test]
    fn test_segment_start_emits_started() {
        let mut s = make_segment();
        let event = s.start().unwrap();
        assert_eq!(
            event,
            DomainEvent::SegmentStarted {
                download_id: DownloadId(10),
                segment_id: 1
            }
        );
    }

    #[test]
    fn test_segment_validates_byte_range() {
        // equal bytes is valid (zero-length segment)
        let s = Segment::new(1, DownloadId(1), 100, 100);
        assert_eq!(s.total_bytes(), 0);

        // inverted is silently normalized
        let s2 = Segment::new(1, DownloadId(1), 500, 100);
        assert_eq!(s2.start_byte(), 100);
        assert_eq!(s2.end_byte(), 500);
    }
}
