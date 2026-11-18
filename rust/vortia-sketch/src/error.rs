use thiserror::Error;

#[derive(Debug, Error)]
pub enum SketchError {
    /// The sketch itself is wrong: a point that is not there, a number that is not finite, too many
    /// entities.
    #[error("{0}")]
    Invalid(String),

    /// The constraints cannot all be met, or the solver did not settle.
    #[error("{0}")]
    Unsolved(String),

    /// The sketch is fine but does not describe a wall profile a mesh can be made from.
    #[error("{0}")]
    Profile(String),

    #[error("could not read or write the sketch: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, SketchError>;
