use lazydb::db::mysql::MySqlAdapter;

#[test]
fn mariadb_monitoring_contract_is_bounded_and_uses_stable_status_queries() {
    assert!(MySqlAdapter::MONITOR_STATUS_SQL.contains("SHOW GLOBAL STATUS"));
    assert!(MySqlAdapter::MONITOR_STATUS_SQL.contains("Threads_running"));
    assert_eq!(MySqlAdapter::PROCESS_LIST_SQL, "SHOW FULL PROCESSLIST");
}

#[test]
fn mariadb_catalog_search_has_a_server_side_result_bound() {
    assert!(lazydb::db::mysql::CATALOG_SEARCH_CANDIDATES_SQL.contains("LIMIT 101"));
}
