//! Durable evidence: history, signing, publication, protection, and audit.
//!
//! Evidence is a security boundary. Paths are kept beneath their declared
//! roots, signed payloads are canonicalized before Ed25519 operations, and an
//! encrypted bundle is authenticated before any member is restored.
//!
//! | part | what it owns |
//! |---|---|
//! | [`basics`] | digests, canonical JSON, and the path rules everything else stands on |
//! | [`history`] | the runs on disk, the audit trail, and the secret scan |
//! | [`bundle`] | encrypting a run's artifacts, restoring them, and retention |
//! | [`receipt`] | what a receipt records, the identity it names, and its signature |
//! | [`publication`] | publishing what a verified run proves |
//! | [`resources`] | the leases a run holds and the object store it reads |
//!
//! Every part opens with `use crate::evidence::*;`, so the list below is the
//! module's single import list.

pub(crate) use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
pub(crate) use std::fs::{self, File, OpenOptions};
pub(crate) use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
pub(crate) use std::path::{Component, Path, PathBuf};
pub(crate) use std::process::{Command, Stdio};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant, SystemTime};

pub(crate) use aes::cipher::{BlockEncrypt, KeyInit, KeyIvInit, StreamCipher};
pub(crate) use aes::Aes256;
pub(crate) use base64::engine::general_purpose::STANDARD as BASE64;
pub(crate) use base64::Engine;
pub(crate) use chrono::{DateTime, SecondsFormat, Utc};
pub(crate) use ctr::Ctr32BE;
pub(crate) use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
pub(crate) use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey};
pub(crate) use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
pub(crate) use ghash::universal_hash::UniversalHash;
pub(crate) use ghash::GHash;
pub(crate) use rand_core::{OsRng, RngCore};
pub(crate) use regex::Regex;
pub(crate) use serde_json::{Map, Value};
pub(crate) use sha2::{Digest, Sha256};
pub(crate) use subtle::ConstantTimeEq;
pub(crate) use url::Url;

pub(crate) use crate::failure::{now_iso, print_json, Answer, Failure};
pub(crate) use crate::manifest;

pub(crate) const MAGIC: &[u8] = b"PROBIERZ-EVIDENCE-1\n";
pub(crate) const TAG_BYTES: usize = 16;

mod basics;
mod bundle;
mod history;
mod publication;
mod receipt;
mod resources;

pub use bundle::*;
pub use history::*;
pub use publication::*;
pub use receipt::*;
pub use resources::*;
pub(crate) use basics::*;
