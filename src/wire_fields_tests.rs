use super::*;

use serde_json::json;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

#[test]
fn extra_is_what_the_variant_did_not_read() {
    let wire = object(json!({"type": "t", "id": "x", "sig": null, "future": {"a": 1}}));
    let mut fields = WireFields::tagged(&wire);
    assert_eq!(fields.required::<String>("id").unwrap(), "x");
    assert_eq!(fields.optional::<String>("sig").unwrap(), None);
    assert_eq!(fields.optional::<String>("absent").unwrap(), None);
    assert_eq!(fields.extra(), object(json!({"future": {"a": 1}})));
}

#[test]
fn nested_objects_keep_a_type_key_as_data() {
    let wire = object(json!({"type": "t", "arguments": {"urls": [], "type": "kept"}}));
    let mut fields = WireFields::tagged(&wire);
    let mut arguments = fields.object("arguments").unwrap().unwrap();
    let urls: Vec<String> = arguments.or_default("urls").unwrap();
    assert!(urls.is_empty());
    assert_eq!(arguments.extra(), object(json!({"type": "kept"})));
    assert!(fields.extra().is_empty());
}

#[test]
fn missing_required_and_mistyped_fields_are_errors() {
    let wire = object(json!({"type": "t", "id": 7, "arguments": "not an object"}));
    let mut fields = WireFields::tagged(&wire);
    assert!(fields.required::<String>("name").is_err());
    assert!(fields.required::<String>("id").is_err());
    assert!(fields.object("arguments").is_err());
}

struct Probe {
    signature: Option<String>,
    extra: Map<String, Value>,
}

impl Serialize for Probe {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = ModeledMap::tagged(serializer, "probe")?;
        map.field("id", "x")?;
        map.opt("signature", self.signature.as_ref())?;
        map.field_unless(true, "summary", &Vec::<String>::new())?;
        map.end(&self.extra)
    }
}

/// A hand-built `extra` cannot retag the value or shadow a modeled field,
/// even one left unset, but its other keys go out.
#[test]
fn extras_never_override_type_or_modeled_fields() {
    let probe = Probe {
        signature: None,
        extra: object(json!({
            "type": "forged",
            "id": "shadow",
            "signature": "shadow",
            "summary": ["shadow"],
            "future": true
        })),
    };
    assert_eq!(
        serde_json::to_value(&probe).unwrap(),
        json!({"type": "probe", "id": "x", "future": true})
    );
}

#[test]
fn merged_arguments_put_modeled_entries_first_and_win() {
    let extra = object(json!({"urls": ["shadow"], "code": "shadow", "max_pages": 3}));
    // An unset modeled key is left out, not filled from the extras.
    let merged = merged_arguments([("urls", Some(json!(["a"]))), ("code", None)], &extra);
    assert_eq!(
        Value::Object(merged),
        json!({"urls": ["a"], "max_pages": 3})
    );
}
