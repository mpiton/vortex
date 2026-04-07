#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountType {
    Free,
    Premium,
    Debrid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    id: u64,
    service_name: String,
    username: String,
    account_type: AccountType,
    enabled: bool,
    traffic_left: Option<u64>,
    valid_until: Option<u64>,
}

impl Account {
    pub fn new(id: u64, service_name: String, username: String, account_type: AccountType) -> Self {
        Self {
            id,
            service_name,
            username,
            account_type,
            enabled: true,
            traffic_left: None,
            valid_until: None,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_premium(&self) -> bool {
        matches!(
            self.account_type,
            AccountType::Premium | AccountType::Debrid
        )
    }

    pub fn set_traffic_left(&mut self, bytes: u64) {
        self.traffic_left = Some(bytes);
    }

    pub fn set_valid_until(&mut self, timestamp: u64) {
        self.valid_until = Some(timestamp);
    }

    pub fn is_expired(&self, now: u64) -> bool {
        match self.valid_until {
            Some(expiry) => now > expiry,
            None => false,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn account_type(&self) -> AccountType {
        self.account_type
    }

    pub fn traffic_left(&self) -> Option<u64> {
        self.traffic_left
    }

    pub fn valid_until(&self) -> Option<u64> {
        self.valid_until
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account() -> Account {
        Account::new(
            1,
            "ExampleHost".to_string(),
            "user@example.com".to_string(),
            AccountType::Free,
        )
    }

    #[test]
    fn test_account_new() {
        let acc = make_account();
        assert_eq!(acc.id(), 1);
        assert_eq!(acc.service_name(), "ExampleHost");
        assert_eq!(acc.username(), "user@example.com");
        assert_eq!(acc.account_type(), AccountType::Free);
        assert!(acc.is_enabled());
        assert!(acc.traffic_left().is_none());
        assert!(acc.valid_until().is_none());
    }

    #[test]
    fn test_account_enable_disable() {
        let mut acc = make_account();
        assert!(acc.is_enabled());
        acc.disable();
        assert!(!acc.is_enabled());
        acc.enable();
        assert!(acc.is_enabled());
    }

    #[test]
    fn test_account_is_premium() {
        let free = Account::new(1, "H".to_string(), "u".to_string(), AccountType::Free);
        let premium = Account::new(2, "H".to_string(), "u".to_string(), AccountType::Premium);
        let debrid = Account::new(3, "H".to_string(), "u".to_string(), AccountType::Debrid);
        assert!(!free.is_premium());
        assert!(premium.is_premium());
        assert!(debrid.is_premium());
    }

    #[test]
    fn test_account_expiry() {
        let mut acc = make_account();
        assert!(!acc.is_expired(1000));
        acc.set_valid_until(500);
        assert!(acc.is_expired(501));
        assert!(!acc.is_expired(500));
        assert!(!acc.is_expired(499));
    }

    #[test]
    fn test_account_traffic() {
        let mut acc = make_account();
        assert!(acc.traffic_left().is_none());
        acc.set_traffic_left(1_000_000);
        assert_eq!(acc.traffic_left(), Some(1_000_000));
    }
}
