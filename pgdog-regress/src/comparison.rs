//! Compare responses without PostgreSQL build-specific diagnostic locations.

use fallible_iterator::FallibleIterator;
use postgres_protocol::message::backend::{ErrorFields, Message};

pub(super) fn responses_equal(a: &[Message], b: &[Message]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Message::ErrorResponse(a), Message::ErrorResponse(b)) => {
                fields_equal(a.fields(), b.fields())
            }
            (Message::NoticeResponse(a), Message::NoticeResponse(b)) => {
                fields_equal(a.fields(), b.fields())
            }
            _ => a == b,
        })
}

fn fields_equal(a: ErrorFields<'_>, b: ErrorFields<'_>) -> bool {
    // File, line and routine describe the server implementation, not the error.
    // Keep all other fields, including SQLSTATE and query positions.
    a.filter(|field| Ok(!matches!(field.type_(), b'F' | b'L' | b'R')))
        .eq(b.filter(|field| Ok(!matches!(field.type_(), b'F' | b'L' | b'R'))))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::slice::from_ref;

    use bytes::{BufMut, BytesMut};

    use super::*;

    fn message(code: u8, body: &[u8]) -> Message {
        let mut bytes = BytesMut::new();
        bytes.put_u8(code);
        bytes.put_i32((body.len() + 4) as i32);
        bytes.extend_from_slice(body);
        Message::parse(&mut bytes)
            .expect("valid message frame")
            .expect("complete message")
    }

    fn diagnostic(code: u8, fields: &[(u8, &str)]) -> Message {
        let mut body = Vec::new();
        for (kind, value) in fields {
            body.push(*kind);
            body.extend_from_slice(value.as_bytes());
            body.push(0);
        }
        body.push(0);
        message(code, &body)
    }

    #[test]
    fn ignores_changed_or_missing_source_locations() {
        for code in [b'E', b'N'] {
            let fields = [
                (b'S', "ERROR"),
                (b'C', "22023"),
                (
                    b'M',
                    "invalid value for parameter \"IntervalStyle\": \"asd\"",
                ),
                (
                    b'H',
                    "Available values: postgres, postgres_verbose, sql_standard, iso_8601.",
                ),
            ];
            let mut first = fields.to_vec();
            first.extend([
                (b'F', "guc.c"),
                (b'L', "3278"),
                (b'R', "parse_and_validate_value"),
            ]);
            let mut second = fields.to_vec();
            second.extend([
                (b'F', "guc.c"),
                (b'L', "3282"),
                (b'R', "parse_and_validate_value"),
            ]);
            let a = diagnostic(code, &first);
            let b = diagnostic(code, &second);
            assert_ne!(a, b);
            assert!(responses_equal(from_ref(&a), &[b]));
            second.truncate(fields.len());
            second.extend([(b'F', "other.c"), (b'L', "1"), (b'R', "other_routine")]);
            assert!(responses_equal(from_ref(&a), &[diagnostic(code, &second)]));
            assert!(responses_equal(&[a], &[diagnostic(code, &fields)]));
        }
    }

    #[test]
    fn preserves_meaningful_diagnostic_fields() {
        for code in [b'E', b'N'] {
            for kind in [
                b'S', b'V', b'C', b'M', b'D', b'H', b'P', b'p', b'q', b'W', b's', b't', b'c', b'd',
                b'n',
            ] {
                let a = diagnostic(code, &[(kind, "first")]);
                let b = diagnostic(code, &[(kind, "second")]);
                assert!(!responses_equal(from_ref(&a), &[b]), "field {kind}");
                assert!(
                    !responses_equal(&[a], &[diagnostic(code, &[])]),
                    "field {kind}"
                );
            }
        }
    }

    #[test]
    fn preserves_message_types_order_and_payloads() {
        let command = message(b'C', b"SET\0");
        let ready = message(b'Z', b"I");
        assert!(responses_equal(
            &[command.clone(), ready.clone()],
            &[command.clone(), ready.clone()]
        ));
        assert!(!responses_equal(
            &[command.clone(), ready.clone()],
            &[ready.clone(), command.clone()]
        ));
        assert!(!responses_equal(
            &[command.clone(), ready.clone()],
            from_ref(&command)
        ));
        assert!(!responses_equal(&[command], &[message(b'C', b"RESET\0")]));
        assert!(!responses_equal(&[ready], &[message(b'Z', b"T")]));
        let fields = [(b'M', "same message")];
        assert!(!responses_equal(
            &[diagnostic(b'E', &fields)],
            &[diagnostic(b'N', &fields)]
        ));
    }

    #[test]
    fn malformed_diagnostics_do_not_compare_equal() {
        let malformed = message(b'E', b"SERROR\0C22023");
        assert!(!responses_equal(from_ref(&malformed), from_ref(&malformed)));
    }
}
