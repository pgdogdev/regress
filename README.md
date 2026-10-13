# PgDog regression testing

Pretty straightforward library to run regression tests against two PostgreSQL servers and make sure both
return the same responses.

Since we're a network proxy that also re-implements the protocol, these tests will assert equality of responses
all the way to the binary protocol level, byte-for-byte.

## How to use

Add the crate to your dependencies:

```toml
[dependencies]
pgdog-regress = { git = "https://github.com/pgdogdev/regress.git" }
```

Write your tests:

```rust
use pgdog_regress::prelude::*;

let server_a = ConnectionArgs {
    addr: "127.0.0.1:5432",
    user: "pgdog",
    password: "pgdog",
    database: "pgdog",
};
let server_b = ConnectionArgs {
    addr: "127.0.0.1:6432".into(),
    ..server_a
};

RegressTest::new(&["SELECT 1"])
    .run_simple(server_a, server_b)
    .await
    .unwrap();
```

The tests will panic if the two servers respond with different anything.

### Postgres regression suite

Postgres comes with its own regression tests, which you can run pretty easily:

```rust
let test = RegressTest::new_pg_regress(&["guc"]));
test.run_simple(&server_a, &server_b).await;
```
