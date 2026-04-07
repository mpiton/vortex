use crate::domain::error::DomainError;

/// Priority level 1-10 (1 = lowest, 10 = highest)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Priority(u8);

impl Priority {
    pub fn new(value: u8) -> Result<Self, DomainError> {
        if value == 0 || value > 10 {
            return Err(DomainError::InvalidPriority(format!(
                "Priority must be 1-10, got {value}"
            )));
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> u8 {
        self.0
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self(5)
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Position in download queue
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueuePosition(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_new_valid_range() {
        for i in 1u8..=10 {
            let p = Priority::new(i).unwrap();
            assert_eq!(p.value(), i);
        }
    }

    #[test]
    fn test_priority_new_rejects_zero() {
        assert!(Priority::new(0).is_err());
    }

    #[test]
    fn test_priority_new_rejects_above_ten() {
        assert!(Priority::new(11).is_err());
        assert!(Priority::new(255).is_err());
    }

    #[test]
    fn test_priority_default_is_five() {
        assert_eq!(Priority::default().value(), 5);
    }

    #[test]
    fn test_priority_ordering() {
        let low = Priority::new(1).unwrap();
        let high = Priority::new(10).unwrap();
        assert!(high > low);
        assert!(low < high);
        let mid = Priority::new(5).unwrap();
        assert!(mid > low);
        assert!(mid < high);
    }

    #[test]
    fn test_queue_position_ordering() {
        let first = QueuePosition(0);
        let second = QueuePosition(1);
        let last = QueuePosition(999);
        assert!(first < second);
        assert!(second < last);
        assert_eq!(first, QueuePosition(0));
    }
}
