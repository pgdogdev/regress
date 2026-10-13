//! Regression testing harness.

use std::{io::Error, ops::Deref};

use postgres_protocol::message::backend::Message;

use crate::{Connection, conn::ConnectionArgs};

#[derive(Debug, Clone)]
pub struct RegressTest {
    queries: Vec<String>,
    fail_fast: bool,
    error: bool,
}

impl Deref for RegressTest {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.queries
    }
}

impl RegressTest {
    /// Run regression tests for the following queries.
    pub fn new(queries: &[&str]) -> Self {
        Self {
            queries: queries.iter().map(|f| f.to_string()).collect(),
            fail_fast: true,
            error: false,
        }
    }

    /// Log mismatches and continue, then panic at the end if any were found.
    /// Connection errors still stop the run immediately.
    pub fn no_fail_fast(mut self) -> Self {
        self.fail_fast = false;
        self
    }

    /// Run regression tests for queries included as part of the standard Postgres
    /// regression suite. Tests are named by their file name, e.g., "guc" => "guc.sql".
    pub fn new_pg_regress(tests: &[&str]) -> Self {
        let files = pg_raw_parse::POSTGRES_REGRESS.files().filter(|file| {
            if tests.is_empty() {
                true
            } else {
                let stem = file.path().file_stem().unwrap().to_str().unwrap();
                tests.contains(&stem)
            }
        });
        let mut queries = vec![];

        for file in files {
            let contents = file.contents_utf8().expect("test file is not utf8");
            let parsed_queries =
                pg_raw_parse::parse(contents).expect("file doesn't contain valid SQL queries");

            for query in parsed_queries.iter() {
                let pos = query.stmt_location as usize
                    ..query.stmt_location as usize + query.stmt_len as usize;
                let query = &contents[pos];
                queries.push(query.to_string());
            }
        }

        Self {
            queries,
            fail_fast: true,
            error: false,
        }
    }

    fn assert_eq(
        &mut self,
        query: usize,
        a: (ConnectionArgs<'_>, Vec<Message>),
        b: (ConnectionArgs<'_>, Vec<Message>),
    ) {
        let actual = &self.queries[query];
        let context_size = std::cmp::max(query as isize - 3, 0) as usize;
        let context_queries = &self.queries[context_size..query]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        if a.1 != b.1 {
            eprintln!(
                r#"
=====================
Query: {}
=====================
Context: {}
=====================
{}:

{:#?}
=====================
{}:

{:#?}
"#,
                actual, context_queries, a.0, a.1, b.0, b.1
            );
            self.error = true;

            if self.fail_fast {
                panic!("Failing fast, the rest of the test suite won't run.");
            }
        }
    }

    fn query_error(&self, query: usize, server: ConnectionArgs<'_>, error: Error) -> Error {
        let context = self.queries[query.saturating_sub(3)..query].join("\n");
        Error::new(
            error.kind(),
            format!(
                "Query: {}\nContext: {context}\nServer: {server}\n{error}",
                self.queries[query]
            ),
        )
    }

    /// Run simple protocol regression tests.
    pub async fn run_simple(
        &mut self,
        a: ConnectionArgs<'_>,
        b: ConnectionArgs<'_>,
    ) -> Result<(), Error> {
        let mut conn_a = Connection::connect(a).await?;
        let mut conn_b = Connection::connect(b).await?;

        for (idx, query) in self.queries.clone().iter().enumerate() {
            let res_a = conn_a
                .query_simple(query)
                .await
                .map_err(|error| self.query_error(idx, a, error))?;
            let res_b = conn_b
                .query_simple(query)
                .await
                .map_err(|error| self.query_error(idx, b, error))?;

            self.assert_eq(idx, (a, res_a), (b, res_b));
        }

        if self.error {
            panic!("Regression tests failed");
        }

        Ok(())
    }

    pub async fn run_extended(
        &mut self,
        a: ConnectionArgs<'_>,
        b: ConnectionArgs<'_>,
    ) -> Result<(), Error> {
        let mut conn_a = Connection::connect(a).await?;
        let mut conn_b = Connection::connect(b).await?;

        for (idx, query) in self.queries.clone().iter().enumerate() {
            let res_a = conn_a
                .query_simple(query)
                .await
                .map_err(|error| self.query_error(idx, a, error))?;
            let res_b = conn_b
                .query_simple(query)
                .await
                .map_err(|error| self.query_error(idx, b, error))?;

            self.assert_eq(idx, (a, res_a), (b, res_b));
        }

        if self.error {
            panic!("Regression tests failed");
        }

        Ok(())
    }

    /// Run this test against hardcoded addresses we use in local dev and in CI.
    pub async fn run_simple_ci(&mut self) -> Result<(), Error> {
        let direct = ConnectionArgs {
            addr: "127.0.0.1:5432",
            user: "pgdog",
            password: "pgdog",
            database: "pgdog",
        };

        let pgdog = ConnectionArgs {
            addr: "127.0.0.1:6432",
            user: "pgdog",
            password: "pgdog",
            database: "pgdog",
        };

        self.run_simple(direct, pgdog).await
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_guc() {
        let queries = RegressTest::new_pg_regress(&["guc"]);
        assert!(queries.len() > 1);
    }

    #[tokio::test]
    async fn test_simple() {
        let mut test = RegressTest::new(&["SELECT 1"]);
        let conn = ConnectionArgs {
            addr: "127.0.0.1:5432",
            user: "pgdog",
            password: "pgdog",
            database: "pgdog",
        };

        test.run_simple(conn.clone(), conn).await.unwrap();
    }

    #[tokio::test]
    #[should_panic]
    async fn test_local() {
        let server_a = ConnectionArgs {
            addr: "127.0.0.1:5432".into(),
            user: "pgdog".into(),
            password: "pgdog".into(),
            database: "pgdog".into(),
        };
        let server_b = ConnectionArgs {
            addr: "127.0.0.1:6432".into(),
            ..server_a
        };

        RegressTest::new_pg_regress(&["guc"])
            .no_fail_fast()
            .run_simple(server_a, server_b)
            .await
            .unwrap();
    }
}
