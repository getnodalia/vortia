/// Why a file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum H5Error {
    #[error("{0}")]
    Io(String),
    #[error("not an HDF5 file")]
    NotHdf5,
    #[error("the file is cut short or damaged: {0}")]
    Corrupt(String),
    #[error("not supported yet: {0}")]
    Unsupported(String),
    #[error("no such object: {0}")]
    NotFound(String),
    #[error("too large to read: {0}")]
    TooLarge(String),
}

pub type Result<T> = std::result::Result<T, H5Error>;

pub(crate) fn corrupt<T>(what: impl Into<String>) -> Result<T> {
    Err(H5Error::Corrupt(what.into()))
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> Result<T> {
    Err(H5Error::Unsupported(what.into()))
}
