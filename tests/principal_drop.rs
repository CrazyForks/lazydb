use lazydb::{
    db::{
        principal::PrincipalKind,
        principal_drop::{PrincipalDropError, PrincipalDropPlan, PrincipalDropRequest},
    },
    identity::ConnectionIdentity,
};
use uuid::Uuid;

fn request(name: &str, kind: PrincipalKind) -> PrincipalDropRequest {
    let profile_id = Uuid::from_u128(1);
    PrincipalDropRequest {
        connection: ConnectionIdentity {
            profile_id,
            generation: 1,
        },
        request_id: 1,
        entry: lazydb::db::principal::PrincipalEntry {
            id: lazydb::db::principal::PrincipalId {
                profile_id,
                scope: lazydb::db::principal::PrincipalScope::Cluster,
                native_id: "42".into(),
                host: None,
            },
            kind,
            name: name.into(),
            native_kind: "role".into(),
            system: false,
        },
    }
}

#[test]
fn principal_drop_plan_quotes_user_and_role_names() {
    let user = request("user\"one", PrincipalKind::User);
    let plan = PrincipalDropPlan::new(user, "DROP ROLE \"user\"\"one\"").unwrap();
    assert_eq!(plan.sql(), "DROP ROLE \"user\"\"one\"");
}

#[test]
fn principal_drop_rejects_cross_profile_and_non_drop_sql() {
    let mut mismatched = request("alice", PrincipalKind::Role);
    mismatched.connection.profile_id = Uuid::from_u128(2);
    assert_eq!(
        mismatched.validate(),
        Err(PrincipalDropError::ProfileMismatch)
    );

    let request = request("alice", PrincipalKind::Role);
    assert_eq!(
        PrincipalDropPlan::new(request, "ALTER ROLE \"alice\" LOGIN"),
        Err(PrincipalDropError::InvalidSql)
    );
}

#[test]
fn principal_drop_requires_a_request_id() {
    let mut request = request("alice", PrincipalKind::Role);
    request.request_id = 0;
    assert_eq!(request.validate(), Err(PrincipalDropError::InvalidSql));
}
