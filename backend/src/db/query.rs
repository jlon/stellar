//! 运行时 SQL 查询适配。
//!
//! SQLite/MySQL 使用 `?` 参数占位符，PostgreSQL 使用 `$1..$N`。sqlx 的动态
//! `query*` API 不会自动改写占位符，因此 [`AppDb`](super::AppDb) 关联各后端的
//! 查询实现：SQLite/MySQL 直接复用 sqlx Query，PostgreSQL 用 QueryBuilder 在
//! `.bind()` 时生成原生 `$N` 占位符。

use std::collections::VecDeque;
use std::marker::PhantomData;

use sqlx::{
    Database, Encode, Executor, FromRow, MySql, Sqlite, Type,
    database::HasArguments,
    mysql::MySqlQueryResult,
    postgres::{PgQueryResult, PgRow},
    sqlite::SqliteQueryResult,
};

use super::AppDb;

/// 后端查询实现的统一能力。
#[async_trait::async_trait]
pub trait AppQuery<'q, DB: AppDb>: Sized {
    fn bind<T: 'q + Send + Encode<'q, DB> + Type<DB>>(self, value: T) -> Self;

    async fn execute<'e, E>(self, executor: E) -> sqlx::Result<DB::QueryResult>
    where
        E: Executor<'e, Database = DB> + Send;

    async fn fetch_one<'e, E>(self, executor: E) -> sqlx::Result<DB::Row>
    where
        E: Executor<'e, Database = DB> + Send;

    async fn fetch_optional<'e, E>(self, executor: E) -> sqlx::Result<Option<DB::Row>>
    where
        E: Executor<'e, Database = DB> + Send;

    async fn fetch_all<'e, E>(self, executor: E) -> sqlx::Result<Vec<DB::Row>>
    where
        E: Executor<'e, Database = DB> + Send;

    async fn insert_id<'e, E>(self, executor: E) -> sqlx::Result<i64>
    where
        E: Executor<'e, Database = DB> + Send;
}

/// 业务层使用的数据库无关查询。
pub struct DbQuery<'q, DB: AppDb> {
    inner: DB::Query<'q>,
}

impl<'q, DB: AppDb> DbQuery<'q, DB> {
    fn new(sql: &'q str) -> Self {
        Self { inner: DB::make_query(sql) }
    }

    pub fn bind<T: 'q + Send + Encode<'q, DB> + Type<DB>>(self, value: T) -> Self {
        Self { inner: self.inner.bind(value) }
    }

    pub async fn execute<'e, E>(self, executor: E) -> sqlx::Result<DB::QueryResult>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.inner.execute(executor).await
    }

    pub async fn fetch_one<'e, E>(self, executor: E) -> sqlx::Result<DB::Row>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.inner.fetch_one(executor).await
    }

    pub async fn fetch_optional<'e, E>(self, executor: E) -> sqlx::Result<Option<DB::Row>>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.inner.fetch_optional(executor).await
    }

    pub async fn fetch_all<'e, E>(self, executor: E) -> sqlx::Result<Vec<DB::Row>>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.inner.fetch_all(executor).await
    }

    pub async fn insert_id<'e, E>(self, executor: E) -> sqlx::Result<i64>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.inner.insert_id(executor).await
    }
}

/// 创建无结果映射的 SQL 查询。
pub fn query<'q, DB: AppDb>(sql: &'q str) -> DbQuery<'q, DB> {
    DbQuery::new(sql)
}

/// 创建映射为结构体/元组的 SQL 查询。
pub fn query_as<'q, DB: AppDb, O>(sql: &'q str) -> DbQueryAs<'q, DB, O>
where
    O: for<'r> FromRow<'r, DB::Row>,
{
    DbQueryAs { query: query(sql), output: PhantomData }
}

/// 创建读取首列标量值的 SQL 查询。
pub fn query_scalar<'q, DB: AppDb, O>(sql: &'q str) -> DbQueryScalar<'q, DB, O>
where
    (O,): for<'r> FromRow<'r, DB::Row>,
{
    DbQueryScalar { query: query(sql), output: PhantomData }
}

/// `query_as` 对应的映射查询。
pub struct DbQueryAs<'q, DB: AppDb, O> {
    query: DbQuery<'q, DB>,
    output: PhantomData<O>,
}

