/// Internal database primary key (`SERIAL` / `INTEGER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(i64);

impl EntityId {
    pub fn new(value: i64) -> Result<Self, EntityIdError> {
        if value <= 0 {
            return Err(EntityIdError::NonPositive);
        }
        Ok(Self(value))
    }

    pub fn get(&self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityIdError {
    NonPositive,
}

impl std::fmt::Display for EntityIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonPositive => write!(f, "entity id must be positive"),
        }
    }
}

impl std::error::Error for EntityIdError {}
