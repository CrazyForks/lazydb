use crate::profile::DatabaseKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabaseDescriptor {
    pub kind: DatabaseKind,
    pub name: &'static str,
}

pub const DRIVERS: [DatabaseDescriptor; 6] = [
    DatabaseDescriptor {
        kind: DatabaseKind::Postgres,
        name: "postgres",
    },
    DatabaseDescriptor {
        kind: DatabaseKind::MySql,
        name: "mysql",
    },
    DatabaseDescriptor {
        kind: DatabaseKind::MariaDb,
        name: "mariadb",
    },
    DatabaseDescriptor {
        kind: DatabaseKind::Oracle,
        name: "oracle",
    },
    DatabaseDescriptor {
        kind: DatabaseKind::SqlServer,
        name: "sqlserver",
    },
    DatabaseDescriptor {
        kind: DatabaseKind::Sqlite,
        name: "sqlite",
    },
];

pub const DRIVER_NAMES: [&str; DRIVERS.len()] = [
    DRIVERS[0].name,
    DRIVERS[1].name,
    DRIVERS[2].name,
    DRIVERS[3].name,
    DRIVERS[4].name,
    DRIVERS[5].name,
];

pub const DRIVER_LIST: &str = "postgres,mysql,mariadb,oracle,sqlserver,sqlite";

pub fn kinds() -> impl Iterator<Item = DatabaseKind> {
    DRIVERS.iter().map(|descriptor| descriptor.kind)
}
