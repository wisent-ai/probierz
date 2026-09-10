//! Authoring, model-backed evaluation, source identity, and accessibility.
//!
//! Model work crosses one boundary only: the authenticated Stado router.  This
//! module never reads provider credentials and never accepts them as arguments.
//!
//! | part | what it owns |
//! |---|---|
//! | [`source`] | which files are a product's source, the identity they hash to, and the accessibility identifiers in them |
//! | [`router`] | the one authenticated boundary model work crosses |
//! | [`spec`] | drafting, installing and running an authored spec, and the manifest around it |
//! | [`figure`] | rendering a figure pair and scoring it against a rubric |
//! | [`seo`] | the declared SEO contract, the crawl, and the signed verdict |
//! | [`repair`] | a recorded failure turned into a briefed, verified repair |
//! | [`result`] | printing what any of the above decided |
//!
//! Every part opens with `use crate::authoring::*;`, so the list below is the
//! module's single import list.

pub(crate) use crate::failure::{print_json, Failure};
pub(crate) use std::collections::{BTreeMap, BTreeSet};
pub(crate) use std::ffi::OsStr;
pub(crate) use std::fs::{self, File};
pub(crate) use std::io::Write;
pub(crate) use std::os::unix::fs::{MetadataExt, PermissionsExt};
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::process::{Command, Stdio};
pub(crate) use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use base64::Engine;
pub(crate) use ed25519_dalek::pkcs8::{spki::der::pem::LineEnding, DecodePrivateKey, EncodePublicKey};
pub(crate) use ed25519_dalek::{Signer, SigningKey};

pub(crate) use serde_json::{Map, Value as JsonValue};
pub(crate) use serde_yaml::Value as YamlValue;
pub(crate) use sha2::{Digest, Sha256};

pub(crate) use crate::manifest;

mod figure;
mod repair;
mod result;
mod router;
mod seo;
mod source;
mod spec;

pub use figure::*;
pub use repair::*;
pub use result::*;
pub use seo::*;
pub use source::*;
pub use spec::*;
pub(crate) use router::*;
