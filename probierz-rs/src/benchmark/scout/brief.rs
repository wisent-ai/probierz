//! What the model is asked while scouting, and how each answer is judged.
//! A model only reads what it was given: every product it names cites the
//! observations it was seen in, every rival is one of the candidates
//! competitors-cli made, and anything else sends the draft back.

use std::collections::BTreeSet;

use serde_json::{json, Value as Json};

use super::{identity, BRIEF_SCHEMA};

/// Stado's bound on a product description.
const DESCRIPTION_BYTES: usize = 350;

fn parsed(content: &str) -> Result<Json, String> {
    serde_json::from_str(content).map_err(|error| format!("the answer is not one JSON document: {error}"))
}

fn ids(seen: &[Json]) -> BTreeSet<&str> {
    seen.iter().filter_map(|entry| entry["id"].as_str()).collect()
}

fn rejected_part(rejected: Option<(&str, &str)>) -> String {
    match rejected {
        Some((draft, why)) => format!("\n\nYour previous answer was:\n{draft}\nIt was refused: {why}\nAnswer again, fixing that."),
        None => String::new(),
    }
}

/// Every observation id an answer cites, refused when any is unknown or
/// none is given.
fn cited<'a>(value: &'a Json, seen: &[Json], what: &str) -> Result<Vec<&'a str>, String> {
    let known = ids(seen);
    let cited: Vec<&str> = value.as_array().into_iter().flatten().filter_map(Json::as_str).collect();
    if cited.is_empty() {
        return Err(format!("{what} cites no observation id"));
    }
    match cited.iter().find(|id| !known.contains(**id)) {
        Some(unknown) => Err(format!("{what} cites {unknown}, which is not one of the observation ids given")),
        None => Ok(cited),
    }
}

pub(super) fn products(topic: &str, seen: &[Json], rejected: Option<(&str, &str)>) -> String {
    format!(
        "Trends recorded these observations for the topic {topic}, newest first:\n{}\n\n\
         Name every product, project or company these observations themselves show serving {topic}. \
         Never add one from memory, and leave out observations that name no product.\n\
         Answer one JSON document: {{\"products\": [{{\"name\", \"domain\", \"url\", \"description\", \"observations\"}}]}} where \
         domain is the product's own web domain, or for a product that only has a code repository its repository path such as github.com/owner/name; \
         url is the https address of its own site or repository; description is one sentence the observations support; \
         observations lists the ids of the observations that show it.{}",
        serde_json::to_string_pretty(seen).unwrap_or_default(),
        rejected_part(rejected)
    )
}

/// The products an answer names, as competitors-cli discovery records.
pub(super) fn records(content: &str, topic: &str, seen: &[Json]) -> Result<Vec<Json>, String> {
    let answer = parsed(content)?;
    let products = answer["products"].as_array().filter(|list| !list.is_empty())
        .ok_or("the answer names no product under \"products\"")?;
    let mut records = Vec::new();
    for product in products {
        let text = |key: &str| product[key].as_str().map(str::trim).unwrap_or_default().to_string();
        let name = text("name");
        if name.is_empty() || text("domain").is_empty() || text("description").is_empty() {
            return Err(format!("product {product} lacks a name, domain or description"));
        }
        if !text("url").starts_with("https://") {
            return Err(format!("product {name} has url {:?}; it must be an https address", text("url")));
        }
        let cited = cited(&product["observations"], seen, &format!("product {name}"))?;
        let first = seen.iter().find(|entry| entry["id"] == cited[0]).expect("cited ids are known");
        records.push(json!({
            "name": name, "domain": text("domain"), "url": text("url"), "description": text("description"),
            "evidenceUrl": first["url"], "observedAt": first["published_at"], "query": topic,
            "attributes": {"observations": cited},
        }));
    }
    Ok(records)
}

/// A candidate's id as a Stado identity, which is also its contender id.
fn rival_id(candidate: &Json) -> String {
    let raw = candidate["id"].as_str().unwrap_or_default().to_ascii_lowercase();
    let mapped: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let trimmed = mapped.trim_matches('-').to_string();
    if trimmed.starts_with(|c: char| c.is_ascii_lowercase()) { trimmed } else { format!("rival-{trimmed}") }
}

pub(super) fn opportunity(topic: &str, trend: &Json, candidates: &[Json], ours: &[Json], rejected: Option<(&str, &str)>) -> String {
    let named: Vec<Json> = candidates.iter().map(|candidate| json!({
        "id": rival_id(candidate), "name": candidate["name"], "domains": candidate["domains"],
        "description": candidate["description"],
        "observations": candidate["evidence"].as_array().into_iter().flatten()
            .flat_map(|evidence| evidence["attributes"]["observations"].as_array().cloned().unwrap_or_default())
            .collect::<Vec<_>>(),
    })).collect();
    let catalog: Vec<Json> = ours.iter().map(|entry| json!({"id": entry["id"], "name": entry["name"], "description": entry["description"]})).collect();
    format!(
        "Topic {topic}. Trends measured: {}\n\nProducts already serving it (candidates):\n{}\n\nOur existing products:\n{}\n\n\
         Propose the one new product we should build so that it beats these candidates, measured on the same benchmark cases. \
         It must not duplicate one of our existing products.\n\
         Answer one JSON document: {{\"product\": {{\"id\", \"name\", \"description\"}}, \"surface\", \"gap\", \"rivals\", \"suite\", \"observations\"}} where \
         product.id is lowercase letters, digits and inner hyphens and is none of our ids; description is at most {DESCRIPTION_BYTES} bytes; \
         surface is the first surface it ships, one of cli, desktop, web or service; gap says what it will do that none of the rivals does; \
         rivals lists the candidate ids it must beat; suite is the id of the benchmark suite that measures them; observations lists the ids the gap rests on.{}",
        json!({"verdict": trend["verdict"], "recent": trend["recent"], "baseline": trend["baseline"], "window_days": trend["window_days"]}),
        serde_json::to_string_pretty(&named).unwrap_or_default(),
        serde_json::to_string_pretty(&catalog).unwrap_or_default(),
        rejected_part(rejected)
    )
}

