//! `wire_enum!`: the Evergreen string-enum pattern, generated.
//!
//! Emits a `#[non_exhaustive]` enum whose known variants map to wire strings,
//! plus an `Unknown { <field>, data }` variant, the three Unknown helpers,
//! `Display`, `Serialize`, and a `Deserialize` that never fails. `|`-listed
//! aliases are accepted on deserialize and normalized to the first spelling.

macro_rules! wire_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident = $wire:literal $(| $alias:literal)*
            ),+ $(,)?
        }
        unknown($field:ident, $getter:ident)
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// Unknown variant for forward compatibility (Evergreen pattern).
            Unknown {
                /// The unrecognized value from the API.
                $field: String,
                /// The raw JSON value, preserved for roundtrip.
                data: serde_json::Value,
            },
        }

        impl $name {
            fn as_wire(&self) -> &str {
                match self {
                    $( Self::$variant => $wire, )+
                    Self::Unknown { $field, .. } => $field,
                }
            }

            /// Returns true if this is an unknown value.
            #[must_use]
            pub const fn is_unknown(&self) -> bool {
                matches!(self, Self::Unknown { .. })
            }

            /// Returns the unrecognized wire value, if this is unknown.
            #[must_use]
            pub fn $getter(&self) -> Option<&str> {
                match self {
                    Self::Unknown { $field, .. } => Some($field),
                    _ => None,
                }
            }

            /// Returns the preserved JSON, if this is unknown.
            #[must_use]
            pub fn unknown_data(&self) -> Option<&serde_json::Value> {
                match self {
                    Self::Unknown { data, .. } => Some(data),
                    _ => None,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_wire())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_wire())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
                match value.as_str() {
                    $( Some($wire $(| $alias)*) => Ok(Self::$variant), )+
                    Some(other) => {
                        tracing::warn!(
                            "Encountered unknown {} '{other}' - using Unknown variant (Evergreen)",
                            stringify!($name)
                        );
                        Ok(Self::Unknown {
                            $field: other.to_string(),
                            data: value.clone(),
                        })
                    }
                    None => {
                        tracing::warn!(
                            "{} received non-string value: {value}. Preserving in Unknown variant.",
                            stringify!($name)
                        );
                        Ok(Self::Unknown {
                            $field: format!("<non-string: {value}>"),
                            data: value,
                        })
                    }
                }
            }
        }
    };
}

pub(crate) use wire_enum;

#[cfg(test)]
mod tests {
    wire_enum! {
        /// Test enum.
        pub enum Sample {
            /// A.
            Alpha = "alpha" | "ALPHA",
            /// B.
            Beta = "beta",
        }
        unknown(sample_type, unknown_sample_type)
    }

    #[test]
    fn known_alias_unknown_and_non_string() {
        let alpha: Sample = serde_json::from_value(serde_json::json!("ALPHA")).unwrap();
        assert_eq!(alpha, Sample::Alpha);
        assert_eq!(serde_json::to_value(&alpha).unwrap(), "alpha");
        assert_eq!(Sample::Beta.to_string(), "beta");

        let other: Sample = serde_json::from_value(serde_json::json!("gamma")).unwrap();
        assert!(other.is_unknown());
        assert_eq!(other.unknown_sample_type(), Some("gamma"));
        assert_eq!(other.unknown_data(), Some(&serde_json::json!("gamma")));
        assert_eq!(serde_json::to_value(&other).unwrap(), "gamma");

        let number: Sample = serde_json::from_value(serde_json::json!(7)).unwrap();
        assert!(number.is_unknown());
    }
}
