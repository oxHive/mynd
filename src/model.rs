use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    Personal,
    Workspace,
    Org,
}

impl std::fmt::Display for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Layer::Personal => write!(f, "personal"),
            Layer::Workspace => write!(f, "workspace"),
            Layer::Org => write!(f, "org"),
        }
    }
}

impl std::str::FromStr for Layer {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "personal" => Ok(Layer::Personal),
            "workspace" => Ok(Layer::Workspace),
            "org" => Ok(Layer::Org),
            _ => Err(anyhow::anyhow!("invalid layer: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Preference,
    Project,
    History,
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryType::Preference => write!(f, "preference"),
            MemoryType::Project => write!(f, "project"),
            MemoryType::History => write!(f, "history"),
        }
    }
}

impl std::str::FromStr for MemoryType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "preference" => Ok(MemoryType::Preference),
            "project" => Ok(MemoryType::Project),
            "history" => Ok(MemoryType::History),
            _ => Err(anyhow::anyhow!("invalid memory type: {s}")),
        }
    }
}

/// Result of attempting to create an edge.
#[derive(Debug, PartialEq, Eq)]
pub enum EdgeCreate {
    Created(String),
    Duplicate,
    MissingEndpoint,
    InvalidRelationship,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_roundtrips_through_string() {
        let l: Layer = "personal".parse().unwrap();
        assert_eq!(l.to_string(), "personal");

        let l: Layer = "workspace".parse().unwrap();
        assert_eq!(l.to_string(), "workspace");
    }

    #[test]
    fn layer_rejects_invalid_value() {
        assert!("unknown".parse::<Layer>().is_err());
    }

    #[test]
    fn memory_type_roundtrips() {
        assert_eq!(
            "preference".parse::<MemoryType>().unwrap().to_string(),
            "preference"
        );
        assert_eq!(
            "project".parse::<MemoryType>().unwrap().to_string(),
            "project"
        );
        assert_eq!(
            "history".parse::<MemoryType>().unwrap().to_string(),
            "history"
        );
    }

    #[test]
    fn memory_type_rejects_invalid_value() {
        assert!("unknown".parse::<MemoryType>().is_err());
    }

    #[test]
    fn layer_equality() {
        assert_eq!(Layer::Personal, Layer::Personal);
        assert_eq!(Layer::Workspace, Layer::Workspace);
        assert_ne!(Layer::Personal, Layer::Workspace);
    }

    #[test]
    fn memory_type_display() {
        assert_eq!(MemoryType::Preference.to_string(), "preference");
        assert_eq!(MemoryType::Project.to_string(), "project");
        assert_eq!(MemoryType::History.to_string(), "history");
    }

    #[test]
    fn layer_org_roundtrips_through_string() {
        let l: Layer = "org".parse().unwrap();
        assert_eq!(l.to_string(), "org");
    }

    #[test]
    fn edge_create_variants_are_distinct() {
        let created = EdgeCreate::Created("edge_abc".to_string());
        assert_ne!(created, EdgeCreate::Duplicate);
        assert_ne!(created, EdgeCreate::MissingEndpoint);
        assert_ne!(EdgeCreate::Duplicate, EdgeCreate::MissingEndpoint);
        assert_eq!(
            EdgeCreate::Created("x".to_string()),
            EdgeCreate::Created("x".to_string())
        );
    }
}
