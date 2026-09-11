#[cfg(not(feature = "driver-oracle"))]
fn main() {
    eprintln!("rebuild with --features driver-oracle");
}

#[cfg(feature = "driver-oracle")]
use oracle::Connection;

#[cfg(feature = "driver-oracle")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connect_string = std::env::var("LAZYDB_TEST_ORACLE_CONNECT")?;
    let user = std::env::var("LAZYDB_TEST_ORACLE_USER")?;
    let password = std::env::var("LAZYDB_TEST_ORACLE_PASSWORD")?;
    let connection = Connection::connect(user, password, connect_string)?;
    let row = connection.query_row(
        "SELECT SYS_CONTEXT('USERENV', 'SESSION_USER'), \
                SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA'), \
                SYS_CONTEXT('USERENV', 'SERVICE_NAME') FROM dual",
        &[],
    )?;
    let session_user: String = row.get(0)?;
    let current_schema: String = row.get(1)?;
    let service_name: String = row.get(2)?;
    println!("authenticated");
    println!("session_user={session_user}");
    println!("current_schema={current_schema}");
    println!("service_name={service_name}");
    Ok(())
}
