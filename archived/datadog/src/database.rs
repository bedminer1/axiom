use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value as JsonValue, json};
use sqlx::{
    Column, Row as SqlxRow, Sqlite, SqlitePool, Transaction as SqlxTransaction, TypeInfo, ValueRef,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
};
use std::{
    collections::HashMap,
    env,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

type DbFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

#[derive(Clone, Debug)]
pub enum DatabaseConfig {
    Local { path: PathBuf },
    Turso { url: String, auth_token: String },
}

impl DatabaseConfig {
    pub fn from_env(data_dir: &Path) -> Result<Self> {
        match env::var("TURSO_DATABASE_URL") {
            Ok(url) if !url.trim().is_empty() => {
                let auth_token = env::var("TURSO_AUTH_TOKEN")
                    .context("TURSO_AUTH_TOKEN is required when TURSO_DATABASE_URL is set")?;
                ensure!(
                    !auth_token.trim().is_empty(),
                    "TURSO_AUTH_TOKEN cannot be empty"
                );
                Ok(Self::Turso { url, auth_token })
            }
            _ => Ok(Self::Local {
                path: data_dir.join("telebot.sqlite"),
            }),
        }
    }
}

/// Backend-neutral persistent database handle. Application code only depends on
/// this type and the query helpers below; Turso and SQLx stay inside adapters.
#[derive(Clone)]
pub struct Database {
    inner: Arc<dyn PersistentDatabase>,
}

pub trait PersistentDatabase: Send + Sync {
    fn execute<'a>(&'a self, statement: Statement) -> DbFuture<'a, QueryResult>;
    fn query<'a>(&'a self, statement: Statement) -> DbFuture<'a, Vec<DbRow>>;
    fn begin<'a>(&'a self) -> DbFuture<'a, Box<dyn PersistentTransaction>>;
    fn migrate<'a>(&'a self) -> DbFuture<'a, ()>;
    fn backend_name(&self) -> &'static str;
}

pub trait PersistentTransaction: Send {
    fn execute<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, QueryResult>;
    fn query<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, Vec<DbRow>>;
    fn commit<'a>(&'a mut self) -> DbFuture<'a, ()>;
    fn rollback<'a>(&'a mut self) -> DbFuture<'a, ()>;
}

impl Database {
    pub async fn connect(config: DatabaseConfig) -> Result<Self> {
        let inner: Arc<dyn PersistentDatabase> = match config {
            DatabaseConfig::Local { path } => Arc::new(SqliteDatabase::connect(&path).await?),
            DatabaseConfig::Turso { url, auth_token } => {
                Arc::new(TursoDatabase::connect(url, auth_token)?)
            }
        };
        let database = Self { inner };
        database.inner.migrate().await?;
        Ok(database)
    }

    pub async fn begin(&self) -> Result<Transaction> {
        Ok(Transaction {
            inner: Some(self.inner.begin().await?),
        })
    }

    pub fn backend_name(&self) -> &'static str {
        self.inner.backend_name()
    }
}

pub struct Transaction {
    inner: Option<Box<dyn PersistentTransaction>>,
}

impl Transaction {
    pub async fn commit(mut self) -> Result<()> {
        let mut inner = self.inner.take().context("transaction already closed")?;
        inner.commit().await
    }

    #[allow(dead_code)]
    pub async fn rollback(mut self) -> Result<()> {
        let mut inner = self.inner.take().context("transaction already closed")?;
        inner.rollback().await
    }
}

#[derive(Clone, Debug)]
pub struct Statement {
    sql: String,
    args: Vec<DbValue>,
}

#[derive(Clone, Debug)]
pub enum DbValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

pub trait IntoDbValue {
    fn into_db_value(self) -> DbValue;
}

