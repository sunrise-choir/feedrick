# feedrick

feedrick is an experimental utility for (non-destructive) operations on
flumedb offset logs containing secure scuttlebutt feeds.

Currently implemented:

- Very basic log viewer
```
feedrick view /tmp/log.offset
```

- "Extract" (copy) a single feed from a source log to a new log

```
feedrick extract --in /tmp/log.offset --out /tmp/me.offset --feed "@N/vWpVVdD1e8IbACUQE4EVGL6+aodQfbQZ8ByC+k79s=.ed25519"
```

## Build

```
git clone https://github.com/sunrise-choir/feedrick
cd feedrick
cargo build --release

./target/release/feedrick view ~/.ssb/log.offset
```
