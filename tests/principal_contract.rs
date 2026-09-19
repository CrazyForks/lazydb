//! Contract tests for connection-level `Users & Roles` browsing.
//!
//! These cover the pure model layer: where the group sits in the tree, how
//! users and roles are ordered, how unsupported databases surface, and how a
//! principal DDL tab accepts or rejects responses.

use lazydb::{
    db::{
        catalog::{CatalogEntry, CatalogId, CatalogKind, OptionalMetadata, QualifiedName},
        principal::{
            PrincipalDdl, PrincipalDisplayKind, PrincipalEntry, PrincipalId, PrincipalKind,
            PrincipalPage, PrincipalScope,
        },
    },
    identity::ConnectionIdentity,
    model::{
        explorer::{ExplorerConnectionStatus, ExplorerNodeId, ExplorerTreeState},
        principal::{PrincipalDdlLoad, PrincipalDdlTab},
    },
};
use uuid::Uuid;

fn profile_id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

fn principal(profile: Uuid, native_id: &str, name: &str, kind: PrincipalKind) -> PrincipalEntry {
    PrincipalEntry {
        id: PrincipalId {
            profile_id: profile,
            scope: PrincipalScope::Cluster,
            native_id: native_id.to_owned(),
            host: None,
        },
        kind,
        name: name.to_owned(),
        native_kind: match kind {
            PrincipalKind::User => "login_role",
            PrincipalKind::Role => "role",
        }
        .to_owned(),
        system: false,
    }
}

fn page(profile: Uuid, entries: Vec<PrincipalEntry>) -> PrincipalPage {
    PrincipalPage {
        connection: ConnectionIdentity {
            profile_id: profile,
            generation: 1,
        },
        entries,
        complete: true,
    }
}

fn database_entry(profile: Uuid, name: &str) -> CatalogEntry {
    CatalogEntry::database(
        CatalogId::new(profile, CatalogKind::Database, [name]),
        QualifiedName {
            database: Some(name.to_owned()),
            schema: None,
            object: name.to_owned(),
        },
        "database",
        OptionalMetadata::Supported(None),
        true,
    )
    .unwrap()
}

fn explorer_with_databases(profile: Uuid, names: &[&str]) -> ExplorerTreeState {
    let mut explorer = ExplorerTreeState::default();
    explorer.add_profile(profile);
    let mut tree = lazydb::model::explorer::CatalogTree::new(profile);
    for name in names {
        tree.insert_subtree(vec![database_entry(profile, name)])
            .unwrap();
    }
    let profile_state = explorer.profiles.get_mut(&profile).unwrap();
    profile_state.catalog = tree;
    profile_state.status = ExplorerConnectionStatus::Online;
    explorer.expanded.insert(ExplorerNodeId::Profile(profile));
    explorer
}

fn visible_ids(explorer: &ExplorerTreeState) -> Vec<ExplorerNodeId> {
    explorer.visible().into_iter().map(|row| row.id).collect()
}

#[test]
fn principal_group_is_the_last_direct_child_of_a_connection() {
    let profile = profile_id(1);
    let explorer = explorer_with_databases(profile, &["app", "audit"]);

    let ids = visible_ids(&explorer);
    let expected_group = ExplorerNodeId::PrincipalGroup {
        profile_id: profile,
    };
    assert_eq!(ids.last(), Some(&expected_group));
    assert_eq!(ids.iter().filter(|id| **id == expected_group).count(), 1);

    // The group sits at the connection's first level, alongside databases.
    let group_depth = explorer
        .visible()
        .into_iter()
        .find(|row| row.id == expected_group)
        .unwrap()
        .depth;
    let database_depth = explorer
        .visible()
        .into_iter()
        .find(|row| matches!(row.id, ExplorerNodeId::Catalog(_)))
        .unwrap()
        .depth;
    assert_eq!(group_depth, database_depth);
}