impl IntoDbValue for DbValue {
    fn into_db_value(self) -> DbValue {
        self
    }
}
impl IntoDbValue for i64 {
    fn into_db_value(self) -> DbValue {
        DbValue::Integer(self)
    }
}
impl IntoDbValue for i32 {
    fn into_db_value(self) -> DbValue {
        DbValue::Integer(i64::from(self))
    }
}
impl IntoDbValue for u32 {
    fn into_db_value(self) -> DbValue {
        DbValue::Integer(i64::from(self))
    }
}
impl IntoDbValue for bool {
    fn into_db_value(self) -> DbValue {
        DbValue::Integer(i64::from(self))
    }
}
impl IntoDbValue for f32 {
    fn into_db_value(self) -> DbValue {
        DbValue::Real(f64::from(self))
    }
}
impl IntoDbValue for f64 {
    fn into_db_value(self) -> DbValue {
        DbValue::Real(self)
    }
}
impl IntoDbValue for String {
    fn into_db_value(self) -> DbValue {
        DbValue::Text(self)
    }
}
impl IntoDbValue for &String {
    fn into_db_value(self) -> DbValue {
        DbValue::Text(self.clone())
    }
}
impl IntoDbValue for &str {
    fn into_db_value(self) -> DbValue {
        DbValue::Text(self.to_owned())
    }
}
impl IntoDbValue for Vec<u8> {
    fn into_db_value(self) -> DbValue {
        DbValue::Blob(self)
    }
}
impl IntoDbValue for &[u8] {
    fn into_db_value(self) -> DbValue {
        DbValue::Blob(self.to_vec())
    }
}
impl<T: IntoDbValue> IntoDbValue for Option<T> {
    fn into_db_value(self) -> DbValue {
        self.map(IntoDbValue::into_db_value)
            .unwrap_or(DbValue::Null)
    }
}
impl IntoDbValue for &Option<String> {
    fn into_db_value(self) -> DbValue {
        self.as_ref()
            .map(|v| DbValue::Text(v.clone()))
            .unwrap_or(DbValue::Null)
    }
}

pub trait FromDbValue: Sized {
    fn from_db_value(value: &DbValue) -> Result<Self>;
}

