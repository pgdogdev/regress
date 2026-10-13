use bytes::BytesMut;
use fallible_iterator::FallibleIterator;
use postgres_protocol::{
    authentication::sasl::{ChannelBinding, SCRAM_SHA_256, ScramSha256},
    message::{
        backend::{Header, Message},
        frontend,
    },
};
use std::{
    fmt::Display,
    io::{Error, ErrorKind},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct ConnectionArgs<'a> {
    pub addr: &'a str,
    pub user: &'a str,
    pub database: &'a str,
    pub password: &'a str,
}

impl Display for ConnectionArgs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}/{}", self.user, self.addr, self.database)
    }
}

#[derive(Debug)]
pub struct Connection {
    stream: TcpStream,
}

impl Connection {
    /// Create a connection and authenticate if needed.
    pub async fn connect(args: ConnectionArgs<'_>) -> Result<Self, Error> {
        let ConnectionArgs {
            addr,
            user,
            database,
            password,
        } = args;

        let mut conn = Self {
            stream: TcpStream::connect(addr).await?,
        };
        let mut buf = BytesMut::new();
        frontend::startup_message(
            [
                ("user", user),
                ("database", database),
                ("client_encoding", "UTF8"),
            ],
            &mut buf,
        )?;
        conn.stream.write_all(&buf).await?;

        enum State {
            Authentication,
            ScramContinue(ScramSha256),
            ScramFinal(ScramSha256),
            AuthenticationOk,
            Ready,
        }

        let mut state = State::Authentication;
        loop {
            let message = conn.read_message().await?;
            if matches!(message, Message::NoticeResponse(_)) {
                continue;
            }
            buf.clear();
            state = match (state, message) {
                (State::Authentication, Message::AuthenticationSasl(body)) => {
                    let mut mechanisms = body.mechanisms();
                    let mut supported = false;
                    while let Some(mechanism) = mechanisms.next()? {
                        supported |= mechanism == SCRAM_SHA_256;
                    }
                    if !supported {
                        return Err(Error::new(
                            ErrorKind::Unsupported,
                            "server does not offer SCRAM-SHA-256",
                        ));
                    }
                    let scram =
                        ScramSha256::new(password.as_bytes(), ChannelBinding::unsupported());
                    frontend::sasl_initial_response(SCRAM_SHA_256, scram.message(), &mut buf)?;
                    conn.stream.write_all(&buf).await?;
                    State::ScramContinue(scram)
                }
                (State::ScramContinue(mut scram), Message::AuthenticationSaslContinue(body)) => {
                    scram.update(body.data())?;
                    frontend::sasl_response(scram.message(), &mut buf)?;
                    conn.stream.write_all(&buf).await?;
                    State::ScramFinal(scram)
                }
                (State::ScramFinal(mut scram), Message::AuthenticationSaslFinal(body)) => {
                    scram.finish(body.data())?;
                    State::AuthenticationOk
                }
                (State::Authentication | State::AuthenticationOk, Message::AuthenticationOk) => {
                    State::Ready
                }
                (State::Ready, Message::ParameterStatus(_) | Message::BackendKeyData(_)) => {
                    State::Ready
                }
                (State::Ready, Message::ReadyForQuery(_)) => return Ok(conn),
                (State::Authentication, _) => {
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        "expected SCRAM-SHA-256 authentication or AuthenticationOk",
                    ));
                }
                (_, Message::ErrorResponse(_)) => {
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        "wrong username/password",
                    ));
                }
                _ => {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        "unexpected message during startup",
                    ));
                }
            };
        }
    }

    async fn read_message(&mut self) -> Result<Message, Error> {
        let mut header = [0; 5];
        self.stream.read_exact(&mut header).await?;
        let header_len = Header::parse(&header)?.unwrap().len() as usize;
        let mut buf = BytesMut::from(header.as_slice());
        buf.resize(header_len + 1, 0);
        self.stream.read_exact(&mut buf[5..]).await?;

        Message::parse(&mut buf)?
            .ok_or_else(|| Error::new(ErrorKind::UnexpectedEof, "incomplete backend message"))
    }

    /// Run a query using the simple protocol against this connection and get all messages back.
    pub async fn query_simple(&mut self, query: &str) -> Result<Vec<Message>, Error> {
        let mut buf = BytesMut::new();
        frontend::query(query, &mut buf)?;
        self.stream.write_all(&buf).await?;

        self.read_messages().await
    }

    /// Run a query using an unnamed prepared statement and portal with text parameters.
    /// `None` represents SQL NULL; parameter types are inferred from the query.
    pub async fn query_extended(
        &mut self,
        query: &str,
        params: &[Option<&str>],
    ) -> Result<Vec<Message>, Error> {
        let mut buf = BytesMut::new();
        frontend::parse("", query, [], &mut buf)?;
        frontend::bind(
            "",
            "",
            [0],
            params.iter().copied(),
            |value, buf| match value {
                Some(value) => {
                    buf.extend_from_slice(value.as_bytes());
                    Ok(postgres_protocol::IsNull::No)
                }
                None => Ok(postgres_protocol::IsNull::Yes),
            },
            [],
            &mut buf,
        )
        .map_err(|error| match error {
            frontend::BindError::Conversion(error) => Error::other(error),
            frontend::BindError::Serialization(error) => error,
        })?;
        frontend::describe(b'P', "", &mut buf)?;
        frontend::execute("", 0, &mut buf)?;
        frontend::sync(&mut buf);
        self.stream.write_all(&buf).await?;

        self.read_messages().await
    }

    async fn read_messages(&mut self) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        loop {
            let message = self.read_message().await?;
            let ready = matches!(message, Message::ReadyForQuery(_));
            messages.push(message);
            if ready {
                return Ok(messages);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_connect() {
        let mut conn = Connection::connect(ConnectionArgs {
            addr: "127.0.0.1:5432",
            user: "pgdog",
            database: "pgdog",
            password: "pgdog",
        })
        .await
        .unwrap();
        let result_simple = conn.query_simple("SELECT 1 AS value").await.unwrap();
        let result_extended = conn
            .query_extended("SELECT $1::int4 AS value", &[Some("1")])
            .await
            .unwrap();
        assert_eq!(4, result_simple.len());
        assert_eq!(6, result_extended.len());
        assert_eq!(result_extended[2..], result_simple[..]);
    }
}
