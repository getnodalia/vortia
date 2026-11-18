use std::fmt;

/// Why a case could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseError {
    /// A number or a name that cannot make a case.
    Invalid(String),
    /// A file or folder that could not be written.
    Io(String),
    /// The mesh the generated `blockMeshDict` describes could not be built.
    Mesh(String),
}

impl fmt::Display for CaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaseError::Invalid(m) | CaseError::Io(m) | CaseError::Mesh(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for CaseError {}

pub type Result<T> = std::result::Result<T, CaseError>;
