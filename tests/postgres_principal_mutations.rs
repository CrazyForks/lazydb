use lazydb::{
    db::{
        DatabaseConnection,
        catalog::{CatalogId, CatalogKind},
        catalog_mutation::{
            CatalogMutationAnchor, CatalogMutationMode, CatalogMutationRequest,
            CatalogObjectDefinitionRequest, CatalogObjectType,
        },
        postgres::PostgresAdapter,
        principal::{
            PrincipalEntry, PrincipalId, PrincipalKind, PrincipalMutation, PrincipalMutationTarget,
            PrincipalReadTarget, PrincipalScope,
        },
        value::CellValue,
    },
    identity::ConnectionIdentity,
    model::{
        catalog_editor::{CatalogDraft, RoleDraft},
        execution_target::ExecutionTarget,
    },
    profile::import_connection_url,
};

#[tokio::test]
async fn postgres_principal_mutation_environment_is_explicit() {
    let Some(url) = std::env::var_os("LAZYDB_TEST_POSTGRES_URL") else {
        if std::env::var_os("LAZYDB_REQUIRE_DATABASE_TESTS").is_some() {
            panic!("LAZYDB_TEST_POSTGRES_URL is required for PostgreSQL principal tests");
        }
        return;
    };
    let imported = import_connection_url(&url.to_string_lossy(), Some("principal-mutations"))
        .expect("test URL should parse");
    let password = imported.transient_password.as_ref();
    let profile = imported.profile.clone();
    let _ = PrincipalKind::User;
    let connection = match DatabaseConnection::connect(&profile, password).await {
        Ok(connection) => connection,
        Err(error) => {
            if std::env::var_os("LAZYDB_REQUIRE_DATABASE_TESTS").is_some() {
                panic!("PostgreSQL principal test connection failed: {error}");
            }
            eprintln!("PostgreSQL principal test skipped: {error}");
            return;
        }
    };
    let page = connection
        .list_principals()
        .await
        .expect("principal listing should work");
    assert!(page.complete);
    let can_create = matches!(
        connection
            .execute("SELECT rolcreaterole FROM pg_roles WHERE rolname = current_user")
            .await
            .ok()
            .and_then(|outcome| outcome.result_sets.last()?.rows.first()?.first().cloned()),
        Some(CellValue::Boolean(true))
    );
    if !can_create {
        eprintln!("PostgreSQL principal mutation test skipped: current role lacks CREATEROLE");
        connection.close().await;
        return;
    }
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let role_name = format!("lazydb_principal_{suffix}");
    connection
        .execute(&format!("CREATE ROLE \"{role_name}\" NOLOGIN"))
        .await
        .expect("create role");
    let profile_id = profile.id;
    let identity = ConnectionIdentity {
        profile_id,
        generation: 1,
    };
    let entry = PrincipalEntry {
        id: PrincipalId {
            profile_id,
            scope: PrincipalScope::Cluster,
            native_id: "0".into(),
            host: None,
        },
        kind: PrincipalKind::Role,
        name: role_name.clone(),
        native_kind: "role".into(),
        system: false,
    };
    let oid = connection
        .execute(&format!(
            "SELECT oid::text FROM pg_roles WHERE rolname = '{role_name}'"
        ))
        .await
        .expect("role oid");
    let oid = match &oid.result_sets.last().unwrap().rows[0][0] {
        CellValue::Text(value) => value.clone(),
        value => panic!("unexpected oid: {value:?}"),
    };
    let entry = PrincipalEntry {
        id: PrincipalId {
            native_id: oid.clone(),
            ..entry.id
        },
        ..entry
    };
    let object = CatalogId::new(
        profile_id,
        CatalogKind::Database,
        ["__role__", role_name.as_str()],
    );
    let request = CatalogObjectDefinitionRequest {
        connection: identity,
        request_id: 1,
        catalog_epoch: 1,
        object: object.clone(),
        target: ExecutionTarget {
            profile_id,
            database: imported
                .profile
                .database
                .clone()
                .unwrap_or_else(|| "postgres".into()),
            schema: None,
        },
        principal: Some(entry.clone()),
    };
    let definition = connection
        .load_catalog_object_definition(&request)
        .await
        .expect("load role definition");
    let baseline = Some(definition.clone());
    let mut draft = RoleDraft::from_definition(match &definition {
        lazydb::db::catalog_mutation::CatalogObjectDefinition::Role(role) => role,
        _ => panic!("expected role"),
    });
    draft.login = true;
    draft.set_password("lazydb-test-password");
    draft.name = format!("{role_name}_renamed").into();
    let mutation = CatalogMutationRequest::new(
        identity,
        2,
        1,
        CatalogMutationMode::Edit,
        CatalogMutationAnchor::Principal(entry),
        CatalogObjectType::Role,
    )
    .unwrap()
    .with_current_database(profile.database.unwrap_or_else(|| "postgres".into()));
    let plan =
        PostgresAdapter::plan_catalog_mutation(mutation, CatalogDraft::Role(draft), baseline)
            .expect("plan role edit");
    assert!(plan.sql().contains("LOGIN"));
    assert!(plan.sql().contains("PASSWORD '<REDACTED>'"));
    assert!(!plan.sql().contains("lazydb-test-password"));
    for statement in plan.statements() {
        connection
            .execute(statement)
            .await
            .expect("apply role edit statement");
    }
    let renamed = format!("{role_name}_renamed");
    let renamed_oid = connection
        .execute(&format!(
            "SELECT oid::text FROM pg_roles WHERE rolname = '{renamed}'"
        ))
        .await
        .expect("renamed oid");
    assert_eq!(
        renamed_oid.result_sets.last().unwrap().rows[0][0],
        CellValue::Text(oid)
    );
    connection
        .execute(&format!("DROP ROLE IF EXISTS \"{renamed}\""))
        .await
        .expect("cleanup renamed role");
    connection.close().await;
}

