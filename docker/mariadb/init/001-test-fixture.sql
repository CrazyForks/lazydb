-- LazyDB MariaDB test fixture.
-- This file is executed automatically only when the named volume is empty.

USE lazydb_test;

SET NAMES utf8mb4;
SET time_zone = '+00:00';

CREATE TABLE IF NOT EXISTS mariadb_test_types (
    id BIGINT UNSIGNED NOT NULL,
    tinyint_signed TINYINT NOT NULL,
    tinyint_unsigned TINYINT UNSIGNED NOT NULL,
    smallint_value SMALLINT NOT NULL,
    mediumint_value MEDIUMINT NOT NULL,
    int_value INT NOT NULL,
    bigint_value BIGINT NOT NULL,
    decimal_value DECIMAL(20, 6) NOT NULL,
    numeric_value NUMERIC(12, 3) NOT NULL,
    float_value FLOAT NOT NULL,
    double_value DOUBLE NOT NULL,
    bit_value BIT(8) NOT NULL,
    boolean_value BOOLEAN NOT NULL,
    date_value DATE NOT NULL,
    time_value TIME(6) NOT NULL,
    datetime_value DATETIME(6) NOT NULL,
    timestamp_value TIMESTAMP(6) NOT NULL,
    year_value YEAR NOT NULL,
    char_value CHAR(10) NOT NULL,
    varchar_value VARCHAR(255) NOT NULL,
    binary_value BINARY(4) NOT NULL,
    varbinary_value VARBINARY(255) NOT NULL,
    tinytext_value TINYTEXT NOT NULL,
    text_value TEXT NOT NULL,
    mediumtext_value MEDIUMTEXT NOT NULL,
    longtext_value LONGTEXT NOT NULL,
    tinyblob_value TINYBLOB NOT NULL,
    blob_value BLOB NOT NULL,
    mediumblob_value MEDIUMBLOB NOT NULL,
    longblob_value LONGBLOB NOT NULL,
    json_value JSON NOT NULL,
    enum_value ENUM('draft', 'published', 'archived') NOT NULL,
    set_value SET('red', 'green', 'blue') NOT NULL,
    location POINT NOT NULL,
    nullable_value VARCHAR(100) NULL,
    generated_length INT AS (CHAR_LENGTH(varchar_value)) STORED,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (id),
    KEY idx_types_date (date_value),
    SPATIAL KEY idx_types_location (location),
    CONSTRAINT chk_types_json CHECK (JSON_VALID(json_value))
) ENGINE = InnoDB;

CREATE TABLE IF NOT EXISTS mariadb_test_users (
    id INT UNSIGNED NOT NULL AUTO_INCREMENT,
    public_id CHAR(36) NOT NULL,
    username VARCHAR(64) NOT NULL,
    email VARCHAR(255) NOT NULL,
    status ENUM('active', 'inactive', 'locked') NOT NULL DEFAULT 'active',
    roles SET('reader', 'writer', 'admin') NOT NULL DEFAULT 'reader',
    preferences JSON NULL,
    birth_date DATE NULL,
    last_login DATETIME(6) NULL,
    avatar BLOB NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uk_users_public_id (public_id),
    UNIQUE KEY uk_users_email (email),
    KEY idx_users_status (status)
) ENGINE = InnoDB;

CREATE TABLE IF NOT EXISTS mariadb_test_orders (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    user_id INT UNSIGNED NOT NULL,
    order_number VARCHAR(32) NOT NULL,
    amount DECIMAL(12, 2) NOT NULL,
    tax_rate DECIMAL(5, 2) NOT NULL DEFAULT 0.00,
    total_amount DECIMAL(14, 2) AS (ROUND(amount * (1 + tax_rate / 100), 2)) STORED,
    status ENUM('pending', 'paid', 'cancelled', 'refunded') NOT NULL,
    tags SET('priority', 'gift', 'international') NULL,
    metadata JSON NOT NULL,
    shipping_location POINT NULL,
    ordered_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    PRIMARY KEY (id),
    UNIQUE KEY uk_orders_number (order_number),
    KEY idx_orders_user_status (user_id, status),
    CONSTRAINT fk_orders_user FOREIGN KEY (user_id) REFERENCES mariadb_test_users (id)
) ENGINE = InnoDB;

