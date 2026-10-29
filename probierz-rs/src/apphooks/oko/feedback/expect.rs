//! What a run's fixture state says the Oko tables and the Slack feedback
//! event hold, read from the state the hooks wrote, never restated here.

use crate::apphooks::*;

/// A field the Slack fixture recorded, or a failure naming it. An absent
/// field used to be written as null into the feedback event, so the RPC
/// applied feedback about no message and the run still counted it applied.
pub(super) fn slack_field(slack: &Value, field: &str) -> Result<Value, Failure> {
    match slack.get(field) {
        Some(value) if !value.is_null() => Ok(value.clone()),
        _ => Err(Failure::config(
            "apphook.oko.feedback",
            format!("the Slack feedback fixture state names no {field}; run the Slack fixture hook first"),
        )),
    }
}

/// What the strategy seeded for this run says the tables hold.
pub(super) struct Seeded {
    pub(super) metrics: usize,
    pub(super) initiatives: usize,
    pub(super) capacity: f64,
}

fn seeded_list<'a>(strategy: &'a Value, pointer: &str) -> Result<&'a Vec<Value>, Failure> {
    strategy
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Failure::config(
                "apphook.oko.state",
                format!("the seeded strategy in the Oko fixture state holds no {pointer} list"),
            )
        })
}

/// The seeded strategy's metric and initiative counts and the capacity its
/// initiatives sum to, from the state the setup hook wrote.
pub(super) fn seeded(state: &Value) -> Result<Seeded, Failure> {
    let strategy = state.pointer("/strategy/initial").ok_or_else(|| {
        Failure::config(
            "apphook.oko.state",
            "the Oko fixture state holds no seeded strategy; run the fixture setup hook first",
        )
    })?;
    let initiatives = seeded_list(strategy, "/initiatives")?;
    Ok(Seeded {
        metrics: seeded_list(strategy, "/northStar/metrics")?.len(),
        initiatives: initiatives.len(),
        capacity: capacity_sum(initiatives, "capacityPercent", "the seeded initiative")?,
    })
}

/// The capacity percentages `rows` state under `field`, summed in ascending
/// order, so the same percentages give the same sum whatever order the
/// table or the seed lists them in; a row that states none is refused naming
/// it, never counted as no capacity.
pub(super) fn capacity_sum(rows: &[Value], field: &str, what: &str) -> Result<f64, Failure> {
    let mut stated = rows
        .iter()
        .map(|row| {
            row.get(field).and_then(Value::as_f64).ok_or_else(|| {
                Failure::unavailable(
                    "apphook.oko.verify",
                    format!("{what} {row} states no numeric {field}"),
                )
            })
        })
        .collect::<Result<Vec<f64>, Failure>>()?;
    stated.sort_by(f64::total_cmp);
    Ok(stated.into_iter().sum())
}
