//! Discovery, adoption, authoring and evaluation, dispatched exactly as
//! `main.rs` dispatched them.

use std::path::Path;

use crate::cli::inspect::{AuthorCommand, EvaluateCommand, InspectCommand};
use crate::failure::Answer;
use crate::{adoption, apphooks, authoring, discovery, readme_gif};

pub fn dispatch(harness: &Path, command: InspectCommand) -> Answer {
    match command {
        InspectCommand::Onboarding { args } => adoption::onboarding(harness, &args),
        InspectCommand::Project { command } => adoption::dispatch(harness, command),
        InspectCommand::List => discovery::list(harness),
        InspectCommand::Apps => discovery::apps(harness),
        InspectCommand::App { app_id } => discovery::app(harness, &app_id),
        InspectCommand::Apphook { capability, args } => {
            apphooks::command(harness, &capability, &args)
        }
        InspectCommand::Specs { surface } => discovery::specs(harness, surface.as_deref()),
        InspectCommand::Describe { spec } => discovery::describe(harness, &spec),
        InspectCommand::Cmd { target } => discovery::cmd(harness, &target),
        InspectCommand::Hosts => discovery::hosts(),
        InspectCommand::Journal { path } => {
            println!("{}", serde_json::to_string_pretty(&authoring::inspect_security_journal(&path)?)?);
            Ok(())
        }
        // PortAuthoring: authoring, evaluation, and identity
        InspectCommand::Identity { app_id } => authoring::source_identity_command(harness, &app_id),
        InspectCommand::Accessibility { app_id } => {
            authoring::accessibility_command(harness, &app_id)
        }
        InspectCommand::Author { command } => author(harness, command),
        InspectCommand::Repair {
            app_id,
            run_id,
            rounds,
            dry_run,
        } => {
            let result =
                authoring::repair_failed_run(harness, &app_id, run_id.as_deref(), rounds, dry_run)?;
            authoring::print_result("repair", "/ok", result)
        }
        InspectCommand::Evaluate { command } => evaluate(harness, command),
        // ReadmeGif
        InspectCommand::Gif {
            input,
            output,
            start,
            duration,
            fps,
            width,
            force,
        } => readme_gif::create(readme_gif::Options {
            input,
            output,
            start_seconds: start,
            duration_seconds: duration,
            frames_per_second: fps,
            width,
            force,
        }),
    }
}

/// `evaluate figure`, `evaluate seo` and `evaluate talk`: each reads the
/// router bearer (and, for SEO, the signing key) from stdin when asked,
/// evaluates, prints the verdict and fails when it does not pass.
fn evaluate(harness: &Path, command: EvaluateCommand) -> Answer {
    match command {
        EvaluateCommand::Security { app_id, model, deterministic: _, output, router_url, agent_id, router_token_stdin } => {
            let mut stdin = String::new();
            if router_token_stdin {
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin)?;
            }
            let mut lines = stdin.lines();
            let result = authoring::evaluate_security(harness, &app_id, &output, model.as_deref(),
                router_url.as_deref(), lines.next(), agent_id.as_deref(), lines.next())?;
            authoring::print_result("evaluate.security", "/pass", result)
        }
        EvaluateCommand::Figure {
            reference,
            candidate,
            rubric,
            model,
            output,
            router_url,
            tex_preamble,
            agent_id,
            router_token_stdin,
        } => {
            let mut stdin = String::new();
            if router_token_stdin {
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin)?;
            }
            let mut lines = stdin.lines();
            let bearer = lines.next();
            let secret = lines.next();
            let result = authoring::evaluate_figure(
                harness,
                &reference,
                &candidate,
                rubric.as_deref(),
                output.as_deref(),
                model.as_deref(),
                router_url.as_deref(),
                tex_preamble.as_deref(),
                bearer,
                agent_id.as_deref(),
                secret,
            )?;
            authoring::print_result("evaluate.figure", "/verdict/pass", result)
        }
        EvaluateCommand::Seo {
            app_id,
            base_url,
            policy,
            brief,
            mode,
            output,
            production_evidence,
            primary_model,
            secondary_model,
            adjudicator_model,
            router_url,
            agent_id,
            private_key_file,
            router_token_stdin,
        } => {
            let mut stdin = String::new();
            if router_token_stdin {
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin)?;
            }
            let mut lines = stdin.lines();
            let bearer = lines.next();
            let secret = lines.next();
            let private_key = if router_token_stdin {
                Some(lines.collect::<Vec<_>>().join("\n"))
            } else {
                None
            };
            let result = authoring::evaluate_seo(
                harness,
                &app_id,
                &base_url,
                policy.as_deref(),
                brief.as_deref(),
                &mode,
                output.as_deref(),
                production_evidence.as_deref(),
                primary_model.as_deref(),
                secondary_model.as_deref(),
                adjudicator_model.as_deref(),
                router_url.as_deref(),
                agent_id.as_deref(),
                private_key_file.as_deref(),
                bearer,
                secret,
                private_key
                    .as_deref()
                    .filter(|value| !value.trim().is_empty()),
            )?;
            authoring::print_result("evaluate.seo", "/pass", result)
        }
        EvaluateCommand::Talk {
            slides,
            script,
            sources,
            rubric,
            model,
            output,
            router_url,
            agent_id,
            router_token_stdin,
        } => {
            let mut stdin = String::new();
            if router_token_stdin {
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin)?;
            }
            let mut lines = stdin.lines();
            let bearer = lines.next();
            let secret = lines.next();
            let result = authoring::evaluate_talk(
                harness,
                &slides,
                &script,
                &sources,
                rubric.as_deref(),
                output.as_deref(),
                model.as_deref(),
                router_url.as_deref(),
                bearer,
                agent_id.as_deref(),
                secret,
            )?;
            authoring::print_result("evaluate.talk", "/verdict/pass", result)
        }
    }
}

/// `author spec` and `author manifest`: each drafts with the model router,
/// verifies what it drafted and prints the result, failing when it did not
/// hold.
fn author(harness: &Path, command: AuthorCommand) -> Answer {
    match command {
        AuthorCommand::Spec {
            app_id,
            journey,
            target,
            desc,
            base_url,
            app_path,
            mapping_paths,
            rounds,
            dry_run,
        } => {
            let result = authoring::author_spec(
                harness,
                &app_id,
                &journey,
                &target,
                &desc,
                base_url.as_deref(),
                app_path.as_deref(),
                &mapping_paths,
                rounds,
                dry_run,
            )?;
            authoring::print_result("author.spec", "/ok", result)
        }
        AuthorCommand::Manifest {
            app_id,
            desc,
            target,
            repositories,
            owner,
            base_url,
            app_path,
            dry_run,
            with_specs,
        } => {
            let result = authoring::author_manifest(
                harness,
                &app_id,
                &desc,
                owner.as_deref(),
                &repositories,
                &target,
                base_url.as_deref(),
                app_path.as_deref(),
                dry_run,
                with_specs,
            )?;
            authoring::print_result("author.manifest", "/ok", result)
        }
    }
}
