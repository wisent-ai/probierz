//! The benchmark against the product catalog Stado serves.
//!
//! The catalog says who a product's rivals are, which Probierz suites
//! measure them and what the product's roadmap holds. [`rivals`] confronts
//! that declaration with the manifest; [`roadmap`] turns the newest run's
//! losses into roadmap items and records a case ours now wins as delivered;
//! [`ledger`] is the one writer every roadmap item goes through, the
//! benchmark's and the incident feedback's alike.

use std::process::Command;

use serde_json::Value as Json;

use crate::failure::{ended, Code, Failure};
use crate::manifest::Manifest;
use crate::stado::STADO_BIN;

mod ledger;
mod rivals;
mod roadmap;

pub(crate) use ledger::{delivered, is_open, titled, write_roadmap, PLANNED};
pub(crate) use rivals::rivals;
pub(crate) use roadmap::{roadmap, roadmap_of};

/// The catalog id of the product a manifest describes.
pub(crate) fn product_id(manifest: &Manifest) -> String {
    match manifest
        .document
        .get("productId")
        .and_then(serde_yaml::Value::as_str)
    {
        Some(id) => id.to_string(),
        None => manifest.app_id.clone(),
    }
}

/// Run one Stado command and answer its stdout, or say what it refused.
pub(crate) fn stado(command: &mut Command) -> Result<Vec<u8>, Failure> {
    let described = format!("{command:?}");
    let output = stado_wait::output(command).map_err(|error| {
        Failure::new(
            "benchmark.catalog",
            Code::Prerequisite,
            format!("{described} could not be started ({error}); install Stado, which serves the product catalog"),
        )
    })?;
    if !output.status.success() {
        // Stado's sentence first: the failure line is cut for a person, and a
        // long argv ahead of it hid the reason. The command is named by its
        // words before the first option: the subcommand and its operands.
        let verb: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .take_while(|arg| !arg.starts_with('-'))
            .collect();
        return Err(Failure::new(
            "benchmark.catalog",
            Code::Unavailable,
            format!(
                "{} (stado {} {})",
                String::from_utf8_lossy(&output.stderr).trim(),
                verb.join(" "),
                ended(&output.status),
            ),
        ));
    }
    Ok(output.stdout)
}

/// Every product record in the catalog Stado serves.
pub(crate) fn products() -> Result<Vec<Json>, Failure> {
    let mut read = Command::new(STADO_BIN);
    read.arg("product").arg("catalog").arg("--json");
    let mut catalog: Json = serde_json::from_slice(&stado(&mut read)?).map_err(|error| {
        Failure::config(
            "benchmark.catalog",
            format!("stado product catalog --json is not JSON: {error}"),
        )
    })?;
    match catalog["products"].take() {
        Json::Array(products) => Ok(products),
        _ => Err(Failure::config(
            "benchmark.catalog",
            "stado product catalog --json carries no products list",
        )),
    }
}

/// One product's record in the catalog Stado serves.
pub(crate) fn record(product: &str) -> Result<Json, Failure> {
    products()?
        .into_iter()
        .find(|entry| entry["id"] == product)
        .ok_or_else(|| {
            Failure::config(
                "benchmark.catalog",
                format!("the product catalog has no product {product}; the manifest's productId names it"),
            )
        })
}
