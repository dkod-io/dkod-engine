use anyhow::{bail, Result};
use colored::Colorize;
use gix::bstr::ByteSlice;

use crate::util::discover_repo;

pub fn run(oneline: bool, n: Option<usize>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = discover_repo(&cwd)?;

    let head = repo.head_commit();
    let head_commit = match head {
        Ok(commit) => commit,
        Err(_) => bail!("no commits yet"),
    };

    let limit = n.unwrap_or(usize::MAX);

    let walk = repo
        .rev_walk([head_commit.id])
        .all()?;

    for (count, info) in walk.enumerate() {
        if count >= limit {
            break;
        }

        let info = info?;
        let commit = info.object()?;

        if oneline {
            let id_str = commit.id().to_string();
            let short_id = &id_str[..id_str.len().min(7)];
            let message = commit
                .message_raw_sloppy()
                .to_str_lossy();
            let first_line = message.lines().next().unwrap_or("");
            println!("{} {}", short_id.yellow(), first_line);
        } else {
            let id = commit.id().to_string();
            let author = commit.author()?;
            let name = author.name.to_str_lossy();
            let email = author.email.to_str_lossy();
            let time = author.time()?;

            println!("commit {}", id.yellow());
            println!("Author: {} <{}>", name, email);
            println!("Date:   {}", format_time(time.seconds, time.offset));
            println!();

            let message = commit.message_raw_sloppy().to_str_lossy();
            for line in message.lines() {
                println!("    {}", line);
            }
            println!();
        }
    }

    Ok(())
}

/// Format a Unix timestamp with timezone offset into a human-readable date string.
/// We avoid adding chrono as a dependency and do simple formatting with offset support.
fn format_time(epoch: i64, offset_secs: i32) -> String {
    // Adjust to local time by applying the timezone offset
    let local_secs = epoch + offset_secs as i64;
    let days = local_secs.div_euclid(86400);
    let time_of_day = local_secs.rem_euclid(86400);

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_date(days);

    let month_name = match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    };

    let weekday = ((days.rem_euclid(7) + 4) % 7) as usize; // 1970-01-01 was Thursday (4)
    let day_name = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][weekday];

    // Format timezone offset as +HHMM or -HHMM
    let sign = if offset_secs >= 0 { '+' } else { '-' };
    let off_h = offset_secs.unsigned_abs() / 3600;
    let off_m = (offset_secs.unsigned_abs() % 3600) / 60;

    format!(
        "{} {} {} {:02}:{:02}:{:02} {} {}{:02}{:02}",
        day_name, month_name, day, hours, minutes, seconds, year, sign, off_h, off_m
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days: i64) -> (i64, u32, u32) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
