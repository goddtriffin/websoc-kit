use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Subscription(String);

impl Subscription {
    #[must_use]
    pub fn new(subscription: &str) -> Self {
        Subscription(subscription.to_string())
    }
}

impl From<String> for Subscription {
    fn from(subscription: String) -> Self {
        Subscription::new(subscription.as_str())
    }
}

impl From<&str> for Subscription {
    fn from(subscription: &str) -> Self {
        Subscription::new(subscription)
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Subscription({})", self.0)
    }
}

impl fmt::Display for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
