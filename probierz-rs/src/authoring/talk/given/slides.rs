//! The slide pictures of a talk, in the order they are shown. Slides are the
//! `slide-<number>.png` pictures `lem-cli talk render --slides` writes; their
//! order comes from those numbers, which must run from the first slide without
//! a gap: a picture named any other way, a repeated number or a missing slide
//! is refused rather than ordered by a guess.

use super::*;

/// The name every slide picture carries before its number.
const SLIDE_PREFIX: &str = "slide-";
/// The picture type a slide is read as.
const SLIDE_EXTENSION: &str = "png";

/// The slide pictures in `directory`, ordered by their numbers, which must
/// run from the first slide without a gap.
pub(super) fn ordered_slides(directory: &Path) -> Result<Vec<PathBuf>, Failure> {
    let entries = fs::read_dir(directory).map_err(|error| {
        Failure::invalid(
            "evaluate.talk",
            format!("--slides {}: {error}", directory.display()),
        )
    })?;
    let mut numbered = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| {
                Failure::invalid(
                    "evaluate.talk",
                    format!("--slides {}: {error}", directory.display()),
                )
            })?
            .path();
        if path.extension().and_then(OsStr::to_str) != Some(SLIDE_EXTENSION) {
            continue;
        }
        numbered.push((slide_number(&path)?, path));
    }
    if numbered.is_empty() {
        return Err(Failure::invalid(
            "evaluate.talk",
            format!(
                "--slides {} holds no {SLIDE_PREFIX}<number>.{SLIDE_EXTENSION} pictures; render them with lem-cli talk render --slides",
                directory.display()
            ),
        ));
    }
    numbered.sort_by_key(|(number, _)| *number);
    // Two pictures with one number (slide-1.png beside slide-01.png) name the
    // same slide twice; that is a repeat, refused as one, not a gap.
    let following = numbered
        .split_first()
        .map(|(_, rest)| rest)
        .into_iter()
        .flatten();
    for ((first, earlier), (second, later)) in numbered.iter().zip(following) {
        if first == second {
            return Err(Failure::invalid(
                "evaluate.talk",
                format!(
                    "--slides {} holds {} and {}, two pictures of slide {first}: keep one",
                    directory.display(),
                    earlier.display(),
                    later.display()
                ),
            ));
        }
    }
    for (expected, (number, _)) in (std::num::NonZeroUsize::MIN.get()..).zip(&numbered) {
        if *number != expected {
            return Err(Failure::invalid(
                "evaluate.talk",
                format!(
                    "--slides {} has no {SLIDE_PREFIX}{expected}.{SLIDE_EXTENSION}: the slides must run from the first without a gap",
                    directory.display()
                ),
            ));
        }
    }
    Ok(numbered.into_iter().map(|(_, path)| path).collect())
}

/// The number in a `slide-<number>.png` name, refused naming the picture
/// and the parser's reason when the name holds none.
fn slide_number(path: &Path) -> Result<usize, Failure> {
    let unnamed = |reason: String| {
        Failure::invalid(
            "evaluate.talk",
            format!(
                "{} is not named {SLIDE_PREFIX}<number>.{SLIDE_EXTENSION} ({reason}), so its place in the talk is unknown",
                path.display()
            ),
        )
    };
    let digits = path
        .file_stem()
        .and_then(OsStr::to_str)
        .and_then(|stem| stem.strip_prefix(SLIDE_PREFIX))
        .ok_or_else(|| unnamed(format!("its name does not start with {SLIDE_PREFIX}")))?;
    digits
        .parse::<usize>()
        .map_err(|error| unnamed(format!("{digits}: {error}")))
}
