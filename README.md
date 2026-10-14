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

### Known errors

To exclude known bad queries from execution, mark queries in the test suite:

```rust
RegressTest::new(&["SELECT 1", "SELECT version()"])
    .known_bad_queries(&["SELECT version()"])
    .run_simple(server_a, server_b)
    .await
    .unwrap();
```
