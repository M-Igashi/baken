use anyhow::Result;
use baken_core::cdjsafe::check::{CheckReport, Player, TrackCheck, Verdict};
use baken_core::cdjsafe::{self, Action, Plan, Report, SkipReason, CDJSAFE_FOLDER_NAME};
use baken_core::Error;
use console::{measure_text_width, pad_str, style, truncate_str, Alignment};

use crate::args::{CdjsafeArgs, PlayerChoice};
use crate::progress::with_bar;
use crate::report::print_counts;

pub fn run(args: &CdjsafeArgs) -> Result<()> {
    baken_core::check_ffmpeg()?;

    let plan = cdjsafe::plan(&args.xml, &args.playlist)?;
    print_skipped(&plan);
    if args.check {
        return run_check(args, &plan);
    }
    let out_dir = args
        .out_dir
        .as_deref()
        .expect("clap requires --out-dir without --check");
    for name in plan.missing_total_time() {
        println!(
            "{} '{}' has no TotalTime attribute — rekordbox will silently skip its cues on import",
            style("⚠").yellow(),
            name
        );
    }

    let skipped_note = match plan.skipped().len() {
        0 => String::new(),
        n => format!(" ({n} skipped)"),
    };
    println!(
        "{} Converting {} tracks from '{}'{} → {}",
        style("▸").cyan(),
        style(plan.len()).cyan(),
        style(&args.playlist).bold(),
        skipped_note,
        out_dir.display()
    );

    let result = with_bar(plan.len(), "Converting...", |p, c| {
        cdjsafe::convert(&plan, out_dir, args.output.as_deref(), p, c)
    });

    match result {
        Ok(report) => {
            print_report(&report);
            Ok(())
        }
        Err(Error::ConversionFailed { failures, total }) => {
            for f in &failures {
                println!("{} {}", style("✗").red(), f);
            }
            Err(Error::ConversionFailed { failures, total }.into())
        }
        Err(e) => Err(e.into()),
    }
}

fn print_report(report: &Report) {
    let (copied, lossless, lossy) = (
        report.count(Action::Copy),
        report.count(Action::Reencode),
        report.count(Action::ReencodeLossy),
    );

    println!(
        "\n{} Done! {} CDJ-safe tracks written.",
        style("✓").green().bold(),
        report.tracks.len()
    );
    print_counts(&[
        (lossless, "re-encoded from lossless sources"),
        (lossy, "re-encoded lossy→lossy (generation loss)"),
        (copied, "copied (already 320 kbps CBR MP3 @ 44.1 kHz)"),
    ]);

    if lossy > 0 {
        println!(
            "\n{} Lossy→lossy re-encodes — refresh these from lossless masters before the next gig:",
            style("⚠").yellow()
        );
        for (name, action) in &report.tracks {
            if *action == Action::ReencodeLossy {
                println!("  {} {}", style("•").dim(), name);
            }
        }
    }

    if !report.skipped.is_empty() {
        println!(
            "\n{} Skipped (file not found on disk, or Location not decodable) — not on the stick, not in the new playlist:",
            style("⚠").yellow()
        );
        for name in &report.skipped {
            println!("  {} {}", style("•").dim(), name);
        }
    }

    println!(
        "\n{} XML written: {} (playlist '{}/{}')",
        style("✓").green().bold(),
        report.output_xml.display(),
        style(CDJSAFE_FOLDER_NAME).bold(),
        style(&report.playlist_name).bold()
    );
    println!(
        "  {} rekordbox: Preferences > Advanced > Database > rekordbox xml → load the XML, restart rekordbox",
        style("ℹ").blue()
    );
    println!(
        "  {} Open the 'rekordbox xml' sidebar tree, right-click the imported tracks → Import to Collection",
        style("ℹ").blue()
    );
    println!(
        "  {} Cues and beatgrid are carried over — no re-analysis needed. Then export the playlist to USB.",
        style("ℹ").blue()
    );
}

fn print_skipped(plan: &Plan) {
    for skipped in plan.skipped() {
        match &skipped.reason {
            SkipReason::NotFound => println!(
                "{} '{}' not found on disk — skipped: {}",
                style("⚠").yellow(),
                skipped.name,
                skipped.location
            ),
            SkipReason::BadLocation(why) => println!(
                "{} '{}' has a Location that is not a usable file URL — skipped: {} ({})",
                style("⚠").yellow(),
                skipped.name,
                skipped.location,
                why
            ),
        }
    }
}

fn run_check(args: &CdjsafeArgs, plan: &Plan) -> Result<()> {
    println!(
        "{} Checking {} tracks from '{}' against Pioneer's operating instructions (nothing is written)",
        style("▸").cyan(),
        style(plan.len()).cyan(),
        style(&args.playlist).bold()
    );
    let report = with_bar(plan.len(), "Checking...", |p, c| {
        cdjsafe::check::check(plan, p, c)
    })?;
    let players = selected_players(&args.players);
    print_check(&report, &players);
    if report.is_flagged(&players) {
        std::process::exit(1);
    }
    Ok(())
}

