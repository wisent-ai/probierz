//! The evidence a scout reads when Trends and Competitors are not there: one
//! JSON file the person names, holding what those two would have answered
//! for the topic — the trend verdict with its arithmetic, the dated
//! observations, and the candidate products. Nothing in it is interpreted
//! a second time; a part that is missing or of the wrong shape is refused by
//! name, never read as empty.

use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use crate::failure::{Code, Failure};

const POINT: &str = "benchmark.scout";

/// The evidence file, read once and judged for the three parts it must hold.
pub(crate) struct Named {
    pub(crate) file: PathBuf,
    document: Json,
}

impl Named {
    pub(crate) fn read(file: &Path) -> Result<Self, Failure> {
        let text = std::fs::read_to_string(file).map_err(|error| {
            Failure::new(
                POINT,
                Code::Prerequisite,
                format!(
                    "the evidence file {} cannot be read: {error}",
                    file.display()
                ),
            )
        })?;
        let document: Json = serde_json::from_str(&text).map_err(|error| {
            Failure::config(
                POINT,
                format!("the evidence file {} is not JSON: {error}", file.display()),
            )
        })?;
        if !document.is_object() {
            return Err(Failure::config(
                POINT,
                format!(
                    "the evidence file {} must hold one object with trend, observations and candidates",
                    file.display()
                ),
            ));
        }
        Ok(Self {
            file: file.to_path_buf(),
            document,
        })
    }

    fn part(&self, name: &str) -> Result<&Json, Failure> {
        self.document.get(name).ok_or_else(|| {
            Failure::config(
                POINT,
                format!("the evidence file {} has no {name}", self.file.display()),
            )
        })
    }

    fn list(&self, name: &str) -> Result<Vec<Json>, Failure> {
        let part = self.part(name)?;
        let listed = part.as_array().ok_or_else(|| {
            Failure::config(
                POINT,
                format!(
                    "{name} in the evidence file {} must be a list",
                    self.file.display()
                ),
            )
        })?;
        if listed.is_empty() {
            return Err(Failure::new(
                POINT,
                Code::Refused,
                format!(
                    "{name} in the evidence file {} is empty",
                    self.file.display()
                ),
            ));
        }
        Ok(listed.clone())
    }

    /// The trend verdict for `topic`, as `trends detect` would answer it,
    /// with the topic named so a file for another topic is refused.
    pub(crate) fn trend(&self, topic: &str) -> Result<Json, Failure> {
        let trend = self.part("trend")?;
        if trend["topic"] != topic {
            return Err(Failure::config(
                POINT,
                format!(
                    "the evidence file {} holds the trend of topic {}, not {topic}",
                    self.file.display(),
                    trend["topic"]
                ),
            ));
        }
        for field in ["verdict", "evidence", "min_evidence", "recent", "baseline"] {
            if trend.get(field).is_none() {
                return Err(Failure::config(
                    POINT,
                    format!(
                        "the trend in the evidence file {} has no {field}",
                        self.file.display()
                    ),
                ));
            }
        }
        Ok(trend.clone())
    }

    /// The newest `limit` observations, as Trends would list them.
    pub(crate) fn observations(&self, limit: usize) -> Result<Vec<Json>, Failure> {
        let mut seen = self.list("observations")?;
        seen.truncate(limit);
        Ok(seen)
    }

    /// The candidate products, as Competitors would normalise them.
    pub(crate) fn candidates(&self) -> Result<Vec<Json>, Failure> {
        self.list("candidates")
    }
}
