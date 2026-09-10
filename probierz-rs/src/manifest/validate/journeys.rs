use crate::manifest::*;

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
        let timeout = journey
            .get("timeoutMs")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        require(
            timeout > 0.0,
            file,
            &format!("journey {name} timeoutMs must be positive"),
        )?;
        let has_identity = [
            "journeyId",
            "journeyVersion",
            "journeyVersionId",
            "firstSuccessFact",
        ]
        .iter()
        .any(|field| journey.get(*field).is_some());
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
            let evidence = string_of(publication, "minimumEvidence").unwrap_or_default();
            require(
                matches!(evidence, "E2" | "E3"),
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