impl<'q, DB: AppDb, O> DbQueryAs<'q, DB, O>
where
    O: Send + Unpin + for<'r> FromRow<'r, DB::Row>,
{
    pub fn bind<T: 'q + Send + Encode<'q, DB> + Type<DB>>(mut self, value: T) -> Self {
        self.query = self.query.bind(value);
        self
    }

    pub async fn fetch_one<'e, E>(self, executor: E) -> sqlx::Result<O>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        O::from_row(&self.query.fetch_one(executor).await?)
    }

    pub async fn fetch_optional<'e, E>(self, executor: E) -> sqlx::Result<Option<O>>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.query
            .fetch_optional(executor)
            .await?
            .map(|row| O::from_row(&row))
            .transpose()
    }

    pub async fn fetch_all<'e, E>(self, executor: E) -> sqlx::Result<Vec<O>>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.query
            .fetch_all(executor)
            .await?
            .iter()
            .map(O::from_row)
            .collect()
    }
}

/// `query_scalar` 对应的标量查询。
pub struct DbQueryScalar<'q, DB: AppDb, O> {
    query: DbQuery<'q, DB>,
    output: PhantomData<O>,
}

impl<'q, DB: AppDb, O> DbQueryScalar<'q, DB, O>
where
    O: Send + Unpin,
    (O,): for<'r> FromRow<'r, DB::Row>,
{
    pub fn bind<T: 'q + Send + Encode<'q, DB> + Type<DB>>(mut self, value: T) -> Self {
        self.query = self.query.bind(value);
        self
    }

    pub async fn fetch_one<'e, E>(self, executor: E) -> sqlx::Result<O>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        let (value,) = <(O,)>::from_row(&self.query.fetch_one(executor).await?)?;
        Ok(value)
    }

    pub async fn fetch_optional<'e, E>(self, executor: E) -> sqlx::Result<Option<O>>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.query
            .fetch_optional(executor)
            .await?
            .map(|row| <(O,)>::from_row(&row).map(|(value,)| value))
            .transpose()
    }

    pub async fn fetch_all<'e, E>(self, executor: E) -> sqlx::Result<Vec<O>>
    where
        E: Executor<'e, Database = DB> + Send,
    {
        self.query
            .fetch_all(executor)
            .await?
            .iter()
            .map(|row| <(O,)>::from_row(row).map(|(value,)| value))
            .collect()
    }
}

macro_rules! impl_direct_sqlx_query {
    ($db:ty, $insert_id:expr) => {
        #[async_trait::async_trait]
        impl<'q> AppQuery<'q, $db>
            for sqlx::query::Query<'q, $db, <$db as HasArguments<'q>>::Arguments>
        {
            fn bind<T: 'q + Send + Encode<'q, $db> + Type<$db>>(self, value: T) -> Self {
                sqlx::query::Query::bind(self, value)
            }

            async fn execute<'e, E>(
                self,
                executor: E,
            ) -> sqlx::Result<<$db as Database>::QueryResult>
            where
                E: Executor<'e, Database = $db> + Send,
            {
                sqlx::query::Query::execute(self, executor).await
            }

            async fn fetch_one<'e, E>(self, executor: E) -> sqlx::Result<<$db as Database>::Row>
            where
                E: Executor<'e, Database = $db> + Send,
            {
                sqlx::query::Query::fetch_one(self, executor).await
            }

            async fn fetch_optional<'e, E>(
                self,
                executor: E,
            ) -> sqlx::Result<Option<<$db as Database>::Row>>
            where
                E: Executor<'e, Database = $db> + Send,
            {
                sqlx::query::Query::fetch_optional(self, executor).await
            }

            async fn fetch_all<'e, E>(
                self,
                executor: E,
            ) -> sqlx::Result<Vec<<$db as Database>::Row>>
            where
                E: Executor<'e, Database = $db> + Send,
            {
                sqlx::query::Query::fetch_all(self, executor).await
            }

            async fn insert_id<'e, E>(self, executor: E) -> sqlx::Result<i64>
            where
                E: Executor<'e, Database = $db> + Send,
            {
                let result = sqlx::query::Query::execute(self, executor).await?;
                Ok(($insert_id)(result))
            }
        }
    };
}

impl_direct_sqlx_query!(Sqlite, |result: SqliteQueryResult| result.last_insert_rowid());
impl_direct_sqlx_query!(MySql, |result: MySqlQueryResult| result.last_insert_id() as i64);

/// PostgreSQL 的动态查询实现。
pub struct PostgresQuery<'q> {
    builder: sqlx::QueryBuilder<'q, sqlx::Postgres>,
    remaining_segments: VecDeque<String>,
}

impl<'q> PostgresQuery<'q> {
    pub(crate) fn new(sql: &'q str) -> Self {
        let (initial_sql, remaining_segments) = split_parameter_markers(sql);
        Self {
            builder: sqlx::QueryBuilder::new(initial_sql),
            remaining_segments: remaining_segments.into(),
        }
    }

