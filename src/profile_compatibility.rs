use uuid::Uuid;

use crate::profile::ProfileCollection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnavailableProfile {
    pub id: Option<Uuid>,
    pub name: String,
    pub kind: Option<String>,
    pub reason: ProfileUnavailableReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileUnavailableReason {
    UnsupportedKind,
    UnsupportedConfiguration,
    InvalidConfiguration,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileLoadReport {
    pub collection: ProfileCollection,
    pub unavailable: Vec<UnavailableProfile>,
}
