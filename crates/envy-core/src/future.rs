//! Seams left open for work that is not in v1.
//!
//! The types below are intentionally unimplemented. They exist so a later CLI,
//! an age-encrypted share, or a gitignore audit can sit next to the parser
//! without growing a second model of a `.env` file. Nothing in the app calls
//! them.

/// How a future "share this file" command would package a document.
/// Not implemented: no encryption, no HTML, no network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareFormat {
    /// [age](https://age-encryption.org) ciphertext.
    Age,
    /// A single HTML file that decrypts itself in the browser.
    SelfDecryptingHtml,
}

/// A future warning about a `.env` that git can already see.
/// Not implemented: Envy does not read git history and does not shell out to
/// `git log` in v1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitExposure {
    pub path: String,
    pub reason: GitExposureReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitExposureReason {
    NotGitignored,
    PresentInHistory,
}