CREATE TABLE IF NOT EXISTS mariadb_test_order_items (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    order_id BIGINT UNSIGNED NOT NULL,
    sku VARCHAR(40) NOT NULL,
    quantity SMALLINT UNSIGNED NOT NULL,
    unit_price DECIMAL(12, 2) NOT NULL,
    line_total DECIMAL(14, 2) AS (quantity * unit_price) STORED,
    description TEXT NULL,
    PRIMARY KEY (id),
    KEY idx_items_order (order_id),
    CONSTRAINT fk_items_order FOREIGN KEY (order_id) REFERENCES mariadb_test_orders (id)
) ENGINE = InnoDB;

CREATE TABLE IF NOT EXISTS mariadb_test_nullable (
    id INT UNSIGNED NOT NULL AUTO_INCREMENT,
    required_value VARCHAR(100) NOT NULL,
    nullable_text VARCHAR(100) NULL,
    nullable_number DECIMAL(10, 3) NULL,
    nullable_date DATE NULL,
    nullable_json JSON NULL,
    PRIMARY KEY (id)
) ENGINE = InnoDB;

CREATE TABLE IF NOT EXISTS mariadb_test_spatial (
    id INT UNSIGNED NOT NULL AUTO_INCREMENT,
    geometry_value GEOMETRY NOT NULL,
    linestring_value LINESTRING NULL,
    polygon_value POLYGON NULL,
    multipoint_value MULTIPOINT NULL,
    multilinestring_value MULTILINESTRING NULL,
    multipolygon_value MULTIPOLYGON NULL,
    geometrycollection_value GEOMETRYCOLLECTION NULL,
    PRIMARY KEY (id)
) ENGINE = InnoDB;

INSERT IGNORE INTO mariadb_test_types (
    id, tinyint_signed, tinyint_unsigned, smallint_value, mediumint_value,
    int_value, bigint_value, decimal_value, numeric_value, float_value,
    double_value, bit_value, boolean_value, date_value, time_value,
    datetime_value, timestamp_value, year_value, char_value, varchar_value,
    binary_value, varbinary_value, tinytext_value, text_value, mediumtext_value,
    longtext_value, tinyblob_value, blob_value, mediumblob_value, longblob_value,
    json_value, enum_value, set_value, location, nullable_value
) VALUES
(
    1, -7, 250, -32000, 8388607, 2147483647, -9223372036854775807,
    1234567890123.123456, -9876543.210, 3.14159, 2.718281828,
    b'10100101', TRUE, '2024-02-29', '23:59:59.123456',
    '2024-02-29 23:59:59.123456', '2024-02-29 23:59:59.123456', 2024,
    'fixed', 'Hello, MariaDB! 你好，LazyDB！', X'00FF10A5', X'CAFE0102',
    'tiny text', 'A longer TEXT value with line breaks\nand punctuation.',
    REPEAT('medium text ', 20),
    'LONGTEXT keeps large textual documents available for preview testing.',
    X'00FF', X'000102030405', REPEAT(X'AB', 64), REPEAT(X'CD', 128),
    '{"kind":"types","active":true,"values":[1,2,3],"nested":{"语言":"中文"}}',
    'published', 'red,green', ST_GeomFromText('POINT(116.391 39.907)'), NULL
),
(
    2, 0, 0, 0, -8388608, -2147483648, 9223372036854775807,
    -0.000001, 0.001, -1.25, -0.0000001,
    b'00000000', FALSE, '1970-01-01', '00:00:00.000000',
    '1970-01-01 00:00:00.000000', '1970-01-01 00:00:00.000000', 1970,
    'zero', '', X'00000000', X'', '', '', '', '', X'', X'', X'', X'',
    '{"kind":"boundary","active":false,"empty":""}',
    'draft', 'blue', ST_GeomFromText('POINT(0 0)'), 'present'
);

INSERT IGNORE INTO mariadb_test_users
    (id, public_id, username, email, status, roles, preferences, birth_date, last_login, avatar)
VALUES
    (1, '11111111-1111-4111-8111-111111111111', 'alice', 'alice@example.test',
     'active', 'reader,writer', '{"theme":"dark","page_size":50}', '1990-05-17',
     '2026-09-17 08:30:00.123456', X'89504E47'),
    (2, '22222222-2222-4222-8222-222222222222', 'bob', 'bob@example.test',
     'inactive', 'reader', NULL, NULL, NULL, NULL),
    (3, '33333333-3333-4333-8333-333333333333', '管理员', 'admin@example.test',
     'active', 'reader,writer,admin', '{"language":"zh-CN","notifications":true}',
     '1985-01-01', '2026-09-16 23:59:59.999999', X'FFD8FFE0');

INSERT IGNORE INTO mariadb_test_orders
    (id, user_id, order_number, amount, tax_rate, status, tags, metadata, shipping_location)
