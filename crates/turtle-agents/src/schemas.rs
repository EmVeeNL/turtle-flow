//! JSON Schemas handed to `claude --json-schema` for the roles with a structured contract.
//! They mirror the serde types in `turtle_domain` (which use `deny_unknown_fields`, so the
//! schemas forbid extra properties too). A test keeps the two in step.

use serde_json::{Value, json};
use turtle_domain::Role;

fn strings() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

#[must_use]
pub fn specification() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "criteria"],
        "properties": {
            "summary": { "type": "string" },
            "in_scope": strings(),
            "out_of_scope": strings(),
            "criteria": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "statement", "verification"],
                    "properties": {
                        "id": { "type": "string" },
                        "statement": { "type": "string" },
                        "verification": { "enum": ["test", "command", "review", "manual"] }
                    }
                }
            },
            "affected_areas": strings(),
            "risks": strings(),
            "assumptions": strings()
        }
    })
}

#[must_use]
pub fn plan() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["tasks"],
        "properties": {
            "tasks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "title", "instructions"],
                    "properties": {
                        "id": { "type": "string" },
                        "title": { "type": "string" },
                        "instructions": { "type": "string" },
                        "expected_paths": strings(),
                        "criteria": strings(),
                        "depends_on": strings(),
                        "complexity": { "enum": ["XS", "S", "M", "L", "XL", null] }
                    }
                }
            }
        }
    })
}

#[must_use]
pub fn review() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary"],
        "properties": {
            "summary": { "type": "string" },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "severity", "category", "message"],
                    "properties": {
                        "id": { "type": "string" },
                        "severity": { "enum": ["nit", "minor", "major", "blocker"] },
                        "category": { "type": "string" },
                        "file": { "type": ["string", "null"] },
                        "line": { "type": ["integer", "null"], "minimum": 1 },
                        "message": { "type": "string" },
                        "suggestion": { "type": ["string", "null"] }
                    }
                }
            },
            "replan": { "type": "boolean" },
            "scope_creep": { "type": "boolean" }
        }
    })
}

#[must_use]
pub fn verification() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["criteria"],
        "properties": {
            "criteria": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "status"],
                    "properties": {
                        "id": { "type": "string" },
                        "status": { "enum": ["met", "not_met", "unverifiable"] },
                        "evidence": { "type": "string" }
                    }
                }
            },
            "structural": { "type": "boolean" }
        }
    })
}

#[must_use]
pub fn discovery() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary"],
        "properties": {
            "summary": { "type": "string" },
            "relevant_files": strings(),
            "risks": strings()
        }
    })
}

/// The schema for a role's structured output; `None` for roles that just change files or write
/// prose.
#[must_use]
pub fn for_role(role: Role) -> Option<Value> {
    match role {
        Role::Discovery => Some(discovery()),
        Role::SpecWriter => Some(specification()),
        Role::Planner => Some(plan()),
        Role::Reviewer => Some(review()),
        Role::Verifier => Some(verification()),
        Role::Implementer | Role::Fixer | Role::Summarizer | Role::ReleaseNotes => None,
    }
}

