# MariaDB 测试数据库

仓库提供了一套可重复启动的 MariaDB 测试实例，供 LazyDB 的连接、目录浏览、SQL 编辑器、数据预览和类型展示功能使用。初始化脚本包含表、视图、索引、外键、触发器、存储过程和函数，并尽可能覆盖 MariaDB 常见数据类型。

## 前置条件

- 已安装并启动 Docker Desktop 或 Docker Engine
- Docker Compose v2（使用 `docker compose` 命令）

## 启动

在仓库根目录执行：

```bash
docker compose -f docker-compose.mariadb.yml up -d
docker compose -f docker-compose.mariadb.yml ps
```

第一次启动会自动执行 `docker/mariadb/init/001-test-fixture.sql`，创建数据库 `lazydb_test` 和测试数据。MariaDB 健康状态变为 `healthy` 后即可连接；首次初始化通常需要几十秒。

默认连接信息：

| 字段 | 值 |
| --- | --- |
| Host | `127.0.0.1` |
| Port | `3307` |
| Database | `lazydb_test` |
| User | `lazydb` |
| Password | `lazydb_password` |

LazyDB 也可以使用 URL：

```text
mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test
```

如需修改密码、用户、时区或宿主机端口，可在仓库根目录创建 `.env`：

```dotenv
MARIADB_ROOT_PASSWORD=change-me-root
MARIADB_PASSWORD=change-me
MARIADB_USER=lazydb
MARIADB_HOST_PORT=3307
TZ=Asia/Shanghai
```

注意：环境变量只会在**首次创建数据卷**时用于初始化账号。修改密码后若已有数据卷，需要手动执行 SQL 修改账号，或按下方说明删除测试数据卷重新初始化。

## 数据覆盖范围

- 数值：`TINYINT`、`SMALLINT`、`MEDIUMINT`、`INT`、`BIGINT`、`DECIMAL`、`NUMERIC`、`FLOAT`、`DOUBLE`、`BIT`、`BOOLEAN`
- 日期时间：`DATE`、`TIME(6)`、`DATETIME(6)`、`TIMESTAMP(6)`、`YEAR`
- 字符与二进制：`CHAR`、`VARCHAR`、`BINARY`、`VARBINARY`、`TINYTEXT`、`TEXT`、`MEDIUMTEXT`、`LONGTEXT`、`TINYBLOB`、`BLOB`、`MEDIUMBLOB`、`LONGBLOB`
- 特殊类型：`JSON`、`ENUM`、`SET`、`POINT`、`GEOMETRY`、`LINESTRING`、`POLYGON`、`MULTIPOINT`、`MULTILINESTRING`、`MULTIPOLYGON`、`GEOMETRYCOLLECTION`
- 行为和结构：`NULL`、默认值、自动递增、生成列、主键、唯一索引、普通索引、空间索引、外键、视图、触发器、存储过程、函数

主要对象：

- `mariadb_test_types`：集中展示各种数据类型，并包含 Unicode、空值、边界值、JSON 和空间数据
- `mariadb_test_users`、`mariadb_test_orders`、`mariadb_test_order_items`：多表关系、外键、生成列和聚合预览
- `mariadb_test_nullable`：显式测试多种可空列
- `mariadb_test_spatial`：覆盖 MariaDB 的主要空间类型
- `mariadb_test_order_summary`：订单汇总视图
- `mariadb_test_orders_for_user`、`mariadb_test_with_tax`：可编程对象

## 快速验证

使用容器内的客户端执行检查：

```bash
docker compose -f docker-compose.mariadb.yml exec mariadb \
  mariadb -ulazydb -plazydb_password lazydb_test \
  -e 'SHOW TABLES; SELECT COUNT(*) AS users FROM mariadb_test_users; SELECT * FROM mariadb_test_order_summary;'
```

查看列类型和生成列：

```bash
docker compose -f docker-compose.mariadb.yml exec mariadb \
  mariadb -ulazydb -plazydb_password lazydb_test \
  -e 'SELECT table_name, column_name, data_type, column_type, is_nullable, extra FROM information_schema.columns WHERE table_schema = "lazydb_test" ORDER BY table_name, ordinal_position;'
```

调用存储过程和函数：

```bash
docker compose -f docker-compose.mariadb.yml exec mariadb \
  mariadb -ulazydb -plazydb_password lazydb_test \
  -e 'CALL mariadb_test_orders_for_user(1); SELECT mariadb_test_with_tax(100.00, 13.00) AS total;'
```

## 停止与重置

停止容器但保留数据：

```bash
docker compose -f docker-compose.mariadb.yml down
```

删除容器和数据卷，使初始化脚本下次重新执行（会删除该测试实例中的全部数据）：

```bash
docker compose -f docker-compose.mariadb.yml down -v
docker compose -f docker-compose.mariadb.yml up -d
```

不要将此配置用于生产环境；默认密码仅适用于本地测试。
