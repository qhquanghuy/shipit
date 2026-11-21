#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Exit {
    No = 1,
    Usage = 2,
    Unclean = 3,
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
    pub(crate) fn no(message: impl Into<String>) -> Self {
        Self {
            exit: Exit::No,
            message: message.into(),
        }
    }

    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self {
            exit: Exit::Usage,
            message: message.into(),
        }
    }

    pub(crate) fn unclean(message: impl Into<String>) -> Self {
        Self {
            exit: Exit::Unclean,
            message: message.into(),
        }
    }
}
