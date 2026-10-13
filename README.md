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

let test = RegressTest::new(&["SELECT 1", "SELECT 1234"]);
let server_a = ConnectionArgs {
    addr: "127.0.0.1:5432".into(),
    user: "pgdog".into(),
    password: "pgdog".into(),
    database: "pgdog".into()
};
let server_b = {
    ..server_a,
    addr: "127.0.0.1:6432".into(),
};

test.run_simple(&server_a, &server_b).await;
```

The tests will panic if the two servers respond with different anything.

### Postgres regression suite

Postgres comes with its own regression tests, which you can run pretty easily:

```rust
let test = RegressTest::new_pg_regress(&["guc"]));
test.run_simple(&server_a, &server_b).await;
```