/// The players `--player` names, in `Player::ALL` order; without it the three
/// generations of CDJ found in most booths.
fn selected_players(choices: &[PlayerChoice]) -> Vec<Player> {
    if choices.is_empty() {
        return vec![Player::PreNxs2, Player::Cdj2000Nxs2, Player::Cdj3000];
    }
    Player::ALL
        .into_iter()
        .filter(|&p| {
            choices.iter().any(|c| match c {
                PlayerChoice::All => true,
                PlayerChoice::One(q) => *q == p,
            })
        })
        .collect()
}

/// Labels are model names: "an XDJ-AZ", "an OPUS-QUAD", "an OMNIS-DUO".
fn article(label: &str) -> &'static str {
    if label.starts_with(['O', 'X']) {
        "an"
    } else {
        "a"
    }
}

fn mark(v: &Verdict) -> console::StyledObject<&'static str> {
    match v {
        Verdict::Plays => style("✓").green(),
        Verdict::Refuses(_) => style("✗").red(),
        Verdict::Unknown(_) => style("?").yellow(),
    }
}

fn describe(t: &TrackCheck) -> String {
    match &t.facts {
        Ok(f) => f.to_string(),
        Err(_) => "unreadable".to_string(),
    }
}

fn print_check(report: &CheckReport, players: &[Player]) {
    let name_width = report
        .tracks
        .iter()
        .map(|t| measure_text_width(&t.name))
        .max()
        .unwrap_or(5)
        .clamp(5, 40);
    let format_width = report
        .tracks
        .iter()
        .map(|t| measure_text_width(&describe(t)))
        .max()
        .unwrap_or(6)
        .max(6);
    let left = |s: &str, w: usize| {
        pad_str(&truncate_str(s, w, "…"), w, Alignment::Left, None).into_owned()
    };

    println!();
    print!(
        "  {}  {}",
        left("Track", name_width),
        left("Format", format_width)
    );
    for &p in players {
        print!("  {}", p.label());
    }
    println!();
    for t in &report.tracks {
        print!(
            "  {}  {}",
            left(&t.name, name_width),
            left(&describe(t), format_width)
        );
        for &p in players {
            let w = measure_text_width(p.label());
            let m = mark(t.verdict(p)).to_string();
            print!("  {}", pad_str(&m, w, Alignment::Center, None));
        }
        println!();
    }

    let notes: Vec<&TrackCheck> = report
        .tracks
        .iter()
        .filter(|t| {
            t.facts.is_err()
                || players.iter().any(|&p| *t.verdict(p) != Verdict::Plays)
                || t.low_cutoff().is_some()
        })
        .collect();
    if !notes.is_empty() {
        println!();
    }
    for t in notes {
        println!("{} {}", style("•").dim(), style(&t.name).bold());
        if let Err(e) = &t.facts {
            println!("    {} {}", style("✗").red(), e);
            continue;
        }
        for &p in players {
            let v = t.verdict(p);
            if let Verdict::Refuses(why) | Verdict::Unknown(why) = v {
                let models = match p.models() {
                    m if m == p.label() => String::new(),
                    m => format!(" ({m})"),
                };
                println!("    {} {}{models}: {why}", mark(v), p.label());
            }
        }
        if let (Some(hz), Ok(f)) = (t.low_cutoff(), &t.facts) {
            println!(
                "    {} nothing above {:.1} kHz, low for {} kbps: likely transcoded from a lower bitrate",
                style("⚠").yellow(),
                f64::from(hz) / 1000.0,
                f.bitrate_kbps.unwrap_or_default()
            );
        }
    }

    let total = report.tracks.len();
    println!();
    for &p in players {
        let (refused, unknown) = (report.refused(p), report.unknown(p));
        let unknown_note = match unknown {
            0 => String::new(),
            n => format!(" {n} more {} unknown.", if n == 1 { "is" } else { "are" }),
        };
        if refused > 0 {
            println!(
                "{} {refused} of {total} tracks will not play on {} {} player; `baken cdjsafe` converts them.{unknown_note}",
                style("✗").red(),
                article(p.label()),
                p.label()
            );
        } else {
            println!(
                "{} {} of {total} tracks are in formats {} {} player plays.{unknown_note}",
                style("✓").green(),
                total - unknown - report.unreadable(),
                article(p.label()),
                p.label()
            );
        }
    }
    let plural = |n: usize, one: &'static str, many: &'static str| if n == 1 { one } else { many };
    let unreadable = report.unreadable();
    if unreadable > 0 {
        println!(
            "{} {unreadable} {} could not be read at all; check {} before the gig.",
            style("⚠").yellow(),
            plural(unreadable, "track", "tracks"),
            plural(unreadable, "it", "them")
        );
    }
    let low = report.low_cutoffs();
    if low > 0 {
        println!(
            "{} {low} lossy {} short of what {} bitrate keeps, likely transcoded from a lower bitrate. Nothing is changed; listen to {} before the gig.",
            style("⚠").yellow(),
            plural(low, "track stops", "tracks stop"),
            plural(low, "its", "their"),
            plural(low, "it", "them")
        );
    }
    let skipped = report.skipped.len();
    if skipped > 0 {
        println!(
            "{} {skipped} more {} missing or {} an unusable Location, so no player gets {}.",
            style("⚠").yellow(),
            plural(skipped, "track is", "tracks are"),
            plural(skipped, "has", "have"),
            plural(skipped, "it", "them")
        );
    }
}