/// Codex's strict structured-output contract requires every declared object property. Empty
/// arrays and nullable values preserve the meaning of optional fields in our domain types.
#[must_use]
pub fn codex_for_role(role: Role) -> Option<Value> {
    fn require_all(value: &mut Value) {
        if let Some(properties) = value.get_mut("properties").and_then(Value::as_object_mut) {
            let keys = properties.keys().cloned().map(Value::String).collect();
            for property in properties.values_mut() {
                require_all(property);
            }
            value["required"] = Value::Array(keys);
        }
        if let Some(items) = value.get_mut("items") {
            require_all(items);
        }
    }
    for_role(role).map(|mut schema| {
        require_all(&mut schema);
        schema
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use turtle_domain::{
        AcceptanceCriterion, CriterionResult, CriterionStatus, Finding, Plan, PlanTask,
        ReviewReport, Severity, Specification, VerificationMethod, VerificationReport,
    };

    /// Every key the Rust type serializes is a declared property, and every required property
    /// is present: so a field added to a domain type cannot silently be forbidden or omitted.
    fn conforms(schema: &Value, sample: &Value, at: &str) {
        match (schema["type"].as_str(), sample) {
            (Some("object"), Value::Object(map)) => {
                let props = schema["properties"].as_object().expect("properties");
                for key in map.keys() {
                    assert!(
                        props.contains_key(key),
                        "{at}: `{key}` is not in the schema"
                    );
                }
                for req in schema["required"].as_array().expect("required") {
                    let req = req.as_str().expect("name");
                    assert!(map.contains_key(req), "{at}: required `{req}` missing");
                }
                for (key, value) in map {
                    conforms(&props[key], value, &format!("{at}.{key}"));
                }
            }
            (Some("array"), Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    conforms(&schema["items"], item, &format!("{at}[{i}]"));
                }
            }
            _ => {}
        }
    }

    #[test]
    fn schemas_match_the_domain_types() {
        let spec = Specification {
            summary: "s".into(),
            in_scope: vec!["a".into()],
            out_of_scope: vec!["b".into()],
            criteria: vec![AcceptanceCriterion {
                id: "AC-1".into(),
                statement: "works".into(),
                verification: VerificationMethod::Test,
            }],
            affected_areas: vec!["src/**".into()],
            risks: vec!["r".into()],
            assumptions: vec!["a".into()],
        };
        conforms(
            &specification(),
            &serde_json::to_value(&spec).expect("json"),
            "spec",
        );

        let plan_doc = Plan {
            tasks: vec![PlanTask {
                id: "T1".into(),
                title: "t".into(),
                instructions: "i".into(),
                expected_paths: vec!["src/**".into()],
                criteria: vec!["AC-1".into()],
                depends_on: vec![],
                complexity: Some(turtle_domain::Complexity::M),
            }],
        };
        conforms(
            &plan(),
            &serde_json::to_value(&plan_doc).expect("json"),
            "plan",
        );

        let report = ReviewReport {
            summary: "s".into(),
            findings: vec![Finding {
                id: "F1".into(),
                severity: Severity::Major,
                category: "bug".into(),
                file: Some("a.rs".into()),
                line: Some(3),
                message: "m".into(),
                suggestion: Some("s".into()),
            }],
            replan: false,
            scope_creep: false,
        };
        conforms(
            &review(),
            &serde_json::to_value(&report).expect("json"),
            "review",
        );

        let verification_doc = VerificationReport {
            criteria: vec![CriterionResult {
                id: "AC-1".into(),
                status: CriterionStatus::Met,
                evidence: "ran".into(),
            }],
            structural: false,
        };
        conforms(
            &verification(),
            &serde_json::to_value(&verification_doc).expect("json"),
            "verification",
        );
    }

    #[test]
    fn enum_values_match_serde_names() {
        let names = |schema: &Value, path: &[&str]| -> Vec<String> {
            let mut v = schema;
            for p in path {
                v = &v[*p];
            }
            v["enum"]
                .as_array()
                .expect("enum")
                .iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        };
        let methods = names(
            &specification(),
            &[
                "properties",
                "criteria",
                "items",
                "properties",
                "verification",
            ],
        );
        for m in [
            VerificationMethod::Test,
            VerificationMethod::Command,
            VerificationMethod::Review,
            VerificationMethod::Manual,
        ] {
            let s = serde_json::to_value(m).expect("json");
            assert!(methods.contains(&s.as_str().expect("str").to_owned()));
        }
    }

    #[test]
    fn only_document_roles_have_schemas() {
        assert!(for_role(Role::Implementer).is_none());
        assert!(for_role(Role::Planner).is_some());
    }

    #[test]
    fn codex_requires_every_property_at_every_object_level() {
        fn assert_strict(schema: &Value) {
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                let required = schema["required"].as_array().expect("required");
                assert_eq!(required.len(), properties.len());
                for key in properties.keys() {
                    assert!(
                        required.contains(&Value::String(key.clone())),
                        "missing {key}"
                    );
                }
                for property in properties.values() {
                    assert_strict(property);
                }
            }
            if let Some(items) = schema.get("items") {
                assert_strict(items);
            }
        }
        for role in [
            Role::Discovery,
            Role::SpecWriter,
            Role::Planner,
            Role::Reviewer,
            Role::Verifier,
        ] {
            assert_strict(&codex_for_role(role).expect("schema"));
        }
    }
}
