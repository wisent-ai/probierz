use crate::apphooks::*;
use serde_json::json;

/// The schema of the reference strategy document this fixture writes.
const STRATEGY_SCHEMA_VERSION: u64 = 2;
pub(crate) fn strategy_document(run_id: &str) -> Value {
    let names = ["Activation", "Retention", "Revenue", "Reliability"];
    let metrics: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            json!({
                "id": deterministic_uuid(&["oko-e2e", run_id, "metric", &index.to_string()]),
                "slug": fixture_slug(run_id, "metric", Some(index)),
                "name": name,
                "description": format!("Probierz metric {}", index + 1),
                "owner": "Oko E2E",
                "horizon": "quarterly",
                "unit": "percent",
                "baselineValue": 0,
                "targetValue": 100,
                "currentValue": 25 * (index + 1),
                "source": format!("probierz:{run_id}"),
                "status": "measured"
            })
        })
        .collect();
    let pillar_names = ["Research", "Product", "Distribution", "Operations"];
    let pillars: Vec<Value> = pillar_names
        .iter()
        .enumerate()
        .map(|(index, title)| {
            json!({
                "id": deterministic_uuid(&["oko-e2e", run_id, "pillar", &index.to_string()]),
                "slug": fixture_slug(run_id, "pillar", Some(index)),
                "title": title,
                "owner": "Oko E2E",
                "role": format!("Probierz pillar {}", index + 1),
                "successCriteria": [format!("Metric {} is measured", index + 1)],
                "linkedProductSlugs": [fixture_slug(run_id, "product", Some(index))]
            })
        })
        .collect();
    let initiative_slugs: Vec<String> = (0..13)
        .map(|index| fixture_slug(run_id, "initiative", Some(index)))
        .collect();
    let product_names = ["Oko", "Platform", "Research", "Distribution"];
    let products: Vec<Value> = product_names
        .iter()
        .enumerate()
        .map(|(index, title)| {
            json!({
                "id": deterministic_uuid(&["oko-e2e", run_id, "product", &index.to_string()]),
                "slug": fixture_slug(run_id, "product", Some(index)),
                "title": title,
                "owner": "Oko E2E",
                "role": format!("Probierz product {}", index + 1),
                "linkedPillarSlugs": [fixture_slug(run_id, "pillar", Some(index))],
                "activeInitiativeSlugs": initiative_slugs.iter().enumerate().filter_map(|(candidate, slug)| (candidate % 4 == index).then_some(slug)).collect::<Vec<_>>(),
                "blockers": [],
                "customerRelevance": "Deterministic E2E evidence",
                "researchRelevance": "Deterministic E2E evidence"
            })
        })
        .collect();
    let initiatives: Vec<Value> = initiative_slugs
        .iter()
        .enumerate()
        .map(|(index, slug)| {
            json!({
                "id": deterministic_uuid(&["oko-e2e", run_id, "initiative", &index.to_string()]),
                "slug": slug,
                "title": format!("Probierz initiative {}", index + 1),
                "owner": "Oko E2E",
                "status": "in_progress",
                "successMetric": names[index % names.len()],
                "linkedProductSlugs": [fixture_slug(run_id, "product", Some(index % 4))],
                "linkedPillarSlugs": [fixture_slug(run_id, "pillar", Some(index % 4))],
                "activeConversationSources": [format!("slack:{run_id}")],
                "artifactSlugs": [fixture_slug(run_id, "run-receipt", None)],
                "nextActions": [format!("Complete deterministic step {}", index + 1)],
                "targetDate": "2026-12-31",
                "budgetUSD": 1000 + index,
                "dependencySlugs": if index == 0 { Vec::<String>::new() } else { vec![initiative_slugs[index - 1].clone()] },
                "outcomeMetricSlugs": [fixture_slug(run_id, "metric", Some(index % names.len()))],
                "priority": 100 - index,
                "capacityPercent": if index == 12 { 10.0 } else { 7.5 },
                "planningStatus": "approved"
            })
        })
        .collect();
    json!({
        "schemaVersion": STRATEGY_SCHEMA_VERSION,
        "northStar": {
            "statement": format!("Probierz Oko reference strategy [{run_id}]"),
            "marketThesis": "Deterministic product evidence is a release requirement.",
            "whyWisentWins": "The product connects strategy, conversations, and execution.",
            "mustBecomeTrue": ["Every critical journey has current evidence."],
            "nonCriticalWork": ["Unseeded cosmetic variation."],
            "metrics": metrics
        },
        "pillars": pillars,
        "products": products,
        "initiatives": initiatives,
        "artifacts": [{
            "id": deterministic_uuid(&["oko-e2e", run_id, "artifact", "0"]),
            "slug": fixture_slug(run_id, "run-receipt", None),
            "title": "Probierz run receipt",
            "kind": "evidence",
            "owner": "Oko E2E",
            "location": format!("probierz:{run_id}"),
            "status": "active",
            "supportsPillarSlugs": pillars.iter().filter_map(|item| item.get("slug")).cloned().collect::<Vec<_>>(),
            "supportsProductSlugs": products.iter().filter_map(|item| item.get("slug")).cloned().collect::<Vec<_>>(),
            "supportsInitiativeSlugs": initiative_slugs
        }],
        "decisions": [{
            "id": deterministic_uuid(&["oko-e2e", run_id, "decision", "0"]),
            "decidedOn": "2026-07-13",
            "title": "Require deterministic Oko evidence",
            "decision": "Release only with current Probierz receipts.",
            "rationale": format!("Seeded by {run_id}"),
            "owner": "Oko E2E",
            "affectedProductSlugs": [fixture_slug(run_id, "product", Some(0))],
            "affectedInitiativeSlugs": [fixture_slug(run_id, "initiative", Some(0))],
            "reversibility": "reversible"
        }]
    })
}