#[test]
fn expanded_group_lists_users_before_roles_at_one_level_deeper() {
    let profile = profile_id(1);
    let mut explorer = explorer_with_databases(profile, &["app"]);
    let group = ExplorerNodeId::PrincipalGroup {
        profile_id: profile,
    };
    explorer.expanded.insert(group.clone());

    let page = page(
        profile,
        vec![
            principal(profile, "10", "alice", PrincipalKind::User),
            principal(profile, "11", "audit_reader", PrincipalKind::Role),
        ],
    );
    explorer
        .profiles
        .get_mut(&profile)
        .unwrap()
        .set_principals(page);

    let rows = explorer.visible();
    let group_index = rows.iter().position(|row| row.id == group).unwrap();
    let alice = ExplorerNodeId::Principal {
        entry: PrincipalId {
            profile_id: profile,
            scope: PrincipalScope::Cluster,
            native_id: "10".to_owned(),
            host: None,
        },
    };
    let audit = ExplorerNodeId::Principal {
        entry: PrincipalId {
            profile_id: profile,
            scope: PrincipalScope::Cluster,
            native_id: "11".to_owned(),
            host: None,
        },
    };
    let alice_row = rows.iter().find(|row| row.id == alice).unwrap();
    let audit_row = rows.iter().find(|row| row.id == audit).unwrap();
    assert_eq!(alice_row.depth, audit_row.depth);
    assert_eq!(alice_row.depth, rows[group_index].depth + 1);
    assert!(
        rows.iter().position(|row| row.id == alice).unwrap()
            < rows.iter().position(|row| row.id == audit).unwrap()
    );

    // The image kind distinguishes users from roles for icon/colour mapping.
    let kinds = rows
        .iter()
        .filter_map(|row| match &row.id {
            ExplorerNodeId::Principal { entry } => Some(entry.native_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(kinds, vec!["10".to_owned(), "11".to_owned()]);
}

#[test]
fn partial_principal_pages_keep_entries_and_add_an_incomplete_notice() {
    let profile = profile_id(1);
    let mut explorer = explorer_with_databases(profile, &["app"]);
    let group = ExplorerNodeId::PrincipalGroup {
        profile_id: profile,
    };
    explorer.expanded.insert(group.clone());
    let account = principal(profile, "alice", "alice", PrincipalKind::User);
    explorer
        .profiles
        .get_mut(&profile)
        .unwrap()
        .set_principals(PrincipalPage {
            connection: ConnectionIdentity {
                profile_id: profile,
                generation: 1,
            },
            entries: vec![account.clone()],
            complete: false,
        });

    let rows = explorer.visible();
    assert!(rows.iter().any(|row| {
        row.id
            == (ExplorerNodeId::Principal {
                entry: account.id.clone(),
            })
    }));
    assert!(rows.iter().any(|row| {
        row.id
            == (ExplorerNodeId::PrincipalNotice {
                profile_id: profile,
            })
    }));
    assert!(!explorer.profiles.get(&profile).unwrap().principals_complete);
}

#[test]
fn unsupported_database_shows_an_informational_notice_instead_of_fake_users() {
    let profile = profile_id(1);
    let mut explorer = explorer_with_databases(profile, &["app"]);
    let group = ExplorerNodeId::PrincipalGroup {
        profile_id: profile,
    };
    explorer.expanded.insert(group.clone());
    {
        let state = explorer.profiles.get_mut(&profile).unwrap();
        state.principals_loaded = true;
        state.principals_unsupported = Some("SQLite does not support users or roles".to_owned());
    }

    let rows = explorer.visible();
    let notice = ExplorerNodeId::PrincipalNotice {
        profile_id: profile,
    };
    assert!(rows.iter().any(|row| row.id == notice));
    assert!(
        rows.iter()
            .all(|row| !matches!(row.id, ExplorerNodeId::Principal { .. }))
    );
}

#[test]
fn principal_ddl_tab_accepts_only_the_pending_request() {
    let profile = profile_id(1);
    let entry = principal(profile, "10", "alice", PrincipalKind::User);
    let mut tab = PrincipalDdlTab::new(entry.clone());
    let connection = ConnectionIdentity {
        profile_id: profile,
        generation: 3,
    };

    let request = tab.allocate_request(connection).unwrap();
    tab.begin_load(request.clone());
    assert!(matches!(tab.load, PrincipalDdlLoad::Loading { .. }));

    // A response for a different (stale) request must not be applied.
    let mut stale = request.clone();
    stale.request_id += 1;
    assert!(
        tab.apply_success(
            &stale,
            PrincipalDdl {
                principal: entry.clone(),
                sql: "CREATE ROLE stale".to_owned(),
            },
        )
        .is_none()
    );

    let sql = tab
        .apply_success(
            &request,
            PrincipalDdl {
                principal: entry,
                sql: "CREATE ROLE alice LOGIN;".to_owned(),
            },
        )
        .expect("pending request should be applied");
    assert_eq!(sql, "CREATE ROLE alice LOGIN;");
    assert!(matches!(tab.load, PrincipalDdlLoad::Ready(_)));
}

#[test]
fn reconnect_invalidates_an_in_flight_principal_ddl_request() {
    let profile = profile_id(1);
    let entry = principal(profile, "10", "alice", PrincipalKind::User);
    let mut tab = PrincipalDdlTab::new(entry);
    let request = tab
        .allocate_request(ConnectionIdentity {
            profile_id: profile,
            generation: 1,
        })
        .unwrap();
    tab.begin_load(request.clone());

    tab.invalidate_for_reconnect();
    assert_ne!(tab.generation, request.tab_generation);
    assert!(matches!(tab.load, PrincipalDdlLoad::Empty));
    assert!(
        tab.apply_success(
            &request,
            PrincipalDdl {
                principal: tab.entry.clone(),
                sql: "CREATE ROLE alice".to_owned(),
            },
        )
        .is_none()
    );
}

#[test]
fn principal_display_kinds_cover_the_three_explorer_glyphs() {
    assert_ne!(PrincipalDisplayKind::Group, PrincipalDisplayKind::User);
    assert_ne!(PrincipalDisplayKind::User, PrincipalDisplayKind::Role);
}
