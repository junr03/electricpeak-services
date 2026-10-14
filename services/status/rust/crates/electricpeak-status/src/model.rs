use serde::{Deserialize, Serialize};

/// Health of one checked thing. `Idle` is a deliberately stopped container,
/// for example a one-shot job that finished successfully.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Up,
    Idle,
    Degraded,
    Down,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Check {
    pub id: String,
    pub name: String,
    pub status: Status,
    pub detail: String,
}

impl Check {
    pub fn new(id: &str, name: &str, status: Status, detail: impl Into<String>) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            status,
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Container {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Overall {
    Operational,
    Degraded,
    Outage,
}

/// The public document. It carries state only: no addresses, network IDs,
/// versions, or other deployment-specific values.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    pub generated_at: u64,
    pub overall: Overall,
    pub checks: Vec<Check>,
    pub containers: Vec<Container>,
}

impl Snapshot {
    #[must_use]
    pub fn new(generated_at: u64, checks: Vec<Check>, mut containers: Vec<Container>) -> Self {
        containers.sort_by(|a, b| a.name.cmp(&b.name));
        let overall = overall(&checks, &containers);
        Self {
            generated_at,
            overall,
            checks,
            containers,
        }
    }
}

fn overall(checks: &[Check], containers: &[Container]) -> Overall {
    if checks.iter().any(|check| check.status == Status::Down) {
        Overall::Outage
    } else if checks
        .iter()
        .any(|check| matches!(check.status, Status::Degraded | Status::Unknown))
        || containers.iter().any(|container| {
            matches!(
                container.status,
                Status::Degraded | Status::Down | Status::Unknown
            )
        })
    {
        Overall::Degraded
    } else {
        Overall::Operational
    }
}