pub(crate) fn feedback_decision(run_id: &str) -> Value {
    json!({
        "isDecision": true,
        "title": format!("Accept Probierz feedback [{run_id}]"),
        "decision": format!("The deterministic Slack correction for {run_id} is accepted."),
        "rationale": "The correction is explicit, scoped to the synthetic organization, and reversible.",
        "affectedProductSlugs": [fixture_slug(run_id, "product", Some(0))],
        "affectedInitiativeSlugs": [fixture_slug(run_id, "initiative", Some(0))],
        "reversibility": "reversible",
        "mutations": [{
            "entity": "north_star",
            "slug": "",
            "field": "statement",
            "stringValue": format!("Probierz Slack feedback applied [{run_id}]")
        }]
    })
}

pub(crate) fn technical_email(email: &str) -> Result<String, Failure> {
    let email = email.to_ascii_lowercase();
    // The local part is what precedes the address's first `@`; a text
    // without one is all local part, which the check below then judges.
    let local = match email.split_once('@') {
        Some((local, _domain)) => local,
        None => email.as_str(),
    };
    if !local.contains("e2e") && !local.contains("probierz") {
        return Err(Failure::config(
            "apphook.oko.email",
            "OKO_E2E_EMAIL must be a dedicated address containing 'e2e' or 'probierz'",
        ));
    }
    Ok(email)
}

pub(crate) fn scoped_oko_source(
    source: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, Failure> {
    let email = configured(source, "OKO_E2E_EMAIL")?.to_ascii_lowercase();
    let Some((address, domain)) = email
        .split_once('@')
        .filter(|(address, domain)| !address.is_empty() && !domain.is_empty())
    else {
        return Err(Failure::config(
            "apphook.oko.email",
            format!("OKO_E2E_EMAIL must be a valid technical email address, found {email:?}"),
        ));
    };
    // A sub-address `+tag` is replaced by this run's own tag.
    let base = match address.split_once('+') {
        Some((base, _tag)) => base,
        None => address,
    };
    // The run id makes the address this run's own; without it every run
    // would share one account, so a missing id is refused, never hashed as "".
    let run_id = configured(source, "PROBIERZ_RUN_ID")?;
    let mut scoped = source.clone();
    scoped.insert(
        "OKO_E2E_EMAIL".into(),
        format!("{base}+probierz-{}@{domain}", hash12(run_id)),
    );
    Ok(scoped)
}
