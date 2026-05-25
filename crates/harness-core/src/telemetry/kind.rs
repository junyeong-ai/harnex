//! # Closed-schema telemetry Kind
//!
//! A minimal JSON-Schema subset sufficient for the toolkit's use case:
//! `type=object` payloads with `required: [...]` and `properties: {<name>:
//! {type, enum?}}`. Fields not declared in `properties` are REJECTED
//! (closed schema). Types supported: `string | integer | number | boolean`.
//! Full JSON Schema is intentionally out of scope — the toolkit privileges
//! the closed-schema discipline over feature completeness.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindSchema {
    pub required: HashSet<String>,
    pub properties: HashMap<String, PropertySchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PropertySchema {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    #[serde(rename = "enum", default, skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<Value>>,
}

impl KindSchema {
    pub fn from_value(v: &Value) -> Result<Self> {
        let obj = v.as_object().ok_or_else(|| Error::ConfigInvalid {
            message: "telemetry kind payload_schema must be an object".into(),
            location: None,
        })?;

        if obj.get("type").and_then(|t| t.as_str()) != Some("object") {
            return Err(Error::ConfigInvalid {
                message: "telemetry kind payload_schema.type must be \"object\"".into(),
                location: None,
            });
        }

        // `required` must be an array of strings. A non-string entry is a
        // malformed schema — reject at load rather than silently drop it
        // (a dropped requirement weakens validation invisibly).
        let required: HashSet<String> = match obj.get("required") {
            None => HashSet::new(),
            Some(Value::Array(arr)) => {
                let mut set = HashSet::new();
                for entry in arr {
                    let Some(s) = entry.as_str() else {
                        return Err(Error::ConfigInvalid {
                            message: format!(
                                "telemetry kind payload_schema.required must be an array of \
                                 strings; got {entry}"
                            ),
                            location: None,
                        });
                    };
                    set.insert(s.to_string());
                }
                set
            }
            Some(_) => {
                return Err(Error::ConfigInvalid {
                    message: "telemetry kind payload_schema.required must be an array".into(),
                    location: None,
                });
            }
        };

        // Each property schema must deserialize cleanly. A malformed entry
        // (unknown type, bad enum) must fail at load — never become an
        // unconstrained field via a silent default.
        let mut properties: HashMap<String, PropertySchema> = HashMap::new();
        if let Some(map) = obj.get("properties").and_then(|p| p.as_object()) {
            for (k, v) in map {
                let prop: PropertySchema =
                    serde_json::from_value(v.clone()).map_err(|e| Error::ConfigInvalid {
                        message: format!(
                            "telemetry kind payload_schema.properties.{k} is malformed: {e}"
                        ),
                        location: None,
                    })?;
                properties.insert(k.clone(), prop);
            }
        }

        // Every required field must also be declared in properties.
        for r in &required {
            if !properties.contains_key(r) {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "telemetry kind required field '{r}' has no entry in properties"
                    ),
                    location: None,
                });
            }
        }

        // Each declared `type` must be one of the supported primitives — at
        // load time, not only when a payload happens to exercise the field.
        for (name, prop) in &properties {
            if let Some(ty) = &prop.ty
                && !matches!(ty.as_str(), "string" | "integer" | "number" | "boolean")
            {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "telemetry kind property '{name}' has unknown type '{ty}' \
                         (use string | integer | number | boolean)"
                    ),
                    location: None,
                });
            }
        }

        Ok(Self {
            required,
            properties,
        })
    }

    pub fn validate(&self, payload: &Value) -> Result<()> {
        let obj = payload
            .as_object()
            .ok_or_else(|| Error::TelemetryPayloadInvalid {
                message: "payload must be a JSON object".into(),
            })?;

        for req in &self.required {
            if !obj.contains_key(req) {
                return Err(Error::TelemetryPayloadInvalid {
                    message: format!("missing required field '{req}'"),
                });
            }
        }

        for (name, value) in obj {
            let Some(schema) = self.properties.get(name) else {
                return Err(Error::TelemetryPayloadInvalid {
                    message: format!("field '{name}' is not declared in payload_schema"),
                });
            };

            if let Some(ty) = &schema.ty {
                let ok = match ty.as_str() {
                    "string" => value.is_string(),
                    "integer" => value.is_i64() || value.is_u64(),
                    "number" => value.is_number(),
                    "boolean" => value.is_boolean(),
                    other => {
                        return Err(Error::ConfigInvalid {
                            message: format!("unknown property type '{other}'"),
                            location: None,
                        });
                    }
                };
                if !ok {
                    return Err(Error::TelemetryPayloadInvalid {
                        message: format!("field '{name}' is not a {ty}"),
                    });
                }
            }

            if let Some(allowed) = &schema.enum_values
                && !allowed.iter().any(|a| a == value)
            {
                return Err(Error::TelemetryPayloadInvalid {
                    message: format!("field '{name}' value not in declared enum"),
                });
            }
        }

        Ok(())
    }
}
