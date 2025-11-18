use uuid::Uuid;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum UserRole {
    Owner,
    Admin,
    Sales,
    Viewer,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            UserRole::Owner => "OWNER",
            UserRole::Admin => "ADMIN",
            UserRole::Sales => "SALES",
            UserRole::Viewer => "VIEWER",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "OWNER" => Some(UserRole::Owner),
            "ADMIN" => Some(UserRole::Admin),
            "SALES" => Some(UserRole::Sales),
            "VIEWER" => Some(UserRole::Viewer),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user_id: Uuid,
    pub roles: Vec<UserRole>,
}