impl FromDbValue for i64 {
    fn from_db_value(value: &DbValue) -> Result<Self> {
        match value {
            DbValue::Integer(v) => Ok(*v),
            DbValue::Real(v) => Ok(*v as i64),
            DbValue::Text(v) => v.parse().context("database value is not an integer"),
            _ => bail!("database value is not an integer"),
        }
    }
}
impl FromDbValue for i32 {
    fn from_db_value(value: &DbValue) -> Result<Self> {
        Ok(i64::from_db_value(value)?.try_into()?)
    }
}
impl FromDbValue for f64 {
    fn from_db_value(value: &DbValue) -> Result<Self> {
        match value {
            DbValue::Real(v) => Ok(*v),
            DbValue::Integer(v) => Ok(*v as f64),
            DbValue::Text(v) => v.parse().context("database value is not a number"),
            _ => bail!("database value is not a number"),
        }
    }
}
impl FromDbValue for String {
    fn from_db_value(value: &DbValue) -> Result<Self> {
        match value {
            DbValue::Text(v) => Ok(v.clone()),
            DbValue::Integer(v) => Ok(v.to_string()),
            DbValue::Real(v) => Ok(v.to_string()),
            _ => bail!("database value is not text"),
        }
    }
}
impl FromDbValue for Vec<u8> {
    fn from_db_value(value: &DbValue) -> Result<Self> {
        match value {
            DbValue::Blob(v) => Ok(v.clone()),
            _ => bail!("database value is not a blob"),
        }
    }
}
impl<T: FromDbValue> FromDbValue for Option<T> {
    fn from_db_value(value: &DbValue) -> Result<Self> {
        match value {
            DbValue::Null => Ok(None),
            other => Ok(Some(T::from_db_value(other)?)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DbRow {
    values: Vec<DbValue>,
    by_name: HashMap<String, usize>,
}

pub trait ColumnIndex {
    fn index(&self, row: &DbRow) -> Option<usize>;
}
impl ColumnIndex for usize {
    fn index(&self, row: &DbRow) -> Option<usize> {
        (*self < row.values.len()).then_some(*self)
    }
}
impl ColumnIndex for &str {
    fn index(&self, row: &DbRow) -> Option<usize> {
        row.by_name.get(*self).copied()
    }
}
impl ColumnIndex for String {
    fn index(&self, row: &DbRow) -> Option<usize> {
        row.by_name.get(self).copied()
    }
}

impl DbRow {
    pub fn get<T: FromDbValue, I: ColumnIndex>(&self, index: I) -> T {
        self.try_get(index).expect("database column type mismatch")
    }

    pub fn try_get<T: FromDbValue, I: ColumnIndex>(&self, index: I) -> Result<T> {
        let position = index.index(self).context("database column not found")?;
        T::from_db_value(&self.values[position])
    }

    fn new(columns: Vec<String>, values: Vec<DbValue>) -> Self {
        let by_name = columns
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), i))
            .collect();
        Self { values, by_name }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct QueryResult {
    rows_affected: u64,
    last_insert_rowid: i64,
}

impl QueryResult {
    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
    pub fn last_insert_rowid(&self) -> i64 {
        self.last_insert_rowid
    }
}

pub trait FromDbRow: Sized {
    fn from_db_row(row: DbRow) -> Result<Self>;
}

impl<A: FromDbValue, B: FromDbValue> FromDbRow for (A, B) {
    fn from_db_row(row: DbRow) -> Result<Self> {
        Ok((row.try_get(0usize)?, row.try_get(1usize)?))
    }
}
impl<A: FromDbValue, B: FromDbValue, C: FromDbValue> FromDbRow for (A, B, C) {
    fn from_db_row(row: DbRow) -> Result<Self> {
        Ok((
            row.try_get(0usize)?,
            row.try_get(1usize)?,
            row.try_get(2usize)?,
        ))
    }
}

pub trait Executor {
    fn execute<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, QueryResult>;
    fn query<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, Vec<DbRow>>;
}

impl Executor for &Database {
    fn execute<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, QueryResult> {
        self.inner.execute(statement)
    }
    fn query<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, Vec<DbRow>> {
        self.inner.query(statement)
    }
}

impl Executor for &mut Transaction {
    fn execute<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, QueryResult> {
        self.inner
            .as_mut()
            .expect("transaction closed")
            .execute(statement)
    }
    fn query<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, Vec<DbRow>> {
        self.inner
            .as_mut()
            .expect("transaction closed")
            .query(statement)
    }
}

pub struct Query {
    statement: Statement,
}
pub fn query(sql: &str) -> Query {
    Query {
        statement: Statement {
            sql: sql.to_owned(),
            args: Vec::new(),
        },
    }
}
impl Query {
    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.statement.args.push(value.into_db_value());
        self
    }
    pub async fn execute(self, mut executor: impl Executor) -> Result<QueryResult> {
        executor.execute(self.statement).await
    }
    pub async fn fetch_all(self, mut executor: impl Executor) -> Result<Vec<DbRow>> {
        executor.query(self.statement).await
    }
    pub async fn fetch_optional(self, mut executor: impl Executor) -> Result<Option<DbRow>> {
        Ok(executor.query(self.statement).await?.into_iter().next())
    }
    pub async fn fetch_one(self, mut executor: impl Executor) -> Result<DbRow> {
        executor
            .query(self.statement)
            .await?
            .into_iter()
            .next()
            .context("query returned no rows")
    }
}

pub struct QueryAs<T> {
    query: Query,
    marker: std::marker::PhantomData<T>,
}
pub fn query_as<T: FromDbRow>(sql: &str) -> QueryAs<T> {
    QueryAs {
        query: query(sql),
        marker: std::marker::PhantomData,
    }
}
impl<T: FromDbRow> QueryAs<T> {
    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.query = self.query.bind(value);
        self
    }
    pub async fn fetch_all(self, executor: impl Executor) -> Result<Vec<T>> {
        self.query
            .fetch_all(executor)
            .await?
            .into_iter()
            .map(T::from_db_row)
            .collect()
    }
    pub async fn fetch_optional(self, executor: impl Executor) -> Result<Option<T>> {
        self.query
            .fetch_optional(executor)
            .await?
            .map(T::from_db_row)
            .transpose()
    }
    pub async fn fetch_one(self, executor: impl Executor) -> Result<T> {
        T::from_db_row(self.query.fetch_one(executor).await?)
    }
}

pub struct QueryScalar<T> {
    query: Query,
    marker: std::marker::PhantomData<T>,
}
pub fn query_scalar<T: FromDbValue>(sql: &str) -> QueryScalar<T> {
    QueryScalar {
        query: query(sql),
        marker: std::marker::PhantomData,
    }
}
impl<T: FromDbValue> QueryScalar<T> {
    pub fn bind(mut self, value: impl IntoDbValue) -> Self {
        self.query = self.query.bind(value);
        self
    }
    pub async fn fetch_all(self, executor: impl Executor) -> Result<Vec<T>> {
        self.query
            .fetch_all(executor)
            .await?
            .into_iter()
            .map(|row| row.try_get(0usize))
            .collect()
    }
    pub async fn fetch_optional(self, executor: impl Executor) -> Result<Option<T>> {
        self.query
            .fetch_optional(executor)
            .await?
            .map(|row| row.try_get(0usize))
            .transpose()
    }
    pub async fn fetch_one(self, executor: impl Executor) -> Result<T> {
        self.query.fetch_one(executor).await?.try_get(0usize)
    }
}

struct SqliteDatabase {
    pool: SqlitePool,
}
impl SqliteDatabase {
    async fn connect(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }
}

impl PersistentDatabase for SqliteDatabase {
    fn execute<'a>(&'a self, statement: Statement) -> DbFuture<'a, QueryResult> {
        Box::pin(async move { local_execute(&self.pool, statement).await })
    }
    fn query<'a>(&'a self, statement: Statement) -> DbFuture<'a, Vec<DbRow>> {
        Box::pin(async move { local_query(&self.pool, statement).await })
    }
    fn begin<'a>(&'a self) -> DbFuture<'a, Box<dyn PersistentTransaction>> {
        Box::pin(async move {
            Ok(Box::new(SqliteTransaction {
                inner: Some(self.pool.begin().await?),
            }) as Box<dyn PersistentTransaction>)
        })
    }
    fn migrate<'a>(&'a self) -> DbFuture<'a, ()> {
        Box::pin(async move {
            sqlx::migrate!("./migrations").run(&self.pool).await?;
            Ok(())
        })
    }
    fn backend_name(&self) -> &'static str {
        "sqlite"
    }
}