/// The opportunity an answer proposes, judged against what it was given.
pub(super) fn opportunity_of(content: &str, candidates: &[Json], seen: &[Json], ours: &[Json]) -> Result<Json, String> {
    let answer = parsed(content)?;
    let text = |value: &Json| value.as_str().map(str::trim).unwrap_or_default().to_string();
    let id = text(&answer["product"]["id"]);
    if !identity(&id) {
        return Err(format!("product.id {id:?} is not lowercase letters, digits and inner hyphens starting with a letter"));
    }
    if ours.iter().any(|entry| entry["id"] == id.as_str()) {
        return Err(format!("product.id {id} is already one of our products"));
    }
    let description = text(&answer["product"]["description"]);
    if text(&answer["product"]["name"]).is_empty() || description.is_empty() || description.len() > DESCRIPTION_BYTES {
        return Err(format!("product needs a name and a description of 1 to {DESCRIPTION_BYTES} bytes"));
    }
    if text(&answer["surface"]).is_empty() || text(&answer["gap"]).is_empty() {
        return Err("the answer needs a surface and a gap".to_string());
    }
    let suite = text(&answer["suite"]);
    if !identity(&suite) {
        return Err(format!("suite {suite:?} is not lowercase letters, digits and inner hyphens starting with a letter"));
    }
    let known: Vec<String> = candidates.iter().map(rival_id).collect();
    let rivals: Vec<String> = answer["rivals"].as_array().into_iter().flatten().map(text).collect();
    if rivals.is_empty() {
        return Err("rivals names no candidate: a product is measured against at least one".to_string());
    }
    if let Some(stranger) = rivals.iter().find(|rival| !known.contains(rival) || **rival == id) {
        return Err(format!("rival {stranger} is not one of the candidate ids given, or is the product itself"));
    }
    if rivals.iter().collect::<BTreeSet<_>>().len() != rivals.len() {
        return Err("rivals names a candidate twice".to_string());
    }
    cited(&answer["observations"], seen, "the gap")?;
    Ok(answer)
}

/// What a scout saw before it decided.
pub(super) struct Scouted<'a> {
    pub id: &'a str,
    pub topic: &'a str,
    pub owner: &'a str,
    pub trend: &'a Json,
    pub seen: &'a [Json],
    pub candidates: &'a [Json],
}

/// The brief: the evidence, the decision, the catalog rivals and benchmark
/// it declares, and the Stado creation request that adopts it.
pub(super) fn document(scouted: &Scouted, opportunity: &Json, drafts: Json) -> Json {
    let product = &opportunity["product"];
    let id = product["id"].as_str().unwrap_or_default();
    let rivals: Vec<Json> = opportunity["rivals"].as_array().into_iter().flatten().filter_map(|rival| {
        let candidate = scouted.candidates.iter().find(|candidate| rival_id(candidate) == rival.as_str().unwrap_or_default())?;
        let evidence = &candidate["evidence"][0];
        Some(json!({"id": rival_id(candidate), "name": candidate["name"], "url": evidence["url"], "evidence": evidence["evidenceUrl"]}))
    }).collect();
    let cited: BTreeSet<&str> = opportunity["observations"].as_array().into_iter().flatten().filter_map(Json::as_str).collect();
    let evidence_refs: Vec<&Json> = scouted.seen.iter().filter(|entry| entry["id"].as_str().is_some_and(|seen| cited.contains(seen))).map(|entry| &entry["url"]).collect();
    json!({
        "schema": BRIEF_SCHEMA,
        "briefId": scouted.id,
        "topic": scouted.topic,
        "createdAt": crate::failure::now_iso(),
        "trend": scouted.trend,
        "observations": scouted.seen,
        "candidates": scouted.candidates,
        "gap": opportunity["gap"],
        "rivals": rivals,
        "benchmark": {"app": id, "suites": [opportunity["suite"]]},
        "creation": {
            "schema_version": 1,
            "request_id": scouted.id,
            "initiative_id": format!("scout-{}", scouted.topic),
            "product": {"id": id, "name": product["name"], "description": product["description"], "family": "wisent", "visibility": "private"},
            "repositories": [{"surface": opportunity["surface"], "repository": format!("{}/{id}", scouted.owner)}],
            "evidence_refs": evidence_refs,
        },
        "drafts": drafts,
    })
}
