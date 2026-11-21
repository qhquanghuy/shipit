#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that reads the text files of the formats"
)]

use std::fs;
use std::path::Path;

use crate::pure::failure::Failure;

// Creates nothing; a folder and an unreadable file count as missing.
pub(crate) fn read_text(path: &Path) -> Result<String, Failure> {
    let bytes =
        fs::read(path).map_err(|_why| Failure::usage(format!("no file at {}", path.display())))?;
    String::from_utf8(bytes)
        .map_err(|_why| Failure::usage(format!("{} is not UTF-8 text", path.display())))
}

#[cfg(test)]
mod tests {
    use crate::edge::testing::scratch;

    use super::*;

    #[test]
    fn read_text_names_a_missing_file_and_non_utf8_text() {
        let dir = scratch("read-text");
        let text = dir.join("plain.md");
        fs::write(&text, "héllo\n").expect("a file");
        let binary = dir.join("binary.md");
        fs::write(&binary, [0xff_u8, 0xfe, 0x00]).expect("a file");
        let missing = dir.join("missing.md");

        assert_eq!(read_text(&text), Ok("héllo\n".to_owned()));
        assert_eq!(
            read_text(&missing),
            Err(Failure::usage(format!("no file at {}", missing.display())))
        );
        assert_eq!(
            read_text(&dir),
            Err(Failure::usage(format!("no file at {}", dir.display())))
        );
        assert_eq!(
            read_text(&binary),
            Err(Failure::usage(format!(
                "{} is not UTF-8 text",
                binary.display()
            )))
        );
        assert!(!missing.exists(), "reading creates nothing");
    }
}
