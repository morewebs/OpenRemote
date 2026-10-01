use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(
            Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Debug,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }

            pub fn parse(s: &str) -> Option<Self> {
                Uuid::parse_str(s).ok().map(Self)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_newtype!(SessionId);
id_newtype!(DecisionId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = SessionId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.starts_with('"') && json.ends_with('"'));
        assert_eq!(serde_json::from_str::<SessionId>(&json).unwrap(), id);
    }
}
