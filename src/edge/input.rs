#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that reads standard input"
)]

use std::io::Read;

// One byte more than the limit is read, to tell a text of exactly the limit from a longer one.
fn read_limited(reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let room = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    reader
        .take(room)
        .read_to_end(&mut bytes)
        .map_err(|why| why.to_string())?;
    if bytes.len() > limit {
        return Err(format!("input over {limit} bytes"));
    }
    Ok(bytes)
}

pub(crate) fn read_stdin(limit: usize) -> Result<Vec<u8>, String> {
    read_limited(std::io::stdin().lock(), limit)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn input_over_the_limit_is_refused_and_the_rest_is_read_whole(
            bytes in proptest::collection::vec(any::<u8>(), 0..64),
            limit in 0_usize..64,
        ) {
            let read = read_limited(bytes.as_slice(), limit);
            if bytes.len() > limit {
                prop_assert_eq!(read, Err(format!("input over {limit} bytes")));
            } else {
                prop_assert_eq!(read, Ok(bytes));
            }
        }
    }
}