    fn ensure_all_bound(&self) {
        assert!(self.remaining_segments.is_empty(), "SQL parameter count exceeds bind count");
    }
}

#[async_trait::async_trait]
impl<'q> AppQuery<'q, sqlx::Postgres> for PostgresQuery<'q> {
    fn bind<T: 'q + Send + Encode<'q, sqlx::Postgres> + Type<sqlx::Postgres>>(
        mut self,
        value: T,
    ) -> Self {
        let segment = self
            .remaining_segments
            .pop_front()
            .expect("bind count exceeds SQL parameter count");
        self.builder.push_bind(value);
        self.builder.push(&segment);
        self
    }

    async fn execute<'e, E>(mut self, executor: E) -> sqlx::Result<PgQueryResult>
    where
        E: Executor<'e, Database = sqlx::Postgres> + Send,
    {
        self.ensure_all_bound();
        self.builder.build().execute(executor).await
    }

    async fn fetch_one<'e, E>(mut self, executor: E) -> sqlx::Result<PgRow>
    where
        E: Executor<'e, Database = sqlx::Postgres> + Send,
    {
        self.ensure_all_bound();
        self.builder.build().fetch_one(executor).await
    }

    async fn fetch_optional<'e, E>(mut self, executor: E) -> sqlx::Result<Option<PgRow>>
    where
        E: Executor<'e, Database = sqlx::Postgres> + Send,
    {
        self.ensure_all_bound();
        self.builder.build().fetch_optional(executor).await
    }

    async fn fetch_all<'e, E>(mut self, executor: E) -> sqlx::Result<Vec<PgRow>>
    where
        E: Executor<'e, Database = sqlx::Postgres> + Send,
    {
        self.ensure_all_bound();
        self.builder.build().fetch_all(executor).await
    }

    async fn insert_id<'e, E>(mut self, executor: E) -> sqlx::Result<i64>
    where
        E: Executor<'e, Database = sqlx::Postgres> + Send,
    {
        self.ensure_all_bound();
        self.builder.push(" RETURNING id");
        self.builder
            .build_query_scalar::<i64>()
            .fetch_one(executor)
            .await
    }
}

fn split_parameter_markers(sql: &str) -> (String, Vec<String>) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        SingleQuoted,
        DoubleQuoted,
        LineComment,
        BlockComment,
    }

    let mut segments = vec![String::new()];
    let mut state = State::Normal;
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => match ch {
                '?' => segments.push(String::new()),
                '\'' => {
                    state = State::SingleQuoted;
                    segments.last_mut().unwrap().push(ch);
                },
                '"' => {
                    state = State::DoubleQuoted;
                    segments.last_mut().unwrap().push(ch);
                },
                '-' if chars.peek() == Some(&'-') => {
                    segments.last_mut().unwrap().push(ch);
                    segments.last_mut().unwrap().push(chars.next().unwrap());
                    state = State::LineComment;
                },
                '/' if chars.peek() == Some(&'*') => {
                    segments.last_mut().unwrap().push(ch);
                    segments.last_mut().unwrap().push(chars.next().unwrap());
                    state = State::BlockComment;
                },
                _ => segments.last_mut().unwrap().push(ch),
            },
            State::SingleQuoted => {
                segments.last_mut().unwrap().push(ch);
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        segments.last_mut().unwrap().push(chars.next().unwrap());
                    } else {
                        state = State::Normal;
                    }
                }
            },
            State::DoubleQuoted => {
                segments.last_mut().unwrap().push(ch);
                if ch == '"' {
                    if chars.peek() == Some(&'"') {
                        segments.last_mut().unwrap().push(chars.next().unwrap());
                    } else {
                        state = State::Normal;
                    }
                }
            },
            State::LineComment => {
                segments.last_mut().unwrap().push(ch);
                if ch == '\n' {
                    state = State::Normal;
                }
            },
            State::BlockComment => {
                segments.last_mut().unwrap().push(ch);
                if ch == '*' && chars.peek() == Some(&'/') {
                    segments.last_mut().unwrap().push(chars.next().unwrap());
                    state = State::Normal;
                }
            },
        }
    }

    let initial = segments.remove(0);
    (initial, segments)
}

#[cfg(test)]
mod tests {
    use super::split_parameter_markers;

    #[test]
    fn splits_only_parameter_markers() {
        let (initial, segments) = split_parameter_markers("SELECT ?, '?', \"?\", -- ?\n ? /* ? */");
        assert_eq!(initial, "SELECT ");
        assert_eq!(segments, vec![", '?', \"?\", -- ?\n ", " /* ? */"]);
    }
}
