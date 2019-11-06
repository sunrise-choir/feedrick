# feedrick

feedrick is an experimental utility for (non-destructive) operations on
flumedb offset logs containing secure scuttlebutt feeds.

```
USAGE:
    feedrick [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    extract    Copy the feed for a single id into a separate file.
    help       Prints this message or the help of the given subcommand(s)
    sort       Copy all the feeds and sort by asserted time
    view       View a flumedb offset log file
```

Currently implemented:

- Very basic log viewer
```
feedrick view ~/.ssb/flume/log.offset
```

- "Extract" (copy) a single feed from a source log to a new log

```
feedrick extract --in ~/.ssb/flume/log.offset --out /tmp/me.offset --feed "@N/vWpVVdD1e8IbACUQE4EVGL6+aodQfbQZ8ByC+k79s=.ed25519"
```

- Make a copy of a your log with all feeds *except* the specified feed
```
feedrick extract --in ~/.ssb/flume/log.offset --out /tmp/everyone_but_sbot.offset --feed "@vYqLJ+S8RSwrgU6Nxja0kM3d19oWqjv9Og2JCbDd8+U=.ed25519" --invert
```

```
USAGE:
    feedrick extract [FLAGS] --feed <id> --in <in> --out <out>

FLAGS:
    -h, --help         Prints help information
        --invert       Output a log file containing all feeds *but* the specified id.
        --overwrite    Overwrite output file, if it exists.
    -V, --version      Prints version information

OPTIONS:
    -f, --feed <id>    feed (user) id (eg. "@N/vWpVVdD..."
    -i, --in <in>      source offset log file
    -o, --out <out>    destination path
```


- `sort` all the messages in an offset file by `assertedTimestamp`
```
feedrick sort --in ~/.ssb/flume/log.offset --out /tmp/sorted.offset 
```

```
USAGE:
    feedrick sort [FLAGS] --in <in> --out <out>

FLAGS:
    -h, --help         Prints help information
        --overwrite    Overwrite output file, if it exists.
    -V, --version      Prints version information

OPTIONS:
    -i, --in <in>      source offset log file
    -o, --out <out>    destination path
```

## Build

```
git clone https://github.com/sunrise-choir/feedrick
cd feedrick
cargo build --release

./target/release/feedrick view ~/.ssb/log.offset
```