struct SqliteTransaction {
    inner: Option<SqlxTransaction<'static, Sqlite>>,
}
impl PersistentTransaction for SqliteTransaction {
    fn execute<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, QueryResult> {
        Box::pin(async move {
            local_execute(
                self.inner.as_mut().context("transaction closed")?.as_mut(),
                statement,
            )
            .await
        })
    }
    fn query<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, Vec<DbRow>> {
        Box::pin(async move {
            local_query(
                self.inner.as_mut().context("transaction closed")?.as_mut(),
                statement,
            )
            .await
        })
    }
    fn commit<'a>(&'a mut self) -> DbFuture<'a, ()> {
        Box::pin(async move {
            self.inner
                .take()
                .context("transaction closed")?
                .commit()
                .await?;
            Ok(())
        })
    }
    fn rollback<'a>(&'a mut self) -> DbFuture<'a, ()> {
        Box::pin(async move {
            self.inner
                .take()
                .context("transaction closed")?
                .rollback()
                .await?;
            Ok(())
        })
    }
}

fn bind_sqlite<'q>(
    mut query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    args: Vec<DbValue>,
) -> sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for value in args {
        query = match value {
            DbValue::Null => query.bind(Option::<String>::None),
            DbValue::Integer(v) => query.bind(v),
            DbValue::Real(v) => query.bind(v),
            DbValue::Text(v) => query.bind(v),
            DbValue::Blob(v) => query.bind(v),
        };
    }
    query
}

async fn local_execute<'e, E>(executor: E, statement: Statement) -> Result<QueryResult>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let result = bind_sqlite(sqlx::query(&statement.sql), statement.args)
        .execute(executor)
        .await?;
    Ok(QueryResult {
        rows_affected: result.rows_affected(),
        last_insert_rowid: result.last_insert_rowid(),
    })
}

async fn local_query<'e, E>(executor: E, statement: Statement) -> Result<Vec<DbRow>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let rows = bind_sqlite(sqlx::query(&statement.sql), statement.args)
        .fetch_all(executor)
        .await?;
    rows.into_iter().map(sqlite_row).collect()
}

