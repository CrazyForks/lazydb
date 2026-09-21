use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::identity::ConnectionIdentity;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum PrincipalKind {
    User,
    Role,
}

/// Render-only classification for the explorer tree, including the container group.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalDisplayKind {
    Group,
    User,
    Role,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum PrincipalScope {
    Cluster,
    Server,
    Database(String),
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct PrincipalId {
    pub profile_id: Uuid,
    pub scope: PrincipalScope,
    pub native_id: String,
    /// MySQL/MariaDB accounts are identified by `user` + `host`; the host is
    /// kept as its own field so it is never re-parsed out of a display string.
    #[serde(default)]
    pub host: Option<String>,
}

impl PrincipalId {
    pub fn connection(&self, generation: u64) -> ConnectionIdentity {
        ConnectionIdentity {
            profile_id: self.profile_id,
            generation,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct PrincipalEntry {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub name: String,
    pub native_kind: String,
    pub system: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalPage {
    pub connection: ConnectionIdentity,
    pub entries: Vec<PrincipalEntry>,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalDdl {
    pub principal: PrincipalEntry,
    pub sql: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalPermission {
    pub target: String,
    pub privilege: String,
    pub source: String,
    pub grantable: bool,
    pub source_kind: PrincipalPermissionSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalPermissionSource {
    Direct,
    Public,
    Owner,
    Default,
    Inherited,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrincipalCoverage {
    Complete,
    Partial(String),
    Unavailable(String),
    Unsupported(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalMembership {
    pub role: String,
    pub member: String,
    pub admin_option: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalDetails {
    pub principal: PrincipalEntry,
    pub database: Option<String>,
    pub permissions: Vec<PrincipalPermission>,
    pub member_of: Vec<PrincipalMembership>,
    pub members: Vec<PrincipalMembership>,
    pub permissions_coverage: PrincipalCoverage,
    pub membership_coverage: PrincipalCoverage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalCapability {
    Unsupported,
    DdlOnly,
    Details,
    DetailsAndMutation,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PrincipalReadTarget {
    pub principal: PrincipalId,
    pub database: Option<String>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum PrincipalMutationTarget {
    Database,
    Schema {
        schema: String,
    },
    Relation {
        schema: String,
        relation: String,
    },
    Column {
        schema: String,
        relation: String,
        column: String,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum PrincipalMutation {
    Grant {
        target: PrincipalMutationTarget,
        privilege: String,
        grant_option: bool,
    },
    Revoke {
        target: PrincipalMutationTarget,
        privilege: String,
        grant_option: bool,
    },
    GrantRole {
        role: String,
        admin_option: bool,
    },
    RevokeRole {
        role: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalMutationSection {
    Permission,
    Membership,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalMutationPlan {
    pub connection: ConnectionIdentity,
    pub principal: PrincipalEntry,
    pub database: Option<String>,
    pub sql: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PrincipalMutationRequest {
    pub connection: ConnectionIdentity,
    pub request_id: u64,
    pub principal: PrincipalEntry,
    pub database: Option<String>,
    pub mutation: PrincipalMutation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalMutationDraft {
    pub section: PrincipalMutationSection,
    pub grant: bool,
    pub target: PrincipalMutationTarget,
    pub privilege: String,
    pub grant_option: bool,
    pub role: Option<String>,
    pub admin_option: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalMutationCapabilities {
    pub permission_targets: Vec<PrincipalMutationTargetKind>,
    pub privileges: Vec<String>,
    pub memberships: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalMutationTargetKind {
    Database,
    Schema,
    Relation,
    Column,
    Membership,
}

impl PrincipalMutationDraft {
    pub fn permission(target: PrincipalMutationTarget) -> Self {
        Self {
            section: PrincipalMutationSection::Permission,
            grant: true,
            target,
            privilege: "SELECT".into(),
            grant_option: false,
            role: None,
            admin_option: false,
        }
    }

    pub fn membership(role: impl Into<String>) -> Self {
        Self {
            section: PrincipalMutationSection::Membership,
            grant: true,
            target: PrincipalMutationTarget::Database,
            privilege: String::new(),
            grant_option: false,
            role: Some(role.into()),
            admin_option: false,
        }
    }

    pub fn set_grant(&mut self, grant: bool) {
        self.grant = grant;
    }

    pub fn set_privilege(&mut self, privilege: impl Into<String>) {
        self.privilege = privilege.into();
    }

    pub fn set_target(&mut self, target: PrincipalMutationTarget) {
        self.target = target;
    }

    pub fn set_role(&mut self, role: impl Into<String>) {
        self.role = Some(role.into());
    }

    pub fn mutation(&self) -> PrincipalMutation {
        if self.section == PrincipalMutationSection::Membership {
            return if self.grant {
                PrincipalMutation::GrantRole {
                    role: self.role.clone().unwrap_or_default(),
                    admin_option: self.admin_option,
                }
            } else {
                PrincipalMutation::RevokeRole {
                    role: self.role.clone().unwrap_or_default(),
                }
            };
        }
        if self.grant {
            PrincipalMutation::Grant {
                target: self.target.clone(),
                privilege: self.privilege.clone(),
                grant_option: self.grant_option,
            }
        } else {
            PrincipalMutation::Revoke {
                target: self.target.clone(),
                privilege: self.privilege.clone(),
                grant_option: false,
            }
        }
    }
}
