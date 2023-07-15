// @filename: errors.rs
// @author: Krisna Pranav
// @license: 2023 Krisna Pranav, NanoBlocks Developers

pub type Result<T> = std::result::Result<T, Error>;
pub type ClientResult<T> = std::result::Result<T, ClientFailed>;

// implements this error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    // parse
    #[error("Parse failed: {0}")]
    ParseFailed(&'static str),

}

pub enum ClientFailed {
}