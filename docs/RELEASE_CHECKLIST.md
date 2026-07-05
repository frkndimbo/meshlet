# Meshlet v0.4 Release Checklist

Use this checklist before tagging or merging a v0.4 release branch. Run commands from the repository root unless a step says otherwise.

## Automated Gates

- [ ] `rtk cargo fmt --check`
- [ ] `rtk cargo check`
- [ ] `rtk cargo build`
- [ ] `rtk cargo test`

## Fresh Local Smoke

Use a temporary repo so local `.meshlet/` state is not reused:

```bash
rtk run 'rm -rf /tmp/meshlet-v04-smoke && mkdir -p /tmp/meshlet-v04-smoke && cp target/debug/meshlet /tmp/meshlet-v04-smoke/meshlet && cd /tmp/meshlet-v04-smoke && ./meshlet init'
```

- [ ] Append a private marker:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && ./meshlet event append --type context.added --json "{\"label\":\"private-needle-v04\"}"'
```

- [ ] Append a public marker:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && ./meshlet event append --type context.added --visibility public --profile public-safe --json "{\"label\":\"public-needle-v04\"}"'
```

- [ ] Verify event chain:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && ./meshlet verify'
```

- [ ] Append public evidence with a local path:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && echo "dummy public evidence" > evidence.txt && ./meshlet event append --type evidence.attached --visibility public --profile public-safe --json "{\"path\":\"/tmp/meshlet-v04-smoke/evidence.txt\",\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}"'
```

- [ ] Verify public doctor:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && ./meshlet doctor public'
```

## Public Export Smoke

- [ ] Write JSON public export:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && ./meshlet export public --out /tmp/meshlet-v04-smoke/public.json'
```

- [ ] Write OKF public export and run OKF doctor:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && ./meshlet export public --format okf --out /tmp/meshlet-v04-smoke/okf && ./meshlet okf doctor /tmp/meshlet-v04-smoke/okf'
```

- [ ] Confirm public marker exists and private marker is absent:

```bash
rtk run 'grep -R "public-needle-v04" /tmp/meshlet-v04-smoke/public.json /tmp/meshlet-v04-smoke/okf && ! grep -R "private-needle-v04" /tmp/meshlet-v04-smoke/public.json /tmp/meshlet-v04-smoke/okf'
```

- [ ] Confirm public evidence exports omit the local path:

```bash
rtk run '! grep -R "/tmp/meshlet-v04-smoke/evidence.txt" /tmp/meshlet-v04-smoke/public.json /tmp/meshlet-v04-smoke/okf'
```

## MCP Public-Safe Smoke

- [ ] Public-safe digest returns compact public data:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && printf "%s\n" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"meshlet_get_digest\",\"arguments\":{\"limit\":10}}}" | ./meshlet serve --mcp stdio --profile public-safe | grep "public-needle-v04"'
```

- [ ] Public-safe mutation rejects with JSON-RPC error:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && printf "%s\n" "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"meshlet_publish_event\",\"arguments\":{\"type\":\"context.added\",\"visibility\":\"public\",\"payload\":{\"label\":\"must-reject\"}}}}" | ./meshlet serve --mcp stdio --profile public-safe | grep "\"error\""'
```

- [ ] Confirm rejected mutation did not append:

```bash
rtk run 'cd /tmp/meshlet-v04-smoke && ./meshlet event list --mode full > /tmp/meshlet-v04-smoke/events.json && ! grep "must-reject" /tmp/meshlet-v04-smoke/events.json'
```

## Cleanup

- [ ] Remove temporary smoke directory:

```bash
rtk run 'rm -rf /tmp/meshlet-v04-smoke'
```
