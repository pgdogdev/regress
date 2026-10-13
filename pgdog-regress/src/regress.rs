//! Regression testing harness.

use std::{io::Error, ops::Deref};

use crate::{Connection, conn::ConnectionArgs};

#[derive(Debug, Clone)]
pub struct RegressTest {
    queries: Vec<String>,
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
        }
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

        Self { queries }
    }

    /// Run simple protocol regression tests.
    pub async fn run_simple(&self, a: ConnectionArgs, b: ConnectionArgs) -> Result<(), Error> {
        let mut a = Connection::connect(a).await?;
        let mut b = Connection::connect(b).await?;

        for query in &self.queries {
            let res_a = a.query_simple(query).await?;
            let res_b = b.query_simple(query).await?;

            assert_eq!(res_a, res_b, "query: {}", query);
        }

        Ok(())
    }

    pub async fn run_extended(&self, a: ConnectionArgs, b: ConnectionArgs) -> Result<(), Error> {
        let mut a = Connection::connect(a).await?;
        let mut b = Connection::connect(b).await?;

        for query in &self.queries {
            let res_a = a.query_extended(query, &[]).await?;
            let res_b = b.query_extended(query, &[]).await?;

            assert_eq!(res_a, res_b, "query: {}", query);
        }

        Ok(())
    }

    /// Run this test against hardcoded addresses we use in local dev and in CI.
    pub async fn run_simple_ci(&self) -> Result<(), Error> {
        let direct = ConnectionArgs {
            addr: "127.0.0.1:5432".to_string(),
            user: "pgdog".to_string(),
            password: "pgdog".to_string(),
            database: "pgdog".to_string(),
        };

        let pgdog = ConnectionArgs {
            addr: "127.0.0.1:6432".to_string(),
            user: "pgdog".to_string(),
            password: "pgdog".to_string(),
            database: "pgdog".to_string(),
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
        let test = RegressTest::new(&["SELECT 1"]);
        let conn = ConnectionArgs {
            addr: "127.0.0.1:5432".to_string(),
            user: "pgdog".to_string(),
            password: "pgdog".to_string(),
            database: "pgdog".to_string(),
        };

        test.run_simple(conn.clone(), conn).await.unwrap();
    }
}
