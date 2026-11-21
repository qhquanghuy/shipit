use crate::pure::setup::Cap;

pub(crate) fn size_verdict(files: u64, cap: Cap) -> (bool, String) {
    (
        files <= cap.get(),
        format!("size: {files} files (cap {})", cap.get()),
    )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::pure::testing::cap;

    #[test]
    fn the_message_names_the_files_and_the_cap() {
        assert_eq!(
            size_verdict(61, cap(60)),
            (false, "size: 61 files (cap 60)".to_owned())
        );
        assert_eq!(
            size_verdict(60, cap(60)),
            (true, "size: 60 files (cap 60)".to_owned())
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn ok_exactly_when_the_files_fit(files in 0_u64..200, limit in 1_u64..200) {
            prop_assert_eq!(size_verdict(files, cap(limit)).0, files <= limit);
        }
    }
}
