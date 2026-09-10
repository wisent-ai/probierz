use crate::evidence::*;
pub fn publication(
    harness: &Path,
    receipt_file: Option<&Path>,
    attempt_id: Option<&str>,
    journey_id: Option<&str>,
    assets_file: Option<&Path>,
    public_key: Option<&Path>,
    fingerprint: Option<&str>,
) -> Answer {
    let (receipt_file, attempt_id, journey_id, assets_file) =
        match (receipt_file, attempt_id, journey_id, assets_file) {
            (Some(receipt), Some(attempt), Some(journey), Some(assets)) => {
                (receipt, attempt, journey, assets)
            }
            _ => {
                return Err(Failure::invalid(
                    "evidence.publication",
                    "publication needs receipt, attemptId, journeyId, and --assets <json>",
                ))
            }
        };
    let assets = json_file(assets_file)?;
    let result = create_publication(
        harness,
        receipt_file,
        attempt_id,
        journey_id,
        &assets,
        public_key,
        fingerprint,
    )?;
    print_json(&result)
}

