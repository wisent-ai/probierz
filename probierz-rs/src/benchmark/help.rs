//! What `probierz benchmark --help` says after the command list.

pub const HELP: &str = "\
A benchmark runs our product and its rivals on the same versioned cases and
records who passed, how fast, and at what cost.

  probierz benchmark suites <app>
  probierz benchmark run <app> --suite <id> [--contender <id>]... [--repetitions N]
  probierz benchmark list <app> [--suite <id>] [--limit N]
  probierz benchmark show <app> <run-id>
  probierz benchmark compare <app> --baseline <run-id> --candidate <run-id>
  probierz benchmark standing <app> --suite <id>
  probierz benchmark rivals <app>
  probierz benchmark roadmap <app> --suite <id>
  probierz benchmark pursue <app> --suite <id> --case <id> --budget-usd <USD>
  probierz benchmark author-suite <app> --suite <id> --cases N --rounds N
  probierz benchmark author <app> --contender <id> --suite <id> [--ours] --rounds N
  probierz benchmark scout <topic> --owner <github-owner> --observations N --rounds N
  probierz benchmark adopt <brief.json> --allow-create --cases N --rounds N
  probierz benchmark cycle [--policy <autonomy.yaml>]
  probierz benchmark schedule --cron <expr> --host <host> --harness-dir <dir> [--secret-env NAME=ITEM#FIELD]... [--policy <file>]

The manifest declares `benchmark.suites.<id>: <suite.json>` and
`benchmark.contenders.<id>: {program, args, env, ours}`. A contender reads one
ai.wisent.probierz.benchmark.task.v1 document on stdin and writes one
ai.wisent.probierz.benchmark.result.v1 document on stdout. Its environment is
empty except for the variables its `env` names. A suite's `variables` map each
`${NAME}` placeholder in a case input to the variable Probierz fills it from.

The product catalog Stado serves names the product's rivals and the suites
that measure them. `rivals` refuses while a named rival has no contender or a
named suite is not declared. `roadmap` writes one catalog roadmap item per
case the newest run lost, and withdraws the item once ours wins that case.

Nothing in a benchmark is written by hand for one product. `author-suite`
drafts a suite from the catalog record of the product and its rivals through
the Stado model router, judges it and declares it; an existing suite file is
never overwritten. `author` drafts the driver of our contender (--ours) or of
a rival the catalog names, places it under benchmark/contenders/<id>/ or
benchmark/rivals/<id>/ in the product's tree, declares it, and verifies it
with a recorded run of that contender alone, redrafting while an attempt
breaks the contract or fails with an error. A declared driver is verified
first and redrafted only if it fails.

`pursue` hands one case the newest run lost to Jeden as a durable pursuit
request in our contender's checkout. Jeden's verdict does not close it:
Probierz then records a new run of the suite itself, accepts the case only
when ours wins or ties it in that run, and brings the roadmap in line with
that run. A pursuit that reports success while the case is still lost is
refused.

`scout` starts a product that does not exist yet. It reads the verdict Trends
measured for a watched topic (refused while the topic lacks its evidence
floor or is falling) and the observations behind it, asks the model router
which products those observations show, normalises them with `competitors
discover`, and asks for the product to build, the candidates it must beat
and its suite. Every product, rival and gap cites an observation id it was
given, or the draft is sent back. The brief lands once under
test-results/.scout/<topic>/ with the Stado creation request. `adopt` acts on
a brief: `stado product create` makes the private repository, its checkout
and a preview catalog record, the catalog names the rivals and the benchmark,
and Probierz declares the manifest and drafts the suite.

`cycle` is all of it without the operator, under the policy he wrote once in
autonomy.yaml (owner, productsPerWeek, observations, rounds, cases,
pursuitBudgetUsd, pursuitsPerProduct). It gives every catalog product without
one a Trends topic whose terms the model drafts from the product's record,
ingests, scouts every watched topic while the week's product limit allows,
has the model judge each brief and adopts the accepted ones, runs every suite
the catalog names, writes the roadmap, hands the newest losses to pursuits
within the policy's budget, and turns every open Probierz incident into a
roadmap item of the product its envelope names, withdrawn once it is
resolved. Every step is recorded under test-results/.autonomy/. `schedule`
has Stado run the cycle on a cron, pinned to one host.";