VALUES
    (1001, 1, 'ORD-20260917-001', 199.99, 13.00, 'paid', 'priority,gift',
     '{"currency":"CNY","payment":{"method":"card","paid":true}}',
     ST_GeomFromText('POINT(116.397 39.908)')),
    (1002, 2, 'ORD-20260917-002', 0.00, 0.00, 'pending', NULL,
     '{"currency":"USD","coupon":null}', NULL),
    (1003, 3, 'ORD-20260917-003', 42.50, 6.00, 'refunded', 'international',
     '{"currency":"EUR","refund_reason":"changed mind"}',
     ST_GeomFromText('POINT(2.352 48.857)'));

INSERT IGNORE INTO mariadb_test_order_items
    (id, order_id, sku, quantity, unit_price, description)
VALUES
    (1, 1001, 'KB-ANSI-001', 1, 149.99, 'Mechanical keyboard'),
    (2, 1001, 'CABLE-USB-C', 2, 25.00, 'USB-C cable × 2'),
    (3, 1002, 'STICKER-001', 0, 1.00, 'Zero quantity for boundary testing'),
    (4, 1003, 'BOOK-SQL-101', 1, 42.50, NULL);

INSERT IGNORE INTO mariadb_test_nullable
    (id, required_value, nullable_text, nullable_number, nullable_date, nullable_json)
VALUES
    (1, 'all values', 'not null', 123.456, '2026-09-17', '{"present":true}'),
    (2, 'only required', NULL, NULL, NULL, NULL),
    (3, '', '', 0.000, '0001-01-01', '{}');

INSERT IGNORE INTO mariadb_test_spatial
    (id, geometry_value, linestring_value, polygon_value, multipoint_value,
     multilinestring_value, multipolygon_value, geometrycollection_value)
VALUES
    (1,
     ST_GeomFromText('GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(0 0,1 1))'),
     ST_GeomFromText('LINESTRING(0 0,1 1,2 1)'),
     ST_GeomFromText('POLYGON((0 0,0 1,1 1,1 0,0 0))'),
     ST_GeomFromText('MULTIPOINT((0 0),(1 1))'),
     ST_GeomFromText('MULTILINESTRING((0 0,1 1),(2 2,3 3))'),
     ST_GeomFromText('MULTIPOLYGON(((0 0,0 1,1 1,1 0,0 0)))'),
     ST_GeomFromText('GEOMETRYCOLLECTION(POINT(1 1),LINESTRING(2 2,3 3))'));

CREATE OR REPLACE VIEW mariadb_test_order_summary AS
SELECT
    o.id AS order_id,
    o.order_number,
    u.username,
    u.email,
    o.status,
    o.amount,
    o.tax_rate,
    o.total_amount,
    COUNT(i.id) AS item_count,
    COALESCE(SUM(i.line_total), 0.00) AS item_total,
    o.ordered_at
FROM mariadb_test_orders AS o
JOIN mariadb_test_users AS u ON u.id = o.user_id
LEFT JOIN mariadb_test_order_items AS i ON i.order_id = o.id
GROUP BY o.id, o.order_number, u.username, u.email, o.status,
         o.amount, o.tax_rate, o.total_amount, o.ordered_at;

DROP TRIGGER IF EXISTS mariadb_test_types_before_update;
DELIMITER //
CREATE TRIGGER mariadb_test_types_before_update
BEFORE UPDATE ON mariadb_test_types
FOR EACH ROW
BEGIN
    SET NEW.updated_at = CURRENT_TIMESTAMP;
END//
DELIMITER ;

DROP PROCEDURE IF EXISTS mariadb_test_orders_for_user;
DELIMITER //
CREATE PROCEDURE mariadb_test_orders_for_user(IN p_user_id INT UNSIGNED)
SQL SECURITY INVOKER
BEGIN
    SELECT *
    FROM mariadb_test_order_summary
    WHERE username = (SELECT username FROM mariadb_test_users WHERE id = p_user_id)
    ORDER BY ordered_at, order_id;
END//
DELIMITER ;

DROP FUNCTION IF EXISTS mariadb_test_with_tax;
DELIMITER //
CREATE FUNCTION mariadb_test_with_tax(p_amount DECIMAL(12, 2), p_tax_rate DECIMAL(5, 2))
RETURNS DECIMAL(14, 2)
DETERMINISTIC
NO SQL
RETURN ROUND(p_amount * (1 + p_tax_rate / 100), 2)//
DELIMITER ;
