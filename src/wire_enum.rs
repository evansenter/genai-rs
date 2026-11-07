//! `wire_enum!`: the Evergreen string-enum pattern, generated.
//!
//! Emits a `#[non_exhaustive]` enum whose known variants map to wire strings,
//! plus an `Unknown { <field>, data }` variant, the three Unknown helpers,
//! `Display` (the wire string), `Serialize`, and a `Deserialize` that never
//! fails on an unrecognized value — unless the `genai_strict_unknown` cfg is
//! on, when it errors instead. `|`-listed aliases are accepted on
//! deserialize and normalized to the first spelling.
//!
//! A non-string value lands in `Unknown` with `<non-string: …>` as its type
//! and the original JSON in `data`, and serializes back as that JSON, so it
//! round-trips like a string does. A hand-built `Unknown` always serializes
//! its type string. [`unknown_type`] and [`serialize_wire`] hold that rule,
//! and the Antigravity harness enums (`src/antigravity/protocol/enums.rs`),
//! which keep soft typing in strict mode too, share them.

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
                $crate::wire_enum::serialize_wire(self.as_wire(), self.unknown_data(), serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
                if let Some(wire) = value.as_str() {
                    match wire {
                        $( $wire $(| $alias)* => return Ok(Self::$variant), )+
                        _ => {}
                    }
                }
                let $field = $crate::wire_enum::unknown_type(&value);
                #[cfg(genai_strict_unknown)]
                {
                    Err(<D::Error as serde::de::Error>::custom(format!(
                        "Unknown {} '{}'. Strict mode is enabled via the 'genai_strict_unknown' \
                         cfg flag. Either update the library or disable strict mode.",
                        stringify!($name),
                        $field
                    )))
                }
                #[cfg(not(genai_strict_unknown))]
                {
                    tracing::warn!(
                        "Encountered unknown {} '{}' - using Unknown variant (Evergreen)",
                        stringify!($name),
                        $field
                    );
                    Ok(Self::Unknown { $field, data: value })
                }
            }
        }
    };
}

pub(crate) use wire_enum;

/// The `Unknown` type `wire_enum!` gives a non-string wire value.
pub(crate) fn non_string_descriptor(value: &serde_json::Value) -> String {
    format!("<non-string: {value}>")
}

/// The `Unknown` type of an unrecognized wire value: the string itself, or
/// its [`non_string_descriptor`].
pub(crate) fn unknown_type(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map_or_else(|| non_string_descriptor(value), str::to_owned)
}

/// Writes a string enum's wire value. `wire` is the variant's string (an
/// `Unknown`'s type), and `unknown_data` an `Unknown`'s preserved JSON. A
/// non-string value read from the wire goes back out as it came, not as its
/// `<non-string: ...>` descriptor; anything else, a hand-built `Unknown`
/// included, is written as `wire`.
pub(crate) fn serialize_wire<S: serde::Serializer>(
    wire: &str,
    unknown_data: Option<&serde_json::Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match unknown_data {
        Some(data) if !data.is_string() && wire == non_string_descriptor(data) => {
            serde::Serialize::serialize(data, serializer)
        }
        _ => serializer.serialize_str(wire),
    }
}

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
    fn known_and_alias() {
        let alpha: Sample = serde_json::from_value(serde_json::json!("ALPHA")).unwrap();
        assert_eq!(alpha, Sample::Alpha);
        assert_eq!(serde_json::to_value(&alpha).unwrap(), "alpha");
        assert_eq!(Sample::Beta.to_string(), "beta");
    }

    #[test]
    fn unknown_serializes_its_wire_value() {
        let unknown = Sample::Unknown {
            sample_type: "gamma".to_string(),
            data: serde_json::json!("gamma"),
        };
        assert!(unknown.is_unknown() && !Sample::Alpha.is_unknown());
        assert_eq!(unknown.unknown_sample_type(), Some("gamma"));
        assert_eq!(unknown.unknown_data(), Some(&serde_json::json!("gamma")));
        assert_eq!(unknown.to_string(), "gamma");
        assert_eq!(serde_json::to_value(&unknown).unwrap(), "gamma");
    }

    #[cfg(genai_strict_unknown)]
    #[test]
    fn strict_mode_rejects_unknown_and_non_string() {
        for value in [serde_json::json!("gamma"), serde_json::json!(7)] {
            let err = serde_json::from_value::<Sample>(value).unwrap_err();
            assert!(err.to_string().contains("genai_strict_unknown"), "{err}");
        }
    }

    #[cfg(not(genai_strict_unknown))]
    #[test]
    fn unknown_and_non_string() {
        let other: Sample = serde_json::from_value(serde_json::json!("gamma")).unwrap();
        assert!(other.is_unknown());
        assert_eq!(other.unknown_sample_type(), Some("gamma"));
        assert_eq!(other.unknown_data(), Some(&serde_json::json!("gamma")));
        assert_eq!(serde_json::to_value(&other).unwrap(), "gamma");

        let number: Sample = serde_json::from_value(serde_json::json!(7)).unwrap();
        assert_eq!(number.unknown_sample_type(), Some("<non-string: 7>"));
        assert_eq!(number.unknown_data(), Some(&serde_json::json!(7)));
    }

    #[cfg(not(genai_strict_unknown))]
    #[test]
    fn a_non_string_value_round_trips() {
        for value in [
            serde_json::json!(7),
            serde_json::json!(null),
            serde_json::json!(true),
            serde_json::json!({"mode": "x"}),
            serde_json::json!(["a"]),
        ] {
            let parsed: Sample = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
        }
    }

    #[test]
    fn a_hand_built_unknown_serializes_its_type() {
        // `data` that does not match the type, as a caller might build it:
        // the type string wins.
        let unknown = Sample::Unknown {
            sample_type: "gamma".to_string(),
            data: serde_json::Value::Null,
        };
        assert_eq!(serde_json::to_value(&unknown).unwrap(), "gamma");
    }
}
