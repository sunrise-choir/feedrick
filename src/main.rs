use std::fs::OpenOptions;
use std::io::{self, stdin, stdout, Write};
use std::path::PathBuf;

use rayon::prelude::*;
use structopt::StructOpt;

use flumedb::flume_log::{Error, FlumeLog};
use flumedb::log_entry::LogEntry;
use flumedb::offset_log::{BidirIterator, OffsetLog};

use serde_json::{to_string_pretty, Value};

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

#[derive(StructOpt)]
#[structopt(
    name = "feedrick",
    author = "Sunrise Choir (sunrisechoir.com)",
    about = "ssb flumedb offset log utilities",
    version = "0.2"
)]

enum Opt {
    #[structopt(about = "Verify that all messages parse as json")]
    Check {
        #[structopt(parse(from_os_str))]
        in_path: PathBuf,
    },

    #[structopt(about = "Copy the feed for a single id into a separate file.")]
    Extract {
        /// source offset log file (eg. "~/.ssb/log.offset")
        #[structopt(long, short, name = "in", parse(from_os_str))]
        in_path: PathBuf,
        /// Destination path
        #[structopt(long, short, name = "out", parse(from_os_str))]
        out_path: PathBuf,
        /// Overwrite output file, if it exists
        #[structopt(long)]
        overwrite: bool,

        /// feed (user) id (eg. "@N/vWpVVdD...")
        #[structopt(long, short)]
        feed: String,

        /// Output a log file containing all feeds *except* the specified id.
        #[structopt(long)]
        invert: bool,
    },

    #[structopt(about = "Copy all the feeds and sort by asserted time")]
    Sort {
        /// source offset log file (eg. "~/.ssb/log.offset")
        #[structopt(long, short, name = "in", parse(from_os_str))]
        in_path: PathBuf,
        /// Destination path
        #[structopt(long, short, name = "out", parse(from_os_str))]
        out_path: PathBuf,
        /// Overwrite output file, if it exists
        #[structopt(long)]
        overwrite: bool,
    },

    #[structopt(about = "View a flumedb offset log file")]
    View {
        #[structopt(parse(from_os_str))]
        in_path: PathBuf,
    },
}

fn main() -> Result<(), Error> {
    match Opt::from_args() {
        Opt::Check { in_path } => check_log(OffsetLog::<u32>::open_read_only(in_path)?),

        Opt::Extract {
            in_path,
            out_path,
            overwrite,
            feed,
            invert,
        } => {
            if !overwrite && out_path.exists() {
                eprintln!("Output path `{:?}` exists.", out_path);
                eprintln!("Use `--overwrite` option to overwrite.");
                return Ok(());
            }

            let in_log = OffsetLog::<u32>::open_read_only(&in_path)?;
            if in_log.end() == 0 {
                eprintln!("Input offset log file is empty.");
                return Ok(());
            }

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&out_path)?;

            let out_log = OffsetLog::<u32>::from_file(file)?;

            println!("Copying feed id: {}", feed);
            eprintln!(" from offset log at path:     {}", in_path.display());
            eprintln!(" into new offset log at path: {}", out_path.display());

            if invert {
                copy_log_entries_using_author(in_log, out_log, |id| id != feed)
            } else {
                copy_log_entries_using_author(in_log, out_log, |id| id == feed)
            }
        }
        Opt::Sort {
            in_path,
            out_path,
            overwrite,
        } => {
            if !overwrite && out_path.exists() {
                eprintln!("Output path `{}` exists.", out_path.display());
                eprintln!("Use `--overwrite` option to overwrite.");
                return Ok(());
            }

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&out_path)?;

            let mut out_log = OffsetLog::<u32>::from_file(file)?;

            let in_log = OffsetLog::<u32>::open_read_only(&in_path)?;
            if in_log.end() == 0 {
                eprintln!("Input offset log file is empty.");
                return Ok(());
            }

            eprintln!(" from offset log at path:     {}", in_path.display());
            eprintln!(" into new offset log at path: {}", out_path.display());

            let mut entries = in_log
                .iter()
                .map(|entry| (get_entry_timestamp(&entry), entry.offset))
                .collect::<Vec<_>>();

            entries.par_sort_unstable_by(|(a, _), (b, _)| a.partial_cmp(&b).unwrap());

            eprintln!(
                " sorted {} entries, writing out to new offset file",
                entries.len()
            );

            entries.iter().for_each(|(_, offset)| {
                let entry = in_log.get(*offset).unwrap();
                out_log.append(&entry).unwrap();
            });

            Ok(())
        }

        Opt::View { in_path } => view_log(OffsetLog::<u32>::open_read_only(in_path)?),
    }
}

