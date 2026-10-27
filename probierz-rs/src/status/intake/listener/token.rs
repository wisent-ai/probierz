//! Where the intake keeps what its clients read: the bearer token and the
//! address it listens on, both in one owner-only `~/.probierz` directory.

use std::fs;
use std::path::PathBuf;

use base64::Engine;
use rand_core::RngCore;

use crate::failure::{write_private, Failure};

/// How many random bytes a token holds, a whole number of base64 groups so
/// it encodes without padding (RFC 4648 section 4,
/// https://www.rfc-editor.org/rfc/rfc4648#section-4).
const TOKEN_BYTES: usize = 24;

/// The intake's directory, created owner-only when it is not there, so the
/// token and the address are written beside each other whichever of them is
/// written first. A token from the environment writes no token file, and the
/// address file still has its directory.
pub(crate) fn intake_directory() -> Result<PathBuf, Failure> {
    let directory = crate::run::home_dir().join(".probierz");
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        // Owner read, write and search only (POSIX mode bits,
        // https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_stat.h.html).
        builder.mode(0o700);
    }
    builder.create(&directory).map_err(|error| {
        Failure::config(
            "intake.directory",
            format!(
                "{} could not be created, so the intake has nowhere to keep its token \
                 and address: {error}",
                directory.display()
            ),
        )
    })?;
    Ok(directory)
}

fn random_token() -> Result<String, Failure> {
    let mut bytes: [u8; TOKEN_BYTES] = Default::default();
    rand_core::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| {
            Failure::unavailable(
                "intake.token",
                format!("the system random source refused a new token: {error}"),
            )
        })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// The bearer token the intake accepts, whether it was minted now, and the
/// token file. `PROBIERZ_INTAKE_TOKEN` wins when it is set; set but empty or
/// not text it is refused naming the variable. Otherwise the token file is
/// read; only one that is not there, or holds nothing, is minted anew. One
/// that exists and cannot be read is refused: replacing it would cut off
/// every client already paired with the token it holds.
pub(crate) fn intake_token() -> Result<(String, bool, PathBuf), Failure> {
    let file = intake_directory()?.join("intake-token");
    match std::env::var("PROBIERZ_INTAKE_TOKEN") {
        Ok(token) if token.trim().is_empty() => {
            return Err(Failure::config(
                "intake.token",
                "PROBIERZ_INTAKE_TOKEN is set but empty; set it to the token clients send, \
                 or unset it to use the token file",
            ))
        }
        Ok(token) => return Ok((token.trim().to_string(), false, file)),
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(Failure::config(
                "intake.token",
                "PROBIERZ_INTAKE_TOKEN is not text, so no client could send it",
            ))
        }
    }
    match fs::read_to_string(&file) {
        Ok(existing) if !existing.trim().is_empty() => {
            return Ok((existing.trim().to_string(), false, file))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Failure::config(
                "intake.token",
                format!(
                    "cannot read the intake token {}, so it is not replaced: {error}",
                    file.display()
                ),
            ))
        }
    }
    let token = random_token()?;
    write_private(&file, format!("{token}\n").as_bytes()).map_err(|error| {
        Failure::config(
            "intake.token",
            format!("{} could not be written: {error}", file.display()),
        )
    })?;
    Ok((token, true, file))
}
