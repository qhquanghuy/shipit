#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Exit {
    Usage = 2,
}

impl Exit {
    pub(crate) const fn code(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) exit: Exit,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self {
            exit: Exit::Usage,
            message: message.into(),
        }
    }
}
