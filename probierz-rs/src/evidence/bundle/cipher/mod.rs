//! Sealing and opening a bundle: the keys, headers and modes it is written
//! with, the GCM it is authenticated with, and the plaintext it replaces.

mod crypto;
mod gcm;
mod plaintext;

pub(crate) use crypto::*;
pub(crate) use gcm::*;
pub(crate) use plaintext::*;
