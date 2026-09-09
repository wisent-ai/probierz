//! Discovery, adoption, authoring and evaluation, dispatched exactly as
//! `main.rs` dispatched them.

use std::path::Path;

use crate::cli::inspect::InspectCommand;
use crate::failure::Answer;
use crate::{adoption, apphooks, authoring, discovery, readme_gif, serve};

pub fn dispatch(harness: &Path, command: InspectCommand) -> Answer {
    match command {
        InspectCommand::Onboarding { args } => adoption::onboarding(harness, &args),
        InspectCommand::Project { command } => adoption::dispatch(harness, command),
        InspectCommand::Serve { args } => serve::serve(harness, &args),
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
        // PortAuthoring: authoring, evaluation, and identity
        InspectCommand::SourceIdentity { app_id } => {
            authoring::source_identity_command(harness, &app_id)
        }
        InspectCommand::Accessibility { app_id } => {
            if !authoring::accessibility_command(harness, &app_id)? {
                std::process::exit(1);
            }
            Ok(())
        }
        InspectCommand::AuthorSpec {
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
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        InspectCommand::AuthorManifest {
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
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        InspectCommand::Repair {
            app_id,
            run_id,
            rounds,
            dry_run,
        } => {
            let result =
                authoring::repair_failed_run(harness, &app_id, run_id.as_deref(), rounds, dry_run)?;
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        InspectCommand::FigureEvaluate {
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
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        InspectCommand::SeoEvaluate {
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
            if !authoring::print_result(result)? {
                std::process::exit(1);
            }
            Ok(())
        }
        // ReadmeGif
        InspectCommand::ReadmeGif {
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
