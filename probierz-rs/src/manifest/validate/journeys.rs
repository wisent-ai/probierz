use crate::manifest::*;

/// The immutable identity a journey may declare; any one of its fields
/// declared makes every one of them required.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    journey_id: Option<Value>,
    journey_version: Option<Value>,
    journey_version_id: Option<Value>,
    first_success_fact: Option<Value>,
}

impl Identity {
    fn declared(&self) -> bool {
        self.journey_id.is_some()
            || self.journey_version.is_some()
            || self.journey_version_id.is_some()
            || self.first_success_fact.is_some()
    }
}

/// Whether `key` holds non-empty text in `value`.
fn stated(value: &Value, key: &str) -> bool {
    string_of(value, key).is_some_and(|text| !text.is_empty())
}

/// The refusal `require` gives, for a fault found without a condition.
fn invalid(file: &Path, what: &str) -> Failure {
    Failure::new(
        "manifest.validate",
        Code::Config,
        format!("invalid app manifest: {} {what}", file.display()),
    )
}

/// A journey's name: its key, which a manifest writes as text.
fn journey_name<'a>(key: &'a Value, file: &Path) -> Result<&'a str, Failure> {
    key.as_str()
        .ok_or_else(|| invalid(file, &format!("a journey is keyed by {key:?}, not a name")))
}

/// Every journey a manifest declares, and what its drivers can actually do.
pub(crate) fn validate_journeys(
    document: &Value,
    file: &Path,
    surfaces: &serde_yaml::Mapping,
    journeys: &serde_yaml::Mapping,
) -> Answer {
    for (journey_key, journey) in journeys {
        let name = journey_name(journey_key, file)?;
        require(
            journey.as_mapping().is_some(),
            file,
            &format!("journey {name} must be an object"),
        )?;
        require(
            stated(journey, "owner"),
            file,
            &format!("journey {name} owner is required"),
        )?;
        // No cutoff is required: `probierz run` waits for a suite to its own
        // end, and a `timeoutMs` a manifest still carries is read by nothing.
        let has_identity = serde_yaml::from_value::<Identity>(journey.clone())
            .map_err(|error| {
                invalid(
                    file,
                    &format!("journey {name} identity does not parse: {error}"),
                )
            })?
            .declared();
        let first_use = name == "onboarding-first-use";
        if first_use || has_identity {
            require(
                stated(journey, "journeyId"),
                file,
                &format!("journey {name} journeyId is required"),
            )?;
            require(
                stated(journey, "journeyVersion"),
                file,
                &format!("journey {name} journeyVersion is required"),
            )?;
            require(
                string_of(journey, "journeyVersionId").is_some_and(is_uuid),
                file,
                &format!("journey {name} journeyVersionId must be a UUID"),
            )?;
            require(
                stated(journey, "firstSuccessFact"),
                file,
                &format!("journey {name} firstSuccessFact is required"),
            )?;
        }
        if first_use {
            require(
                map_of(journey, "publication").is_some(),
                file,
                &format!("journey {name} publication is required"),
            )?;
        }
        if let Some(publication) = journey.get("publication") {
            require(
                has_identity || first_use,
                file,
                &format!("journey {name} publication requires immutable journey identity"),
            )?;
            require(
                stated(document, "productId"),
                file,
                &format!("journey {name} publication requires productId"),
            )?;
            require(
                stated(publication, "screenId"),
                file,
                &format!("journey {name} publication.screenId is required"),
            )?;
            // Artifact kinds are names; a kind that is not one is refused, not
            // left out of the list it is checked against.
            let mut kinds: Vec<String> = Vec::new();
            for item in sequence_of(publication, "artifactKinds")
                .into_iter()
                .flatten()
            {
                let kind = item.as_str();
                require(
                    kind.is_some(),
                    file,
                    &format!("journey {name} publication artifact kind {item:?} is not a name"),
                )?;
                kinds.extend(kind.map(str::to_string));
            }
            require(
                !kinds.is_empty(),
                file,
                &format!("journey {name} publication.artifactKinds are required"),
            )?;
            let unique: BTreeSet<&String> = kinds.iter().collect();
            require(
                unique.len() == kinds.len(),
                file,
                &format!("journey {name} publication.artifactKinds must be unique"),
            )?;
            for kind in &kinds {
                require(
                    PUBLICATION_ARTIFACT_KINDS.contains(&kind.as_str()),
                    file,
                    &format!("journey {name} publication artifact kind {kind} is unsupported"),
                )?;
            }
            // Publication needs at least E2 evidence and can ask for no more
            // than E3, the ranks the gate reads.
            let rank =
                string_of(publication, "minimumEvidence").and_then(crate::gate::evidence_rank);
            require(
                rank.is_some()
                    && rank >= crate::gate::evidence_rank("E2")
                    && rank <= crate::gate::evidence_rank("E3"),
                file,
                &format!("journey {name} publication.minimumEvidence must be E2 or E3"),
            )?;
            require(
                publication
                    .get("redactionRequired")
                    .and_then(Value::as_bool)
                    .is_some(),
                file,
                &format!("journey {name} publication.redactionRequired must be boolean"),
            )?;
        }
    }

    // A journey may only claim a recording when at least one driver serving it
    // can make one.
    for (journey_key, journey) in journeys {
        let name = journey_name(journey_key, file)?;
        let claims_recording = journey
            .get("publication")
            .and_then(|publication| sequence_of(publication, "artifactKinds"))
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("recording")));
        if !claims_recording {
            continue;
        }
        let serving: Vec<String> = surfaces
            .iter()
            .filter(|(_, surface)| {
                sequence_of(surface, "journeys")
                    .is_some_and(|list| list.iter().any(|entry| entry.as_str() == Some(name)))
            })
            .filter_map(|(target, _)| target.as_str().map(str::to_string))
            .collect();
        require(
            serving
                .iter()
                .any(|target| target_supports_artifact_kind(target, "recording")),
            file,
            &format!("journey {name} claims recording but none of its drivers support recording"),
        )?;
    }

    Ok(())
}
