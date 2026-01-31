//! User domain model

use serde::{Deserialize, Serialize};

/// Represents an authenticated user
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
}

/// COMMENT: I think this User object was legacy from the Python
/// CLI, and I don't think it's used at all. We should consider deleting
/// altogether. Users don't have accounts with ids or emails.
impl User {
    pub fn new(id: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            email: email.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_creation() {
        let user = User::new("user-123", "test@example.com");
        assert_eq!(user.id, "user-123");
        assert_eq!(user.email, "test@example.com");
    }
}