// copy if author id matches predicate
fn copy_log_entries_using_author<F>(
    in_log: OffsetLog<u32>,
    out_log: OffsetLog<u32>,
    should_write: F,
) -> Result<(), Error>
where
    F: Fn(&str) -> bool,
{
    copy_log_entries(in_log, out_log, |e| {
        let v: Result<Value, serde_json::error::Error> = serde_json::from_slice(&e.data);

        match v {
            Ok(v) => v
                .get("value")
                .and_then(|v| v.get("author"))
                .and_then(|v| v.as_str())
                .map_or(false, |v| should_write(v)),
            Err(_) => false,
        }
    })
}

fn copy_log_entries<F>(
    in_log: OffsetLog<u32>,
    mut out_log: OffsetLog<u32>,
    should_write: F,
) -> Result<(), Error>
where
    F: Fn(&LogEntry) -> bool,
{
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    let in_len = in_log.end();
    if in_len == 0 {
        eprintln!("Input offset log file is empty.");
        return Ok(());
    }

    let iter = in_log.iter().map(|e| {
        let sw = should_write(&e);
        (e, sw)
    });

    let mut count: usize = 0;
    let mut prev_pct: usize = 0;
    let mut bytes: u64 = 0;

    for (e, should_write) in iter {
        let pct = (100.0 * (e.offset as f64 / in_len as f64)) as usize;

        if should_write {
            bytes = out_log.append(&e.data)?;
            count += 1;
        }

        if should_write || (pct > prev_pct) {
            write!(
                handle,
                "\rProgress: {}%\tCopied {} messages ({} bytes)",
                pct, count, bytes
            )?;
            handle.flush()?;
            prev_pct = pct;
        }
    }
    println!("");
    println!("Done!");
    Ok(())
}

fn check_log(log: OffsetLog<u32>) -> Result<(), Error> {
    println!("total entries: {}", log.end());

    // TODO: option for reverse?
    log.iter().for_each(|e| {
        let value = serde_json::from_slice::<Value>(&e.data);

        match value {
            Ok(_) => print!("\rentry {} ok", e.offset),
            Err(err) => {
                println!("\n\n===>found broken entry: {} (err: {})", e.offset, err);
            }
        }
    });

    Ok(())
}

fn view_log(log: OffsetLog<u32>) -> Result<(), Error> {
    let stdin = stdin();
    let mut stdout = stdout().into_raw_mode()?;

    let mut iter = log.bidir_iter().map(|e| {
        let v = serde_json::from_slice(&e.data).unwrap();
        (e, v)
    });

    iter.next()
        .map(|(e, v)| print_entry(e.offset, &v, &mut stdout));

    for c in stdin.keys() {
        match c? {
            Key::Char('q') | Key::Ctrl('c') | Key::Esc => {
                break;
            }
            Key::Up | Key::Left | Key::Char('p') | Key::Char('k') => {
                iter.prev()
                    .map(|(e, v)| print_entry(e.offset, &v, &mut stdout))
                    .or_else(|| write!(stdout, "No record").ok());
            }
            Key::Down | Key::Right | Key::Char('n') | Key::Char('j') => {
                iter.next()
                    .map(|(e, v)| print_entry(e.offset, &v, &mut stdout))
                    .or_else(|| write!(stdout, "No record").ok());
            }
            Key::Char(c) => {
                eprintln!("KEY: {}", c);
            }
            _ => {}
        }
    }

    Ok(())
}

fn get_entry_timestamp(e: &LogEntry) -> f64 {
    let v: Result<Value, serde_json::error::Error> = serde_json::from_slice(&e.data);

    match v {
        Ok(v) => v
            .get("value")
            .and_then(|v| v.get("timestamp"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        Err(_) => 0.0,
    }
}

fn print_entry<W: Write>(offset: u64, data: &serde_json::Value, mut stdout: &mut W) {
    write!(
        stdout,
        "{}{}Press `j` or `k` to show the next or previous entry. Press `q` to exit.{}Offset: {}",
        termion::clear::All,
        termion::cursor::Goto(1, 1),
        termion::cursor::Goto(1, 2),
        offset
    )
    .unwrap();
    print_lines(&to_string_pretty(&data).unwrap(), &mut stdout).unwrap();
    stdout.flush().unwrap();
}

fn print_lines<W: Write>(s: &str, stdout: &mut W) -> io::Result<()> {
    for line in s.lines() {
        write!(stdout, "\n\r{}", &line)?;
    }
    Ok(())
}