fn sqlite_row(row: SqliteRow) -> Result<DbRow> {
    let mut columns = Vec::with_capacity(row.len());
    let mut values = Vec::with_capacity(row.len());
    for (index, column) in row.columns().iter().enumerate() {
        columns.push(column.name().to_owned());
        let raw = row.try_get_raw(index)?;
        let value = if raw.is_null() {
            DbValue::Null
        } else {
            match raw.type_info().name() {
                "INTEGER" | "INT" | "BOOLEAN" => DbValue::Integer(row.try_get(index)?),
                "REAL" | "FLOAT" | "DOUBLE" => DbValue::Real(row.try_get(index)?),
                "BLOB" => DbValue::Blob(row.try_get(index)?),
                _ => DbValue::Text(row.try_get(index)?),
            }
        };
        values.push(value);
    }
    Ok(DbRow::new(columns, values))
}

#[derive(Clone)]
struct TursoDatabase {
    client: TursoClient,
}
impl TursoDatabase {
    fn connect(url: String, auth_token: String) -> Result<Self> {
        Ok(Self {
            client: TursoClient::new(url, auth_token)?,
        })
    }
}

impl PersistentDatabase for TursoDatabase {
    fn execute<'a>(&'a self, statement: Statement) -> DbFuture<'a, QueryResult> {
        Box::pin(async move { self.client.single(statement, false).await.map(|v| v.0) })
    }
    fn query<'a>(&'a self, statement: Statement) -> DbFuture<'a, Vec<DbRow>> {
        Box::pin(async move { self.client.single(statement, true).await.map(|v| v.1) })
    }
    fn begin<'a>(&'a self) -> DbFuture<'a, Box<dyn PersistentTransaction>> {
        Box::pin(async move {
            let mut tx = TursoTransaction {
                client: self.client.clone(),
                baton: None,
                base_url: None,
                closed: false,
            };
            tx.send(vec![execute_request(
                Statement {
                    sql: "BEGIN IMMEDIATE".into(),
                    args: Vec::new(),
                },
                false,
            )])
            .await?;
            Ok(Box::new(tx) as Box<dyn PersistentTransaction>)
        })
    }
    fn migrate<'a>(&'a self) -> DbFuture<'a, ()> {
        Box::pin(async move { migrate_turso(&self.client).await })
    }
    fn backend_name(&self) -> &'static str {
        "turso"
    }
}

