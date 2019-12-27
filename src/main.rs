use std::fs::OpenOptions;
use std::io::{self, stdin, stdout, Write};
use std::path::Path;

use clap::{App, Arg, SubCommand};
use rayon::prelude::*;

use flumedb::flume_log::{Error, FlumeLog};
use flumedb::log_entry::LogEntry;
use flumedb::offset_log::{BidirIterator, OffsetLog};

use serde_json::{to_string_pretty, Value};

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

fn main() -> Result<(), Error> {
    let app_m = App::new("feedrick")
        .version("0.1")
        .author("Sunrise Choir (sunrisechoir.com)")
        .about("ssb flumedb offset log utilities")
        .subcommand(
            SubCommand::with_name("sort")
                .about("Copy all the feeds and sort by asserted time")
                .arg(
                    Arg::with_name("in")
                        .long("in")
                        .short("i")
                        .required(true)
                        .takes_value(true)
                        .help("source offset log file"),
                )
                .arg(
                    Arg::with_name("out")
                        .long("out")
                        .short("o")
                        .required(true)
                        .takes_value(true)
                        .help("destination path"),
                )
                .arg(
                    Arg::with_name("overwrite")
                        .long("overwrite")
                        .help("Overwrite output file, if it exists."),
                ),
        )
        .subcommand(
            SubCommand::with_name("extract")
                .about("Copy the feed for a single id into a separate file.")
                .arg(
                    Arg::with_name("in")
                        .long("in")
                        .short("i")
                        .required(true)
                        .takes_value(true)
                        .help("source offset log file"),
                )
                .arg(
                    Arg::with_name("out")
                        .long("out")
                        .short("o")
                        .required(true)
                        .takes_value(true)
                        .help("destination path"),
                )
                .arg(
                    Arg::with_name("id")
                        .long("feed")
                        .short("f")
                        .required(true)
                        .takes_value(true)
                        .help("feed (user) id (eg. \"@N/vWpVVdD...\""),
                )
                .arg(
                    Arg::with_name("overwrite")
                        .long("overwrite")
                        .help("Overwrite output file, if it exists."),
                )
                .arg(
                    Arg::with_name("invert")
                        .long("invert")
                        .help("Output a log file containing all feeds *but* the specified id."),
                ),
        )
        .subcommand(
            SubCommand::with_name("view")
                .about("View a flumedb offset log file")
                .arg(
                    Arg::with_name("FILE")
                        .help("offset log file to view")
                        .required(true)
                        .index(1),
                ),
        )
        .get_matches();

    match app_m.subcommand() {
        ("extract", Some(sub_m)) => {
            let in_path = sub_m.value_of("in").unwrap();
            let out_path = sub_m.value_of("out").unwrap();
            let feed_id = sub_m.value_of("id").unwrap();
            let overwrite = sub_m.is_present("overwrite");
            let invert = sub_m.is_present("invert");

            if !overwrite && Path::new(out_path).exists() {
                eprintln!("Output path `{}` exists.", out_path);
                eprintln!("Use `--overwrite` option to overwrite.");
                return Ok(());
            }

            let in_log = OffsetLog::<u32>::open_read_only(in_path)?;
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

            println!("Copying feed id: {}", feed_id);
            eprintln!(" from offset log at path:     {}", in_path);
            eprintln!(" into new offset log at path: {}", out_path);

            if invert {
                copy_log_entries_using_author(in_log, out_log, |id| id != feed_id)
            } else {
                copy_log_entries_using_author(in_log, out_log, |id| id == feed_id)
            }
        }
        ("sort", Some(sub_m)) => {
            let in_path = sub_m.value_of("in").unwrap();
            let out_path = sub_m.value_of("out").unwrap();
            let overwrite = sub_m.is_present("overwrite");

            if !overwrite && Path::new(out_path).exists() {
                eprintln!("Output path `{}` exists.", out_path);
                eprintln!("Use `--overwrite` option to overwrite.");
                return Ok(());
            }

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&out_path)?;

            let mut out_log = OffsetLog::<u32>::from_file(file)?;

            let in_log = OffsetLog::<u32>::open_read_only(in_path)?;
            if in_log.end() == 0 {
                eprintln!("Input offset log file is empty.");
                return Ok(());
            }

            eprintln!(" from offset log at path:     {}", in_path);
            eprintln!(" into new offset log at path: {}", out_path);

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

        ("view", Some(sub_m)) => {
            let path = sub_m.value_of("FILE").unwrap();

            let log = OffsetLog::<u32>::open_read_only(path)?;
            view_log(log)
        }
        _ => {
            println!("{}", app_m.usage());
            Ok(())
        }
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