#[tokio::test]
async fn postgres_principal_grant_read_revoke_round_trip_is_explicit() {
    let Some(url) = std::env::var_os("LAZYDB_TEST_POSTGRES_URL") else {
        if std::env::var_os("LAZYDB_REQUIRE_DATABASE_TESTS").is_some() {
            panic!("LAZYDB_TEST_POSTGRES_URL is required for PostgreSQL principal tests");
        }
        return;
    };
    let imported = import_connection_url(&url.to_string_lossy(), Some("principal-round-trip"))
        .expect("PostgreSQL URL should parse");
    let password = imported.transient_password.as_ref();
    let profile = imported.profile.clone();
    let connection = match DatabaseConnection::connect(&profile, password).await {
        Ok(connection) => connection,
        Err(error) => {
            if std::env::var_os("LAZYDB_REQUIRE_DATABASE_TESTS").is_some() {
                panic!("PostgreSQL grant round-trip connection failed: {error}");
            }
            eprintln!("PostgreSQL grant round-trip skipped: {error}");
            return;
        }
    };
    let can_create = matches!(
        connection
            .execute("SELECT rolcreaterole FROM pg_roles WHERE rolname = current_user")
            .await
            .ok()
            .and_then(|outcome| outcome.result_sets.last()?.rows.first()?.first().cloned()),
        Some(CellValue::Boolean(true))
    );
    if !can_create {
        if std::env::var_os("LAZYDB_REQUIRE_DATABASE_TESTS").is_some() {
            panic!("PostgreSQL grant round-trip requires CREATEROLE");
        }
        eprintln!("PostgreSQL grant round-trip skipped: current role lacks CREATEROLE");
        connection.close().await;
        return;
    }
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let role = format!("lazydb_acl_{suffix}");
    let table = format!("lazydb_acl_table_{suffix}");
    connection
        .execute(&format!("CREATE ROLE \"{role}\" NOLOGIN"))
        .await
        .expect("create role");
    connection
        .execute(&format!("CREATE TABLE \"{table}\" (id integer)"))
        .await
        .expect("create table");
    let page = connection.list_principals().await.expect("list principals");
    let principal = page
        .entries
        .into_iter()
        .find(|entry| entry.name == role)
        .expect("created role should be listed");
    let database = profile
        .database
        .clone()
        .unwrap_or_else(|| "postgres".into());
    let connection_id = ConnectionIdentity {
        profile_id: profile.id,
        generation: 1,
    };
    let plan = connection
        .plan_principal_mutation(
            &principal,
            connection_id,
            Some(&database),
            PrincipalMutation::Grant {
                target: PrincipalMutationTarget::Relation {
                    schema: "public".into(),
                    relation: table.clone(),
                },
                privilege: "SELECT".into(),
                grant_option: false,
            },
        )
        .expect("grant plan");
    connection.execute(&plan.sql).await.expect("grant");
    let details = connection
        .principal_details(
            &principal,
            &PrincipalReadTarget {
                principal: principal.id.clone(),
                database: Some(database.clone()),
            },
        )
        .await
        .expect("read granted permission");
    assert!(details.permissions.iter().any(|permission| {
        permission.target == format!("public.{table}") && permission.privilege == "SELECT"
    }));
    let revoke = connection
        .plan_principal_mutation(
            &principal,
            connection_id,
            Some(&database),
            PrincipalMutation::Revoke {
                target: PrincipalMutationTarget::Relation {
                    schema: "public".into(),
                    relation: table.clone(),
                },
                privilege: "SELECT".into(),
                grant_option: false,
            },
        )
        .expect("revoke plan");
    connection.execute(&revoke.sql).await.expect("revoke");
    let after = connection
        .principal_details(
            &principal,
            &PrincipalReadTarget {
                principal: principal.id.clone(),
                database: Some(database),
            },
        )
        .await
        .expect("read revoked permission");
    assert!(!after.permissions.iter().any(|permission| {
        permission.target == format!("public.{table}")
            && permission.privilege == "SELECT"
            && permission.source == "direct"
    }));
    connection
        .execute(&format!("DROP TABLE IF EXISTS \"{table}\""))
        .await
        .expect("drop table");
    connection
        .execute(&format!("DROP ROLE IF EXISTS \"{role}\""))
        .await
        .expect("drop role");
    connection.close().await;
}
