use crate::types::OrderId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    DuplicateId(OrderId),
    UnknownId(OrderId),
}

impl std::fmt::Display for BookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "order {id} already exists"),
            Self::UnknownId(id) => write!(f, "order {id} not found"),
        }
    }
}

impl std::error::Error for BookError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_id() {
        let id = OrderId::new(7).unwrap();
        assert_eq!(BookError::DuplicateId(id).to_string(), "order 7 already exists");
        assert_eq!(BookError::UnknownId(id).to_string(), "order 7 not found");
    }

    #[test]
    fn is_a_std_error_and_comparable() {
        let id = OrderId::new(1).unwrap();
        let e: Box<dyn std::error::Error> = Box::new(BookError::UnknownId(id));
        assert!(e.to_string().contains("not found"));
        assert_ne!(BookError::UnknownId(id), BookError::DuplicateId(id));
    }
}