#[derive(Clone)]
struct TursoClient {
    client: reqwest::Client,
    endpoint: String,
    auth_token: Arc<String>,
}
impl TursoClient {
    fn new(url: String, auth_token: String) -> Result<Self> {
        let base = if let Some(rest) = url.strip_prefix("libsql://") {
            format!("https://{rest}")
        } else if url.starts_with("https://") {
            url
        } else {
            bail!("TURSO_DATABASE_URL must use libsql:// or https://")
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .build()?;
        Ok(Self {
            client,
            endpoint: format!("{}/v3/pipeline", base.trim_end_matches('/')),
            auth_token: Arc::new(auth_token),
        })
    }

    async fn pipeline(
        &self,
        endpoint: &str,
        baton: Option<String>,
        requests: Vec<JsonValue>,
    ) -> Result<PipelineResponse> {
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(self.auth_token.as_str())
            .json(&json!({"baton":baton,"requests":requests}))
            .send()
            .await
            .context("Turso connection failed")?;
        let status = response.status();
        ensure!(status.is_success(), "Turso HTTP {}", status.as_u16());
        let body: JsonValue = response.json().await.context("invalid Turso response")?;
        let results = body["results"]
            .as_array()
            .context("Turso response has no results")?;
        let mut statements = Vec::new();
        for result in results {
            if result["type"] == "error" {
                bail!(
                    "Turso {}: {}",
                    result["error"]["code"].as_str().unwrap_or("error"),
                    result["error"]["message"]
                        .as_str()
                        .unwrap_or("query failed")
                );
            }
            if result["response"]["type"] == "execute" {
                statements.push(parse_turso_result(&result["response"]["result"])?);
            }
        }
        Ok(PipelineResponse {
            baton: body["baton"].as_str().map(str::to_owned),
            base_url: body["base_url"]
                .as_str()
                .map(|v| format!("{}/v3/pipeline", v.trim_end_matches('/'))),
            statements,
        })
    }

    async fn single(
        &self,
        statement: Statement,
        want_rows: bool,
    ) -> Result<(QueryResult, Vec<DbRow>)> {
        let response = self
            .pipeline(
                &self.endpoint,
                None,
                vec![
                    execute_request(statement, want_rows),
                    json!({"type":"close"}),
                ],
            )
            .await?;
        response
            .statements
            .into_iter()
            .next()
            .context("Turso returned no statement result")
    }

    async fn sequence(&self, sql: String) -> Result<()> {
        let response = self
            .pipeline(
                &self.endpoint,
                None,
                vec![
                    json!({"type":"sequence","sql":sql}),
                    json!({"type":"close"}),
                ],
            )
            .await?;
        ensure!(
            response.baton.is_none(),
            "Turso migration stream did not close"
        );
        Ok(())
    }
}

struct PipelineResponse {
    baton: Option<String>,
    base_url: Option<String>,
    statements: Vec<(QueryResult, Vec<DbRow>)>,
}

fn execute_request(statement: Statement, want_rows: bool) -> JsonValue {
    let args: Vec<JsonValue> = statement
        .args
        .into_iter()
        .map(|value| match value {
            DbValue::Null => json!({"type":"null"}),
            DbValue::Integer(value) => json!({"type":"integer","value":value.to_string()}),
            DbValue::Real(value) => json!({"type":"float","value":value}),
            DbValue::Text(value) => json!({"type":"text","value":value}),
            DbValue::Blob(value) => json!({"type":"blob","base64":STANDARD.encode(value)}),
        })
        .collect();
    json!({"type":"execute","stmt":{"sql":statement.sql,"args":args,"want_rows":want_rows}})
}

fn parse_turso_result(result: &JsonValue) -> Result<(QueryResult, Vec<DbRow>)> {
    let columns: Vec<String> = result["cols"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, col)| {
            col["name"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("column_{i}"))
        })
        .collect();
    let mut rows = Vec::new();
    if let Some(values) = result["rows"].as_array() {
        for row in values {
            let row_values = row
                .as_array()
                .context("invalid Turso row")?
                .iter()
                .map(parse_turso_value)
                .collect::<Result<Vec<_>>>()?;
            rows.push(DbRow::new(columns.clone(), row_values));
        }
    }
    let last_insert_rowid = match &result["last_insert_rowid"] {
        JsonValue::String(v) => v.parse().unwrap_or_default(),
        JsonValue::Number(v) => v.as_i64().unwrap_or_default(),
        _ => 0,
    };
    Ok((
        QueryResult {
            rows_affected: result["affected_row_count"].as_u64().unwrap_or_default(),
            last_insert_rowid,
        },
        rows,
    ))
}

fn parse_turso_value(value: &JsonValue) -> Result<DbValue> {
    Ok(
        match value["type"].as_str().context("invalid Turso value")? {
            "null" => DbValue::Null,
            "integer" => DbValue::Integer(
                value["value"]
                    .as_str()
                    .context("invalid Turso integer")?
                    .parse()?,
            ),
            "float" => DbValue::Real(value["value"].as_f64().unwrap_or(f64::NAN)),
            "text" => DbValue::Text(
                value["value"]
                    .as_str()
                    .context("invalid Turso text")?
                    .to_owned(),
            ),
            "blob" => DbValue::Blob(
                STANDARD.decode(value["base64"].as_str().context("invalid Turso blob")?)?,
            ),
            kind => bail!("unsupported Turso value type: {kind}"),
        },
    )
}

