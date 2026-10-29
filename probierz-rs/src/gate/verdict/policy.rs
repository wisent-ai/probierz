use crate::gate::*;

/// A gate reads only requirements the application's manifest declares.
pub(super) struct Policy<'a> {
    document: &'a Yaml,
    location: String,
}

impl<'a> Policy<'a> {
    pub(super) fn read(app: &'a manifest::Manifest, mode: &str) -> Result<Self, Failure> {
        let key = match mode {
            "pull-request" => "pullRequestPolicy",
            "release" => "releasePolicy",
            other => {
                return Err(Failure::invalid(
                    "gate.evaluate",
                    format!("unsupported gate mode: {other}"),
                ))
            }
        };
        let location = format!("{} {key}", app.file.display());
        let document = yaml_get(&app.document, key).ok_or_else(|| {
            Failure::config(
                "gate.evaluate",
                format!("{location} is missing; declare the policy before evaluating this mode"),
            )
        })?;
        if !document.is_mapping() {
            return Err(Failure::config(
                "gate.evaluate",
                format!("{location} must be a mapping, found {document:?}"),
            ));
        }
        Ok(Self { document, location })
    }

    fn malformed(&self, key: &str, expected: &str, value: &Yaml) -> Failure {
        Failure::config(
            "gate.evaluate",
            format!(
                "{}.{key} must be {expected}, found {value:?}",
                self.location
            ),
        )
    }

    pub(super) fn text(&self, key: &str) -> Result<Option<String>, Failure> {
        match yaml_get(self.document, key) {
            None => Ok(None),
            Some(Yaml::String(text)) if !text.trim().is_empty() => Ok(Some(text.clone())),
            Some(value) => Err(self.malformed(key, "non-empty text", value)),
        }
    }

    pub(super) fn minimum_evidence(&self) -> Result<(String, i32), Failure> {
        let level = self.text("minimumEvidence")?.ok_or_else(|| {
            Failure::config(
                "gate.evaluate",
                format!(
                    "{}.minimumEvidence is missing; declare the required E<n> evidence level",
                    self.location
                ),
            )
        })?;
        let rank = evidence_rank(&level).ok_or_else(|| {
            Failure::config(
                "gate.evaluate",
                format!(
                    "{}.minimumEvidence is not an E<n> evidence level: {level}",
                    self.location
                ),
            )
        })?;
        Ok((level, rank))
    }

    /// An absent optional requirement declares none; a malformed one is refused.
    pub(super) fn texts(&self, key: &str) -> Result<Vec<String>, Failure> {
        let list = match yaml_get(self.document, key) {
            None => return Ok(Vec::new()),
            Some(Yaml::Sequence(list)) => list,
            Some(value) => {
                return Err(self.malformed(key, "a list of non-empty text values", value))
            }
        };
        list.iter()
            .map(|value| match value {
                Yaml::String(text) if !text.trim().is_empty() => Ok(text.clone()),
                other => Err(self.malformed(key, "a list of non-empty text values", other)),
            })
            .collect()
    }

    pub(super) fn flag(&self, key: &str) -> Result<bool, Failure> {
        match yaml_get(self.document, key) {
            None => Ok(false),
            Some(Yaml::Bool(value)) => Ok(*value),
            Some(value) => Err(self.malformed(key, "a boolean", value)),
        }
    }
}
