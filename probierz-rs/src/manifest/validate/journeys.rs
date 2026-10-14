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

/// Every journey a manifest declares, and what its drivers can actually do.
pub(crate) fn validate_journeys(
    document: &Value,
    file: &Path,
    surfaces: &serde_yaml::Mapping,
    journeys: &serde_yaml::Mapping,
) -> Answer {
    for (journey_key, journey) in journeys {
        let name = journey_key.as_str().unwrap_or_default();
        require(
            journey.as_mapping().is_some(),
            file,
            &format!("journey {name} must be an object"),
        )?;
        require(
            !string_of(journey, "owner").unwrap_or_default().is_empty(),
            file,
            &format!("journey {name} owner is required"),
        )?;
        // No cutoff is required: `probierz run` waits for a suite to its own
        // end, and a `timeoutMs` a manifest still carries is read by nothing.
        let identity = serde_yaml::from_value::<Identity>(journey.clone());
        if let Err(error) = &identity {
            require(
                false,
                file,
                &format!("journey {name} identity does not parse: {error}"),
            )?;
        }
        let has_identity = identity.is_ok_and(|identity| identity.declared());
        if name == "onboarding-first-use" || has_identity {
            require(
                !string_of(journey, "journeyId")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} journeyId is required"),
            )?;
            require(
                !string_of(journey, "journeyVersion")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} journeyVersion is required"),
            )?;
            require(
                is_uuid(string_of(journey, "journeyVersionId").unwrap_or_default()),
                file,
                &format!("journey {name} journeyVersionId must be a UUID"),
            )?;
            require(
                !string_of(journey, "firstSuccessFact")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} firstSuccessFact is required"),
            )?;
        }
        if name == "onboarding-first-use" {
            require(
                map_of(journey, "publication").is_some(),
                file,
                &format!("journey {name} publication is required"),
            )?;
        }
        if let Some(publication) = journey.get("publication") {
            require(
                has_identity || name == "onboarding-first-use",
                file,
                &format!("journey {name} publication requires immutable journey identity"),
            )?;
            require(
                !string_of(document, "productId")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} publication requires productId"),
            )?;
            require(
                !string_of(publication, "screenId")
                    .unwrap_or_default()
                    .is_empty(),
                file,
                &format!("journey {name} publication.screenId is required"),
            )?;
            let kinds: Vec<String> = sequence_of(publication, "artifactKinds")
                .map(|list| {
                    list.iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
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
            require(
                matches!(string_of(publication, "minimumEvidence"), Some("E2" | "E3")),
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
        let name = journey_key.as_str().unwrap_or_default();
        let claims_recording = journey
            .get("publication")
            .and_then(|publication| sequence_of(publication, "artifactKinds"))
            .map(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("recording")))
            .unwrap_or(false);
        if !claims_recording {
            continue;
        }
        let serving: Vec<String> = surfaces
            .iter()
            .filter(|(_, surface)| {
                sequence_of(surface, "journeys")
                    .map(|list| list.iter().any(|entry| entry.as_str() == Some(name)))
                    .unwrap_or(false)
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