struct TursoTransaction {
    client: TursoClient,
    baton: Option<String>,
    base_url: Option<String>,
    closed: bool,
}
impl TursoTransaction {
    async fn send(&mut self, requests: Vec<JsonValue>) -> Result<PipelineResponse> {
        let endpoint = self
            .base_url
            .as_deref()
            .unwrap_or(&self.client.endpoint)
            .to_owned();
        let response = self
            .client
            .pipeline(&endpoint, self.baton.take(), requests)
            .await?;
        self.baton = response.baton.clone();
        if response.base_url.is_some() {
            self.base_url = response.base_url.clone();
        }
        Ok(response)
    }
    async fn statement(
        &mut self,
        statement: Statement,
        want_rows: bool,
    ) -> Result<(QueryResult, Vec<DbRow>)> {
        self.send(vec![execute_request(statement, want_rows)])
            .await?
            .statements
            .into_iter()
            .next()
            .context("Turso returned no transaction result")
    }
}
impl PersistentTransaction for TursoTransaction {
    fn execute<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, QueryResult> {
        Box::pin(async move { self.statement(statement, false).await.map(|v| v.0) })
    }
    fn query<'a>(&'a mut self, statement: Statement) -> DbFuture<'a, Vec<DbRow>> {
        Box::pin(async move { self.statement(statement, true).await.map(|v| v.1) })
    }
    fn commit<'a>(&'a mut self) -> DbFuture<'a, ()> {
        Box::pin(async move {
            self.send(vec![
                execute_request(
                    Statement {
                        sql: "COMMIT".into(),
                        args: Vec::new(),
                    },
                    false,
                ),
                json!({"type":"close"}),
            ])
            .await?;
            self.closed = true;
            Ok(())
        })
    }
    fn rollback<'a>(&'a mut self) -> DbFuture<'a, ()> {
        Box::pin(async move {
            self.send(vec![
                execute_request(
                    Statement {
                        sql: "ROLLBACK".into(),
                        args: Vec::new(),
                    },
                    false,
                ),
                json!({"type":"close"}),
            ])
            .await?;
            self.closed = true;
            Ok(())
        })
    }
}
impl Drop for TursoTransaction {
    fn drop(&mut self) {
        if self.closed || self.baton.is_none() {
            return;
        }
        let client = self.client.clone();
        let baton = self.baton.take();
        let endpoint = self
            .base_url
            .clone()
            .unwrap_or_else(|| client.endpoint.clone());
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = client
                    .pipeline(
                        &endpoint,
                        baton,
                        vec![
                            execute_request(
                                Statement {
                                    sql: "ROLLBACK".into(),
                                    args: Vec::new(),
                                },
                                false,
                            ),
                            json!({"type":"close"}),
                        ],
                    )
                    .await;
            });
        }
    }
}

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_evidence.sql")),
    (2, include_str!("../migrations/0002_visual_reports.sql")),
    (3, include_str!("../migrations/0003_onboarding.sql")),
    (4, include_str!("../migrations/0004_users.sql")),
    (5, include_str!("../migrations/0005_project_teams.sql")),
    (6, include_str!("../migrations/0006_inspection_runs.sql")),
    (
        7,
        include_str!("../migrations/0007_consent_and_group_teams.sql"),
    ),
    (8, include_str!("../migrations/0008_group_projects.sql")),
    (9, include_str!("../migrations/0009_rate_limits.sql")),
    (10, include_str!("../migrations/0010_group_listening.sql")),
    (
        11,
        include_str!("../migrations/0011_processing_animation.sql"),
    ),
    (
        12,
        include_str!("../migrations/0012_project_confirmation.sql"),
    ),
];

async fn migrate_turso(client: &TursoClient) -> Result<()> {
    client.sequence("CREATE TABLE IF NOT EXISTS datadog_schema_migrations(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);".into()).await?;
    let (_, rows) = client
        .single(
            Statement {
                sql: "SELECT version FROM datadog_schema_migrations ORDER BY version".into(),
                args: Vec::new(),
            },
            true,
        )
        .await?;
    let applied: std::collections::HashSet<i64> = rows
        .into_iter()
        .map(|row| row.try_get(0usize))
        .collect::<Result<_>>()?;
    for (version, sql) in MIGRATIONS {
        if applied.contains(version) {
            continue;
        }
        let migration = format!(
            "BEGIN IMMEDIATE;\n{sql}\nINSERT INTO datadog_schema_migrations(version,applied_at) VALUES({version},unixepoch());\nCOMMIT;"
        );
        client
            .sequence(migration)
            .await
            .with_context(|| format!("Turso migration {version} failed"))?;
    }
    Ok(())
}
